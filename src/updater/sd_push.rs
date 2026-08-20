//! Bench/qualification-only bundle transport (ADR-0014 Phase 4).
//!
//! **Not part of the production delivery path.** The device's SD card is not
//! reachable without disassembly, so real bundle delivery to a fielded
//! device is WiFi or BLE (a separate, out-of-scope feature) — this exists
//! solely so `hostctl` can stage a signed bundle onto a bench board's SD
//! card over the same serial link already used for console/log capture,
//! without opening the case. `sd-qual-push` is a distinct build feature
//! (composes with, but is not implied by, `factory-updater`): a board is
//! flashed with this variant only long enough to receive one bundle, then
//! reflashed with the normal updater build to actually exercise the real
//! verify/install/confirm/interruption paths Phase 3 proved. This module
//! never runs in a shipped updater.
//!
//! Wire protocol, chosen after a hardware investigation (ADR-0014 Phase 3's
//! "digest mismatch that was the test harness, not the updater" section):
//! magic `b"SDQP"`, a little-endian `u32` total length, then the payload in
//! fixed [`CHUNK_BYTES`] pieces — each written to SD before this side prints
//! one `UPDATER_SDPUSH_CHUNK_OK n=<i>` text line, which the host regex-waits
//! for (`hostctl`'s existing `SerialConsole`, the same one every other
//! workflow uses) before sending the next piece. That per-chunk ack is
//! load-bearing, not cosmetic: a free-running host transmitting at full
//! UART rate while this task is blocked for hundreds of milliseconds inside
//! a single SD write overflows the ESP32's 128-byte hardware RX FIFO and
//! silently drops bytes — same total length, wrong content. Pacing to one
//! chunk in flight at a time means the FIFO can never be asked to hold more
//! than it can.

use esp_hal::{rtc_cntl::Rtc, uart::Uart, Blocking};
use sdcard::{
    fat::{FatEngine, FatIoCompletion, FatPayloadId, FatRequest, FatResult, FatStep},
    probe::{SdCardProbe, SdSpiBus},
};

use super::fat_io::{encode_path, execute_action};

const MAGIC: [u8; 4] = *b"SDQP";
const CHUNK_BYTES: usize = 8192;

/// Runs forever: power SD on and mount it once, then repeatedly accept one
/// pushed bundle per magic-prefixed frame, writing each to `path`
/// (overwriting any previous content — `FatRequest::Write` truncates). Never
/// returns; the operator power-cycles or reflashes when done staging. Feeds
/// `rtc`'s watchdog every chunk — at 400kHz SPI a large future bundle could
/// otherwise run well past the 30s RWDT timeout mid-transfer.
pub(super) async fn run<I2C, SPI>(
    i2c: &mut I2C,
    sd_spi: SPI,
    sd_cs: esp_hal::gpio::Output<'static>,
    mut uart: Uart<'static, Blocking>,
    rtc: &mut Rtc<'static>,
    path: &str,
) -> !
where
    I2C: embedded_hal_async::i2c::I2c,
    SPI: SdSpiBus,
{
    if let Err(err) = super::sd_power::power_on(i2c).await {
        console::println!("UPDATER_SDPUSH_SD_POWER_ERROR err={:?}", err);
        loop_forever().await;
    }
    embassy_time::Timer::after_millis(sdcard::power::SD_POWER_SETTLE_MS).await;

    let mut probe = SdCardProbe::new(sd_spi, sd_cs);
    match probe.init().await {
        Ok(status) => console::println!(
            "UPDATER_SDPUSH_SD_PROBE_OK capacity_bytes={} filesystem={:?}",
            status.capacity_bytes,
            status.filesystem
        ),
        Err(err) => {
            console::println!("UPDATER_SDPUSH_SD_PROBE_ERROR err={:?}", err);
            loop_forever().await;
        }
    }
    console::println!("UPDATER_SDPUSH_READY");

    let mut buf = [0u8; CHUNK_BYTES];
    loop {
        // Non-blocking, watchdog-fed wait: a push may not start for a long
        // time after boot (an operator triggers it manually from hostctl),
        // well past the RWDT timeout if this waited the way the rest of
        // this loop's reads do — `Uart::read` blocks synchronously until at
        // least one byte arrives, with no watchdog feed in between.
        wait_for_magic(&mut uart, rtc).await;
        let mut len_bytes = [0u8; 4];
        read_exact(&mut uart, &mut len_bytes).await;
        let total_len = u32::from_le_bytes(len_bytes) as usize;
        console::println!("UPDATER_SDPUSH_RECEIVING total_len={total_len}");

        let mut engine = FatEngine::new();
        let mut written = 0usize;
        let mut chunk_num = 0u32;
        let mut ok = true;
        while written < total_len {
            rtc.rwdt.feed();
            let take = (total_len - written).min(CHUNK_BYTES);
            read_exact(&mut uart, &mut buf[..take]).await;
            let result = if written == 0 {
                write_or_append(&mut probe, &mut engine, path, &buf[..take], true).await
            } else {
                write_or_append(&mut probe, &mut engine, path, &buf[..take], false).await
            };
            match result {
                Ok(()) => written += take,
                Err(()) => {
                    console::println!("UPDATER_SDPUSH_CHUNK_ERROR n={chunk_num} written={written}");
                    ok = false;
                    break;
                }
            }
            console::println!("UPDATER_SDPUSH_CHUNK_OK n={chunk_num}");
            chunk_num += 1;
        }
        console::println!("UPDATER_SDPUSH_DONE path={path} bytes={written} ok={ok}");
    }
}

async fn loop_forever() -> ! {
    loop {
        embassy_time::Timer::after_secs(5).await;
    }
}

/// Polls for [`MAGIC`] one byte at a time without ever blocking on the UART
/// for long, feeding `rtc`'s watchdog and yielding to the executor between
/// polls. Checks readiness with `read_ready()` (a plain non-consuming
/// register peek) and only calls the blocking single-byte `read()` — the
/// same primitive `read_exact` uses everywhere else in this module — once
/// it's true, rather than `read_buffered()`: that reads at most `buf.len()`
/// bytes per call regardless of how many are actually queued, so with a
/// 1-byte buffer it only ever drains the backlog one byte per poll interval
/// no matter how much has piled up — confirmed on hardware to deliver
/// corrupted bytes under exactly that backlog (debug instrumentation
/// caught a mismatched byte immediately after the first correctly-matched
/// one; switching to this `read_ready` + `read_exact` pairing fixed it).
async fn wait_for_magic(uart: &mut Uart<'static, Blocking>, rtc: &mut Rtc<'static>) {
    let mut matched = 0usize;
    loop {
        rtc.rwdt.feed();
        if uart.read_ready() {
            let mut byte = [0u8; 1];
            read_exact(uart, &mut byte).await;
            if byte[0] == MAGIC[matched] {
                matched += 1;
                if matched == MAGIC.len() {
                    return;
                }
            } else {
                matched = 0;
            }
            continue;
        }
        embassy_time::Timer::after_millis(10).await;
    }
}

async fn read_exact(uart: &mut Uart<'static, Blocking>, buf: &mut [u8]) {
    let mut filled = 0;
    while filled < buf.len() {
        match uart.read(&mut buf[filled..]) {
            Ok(0) | Err(_) => {}
            Ok(n) => filled += n,
        }
    }
}

async fn write_or_append<'d, SPI>(
    probe: &mut SdCardProbe<'d, SPI>,
    engine: &mut FatEngine,
    path: &str,
    data: &[u8],
    truncate: bool,
) -> Result<(), ()>
where
    SPI: SdSpiBus,
{
    let (path_bytes, path_len) = encode_path(path).ok_or(())?;
    let request = if truncate {
        FatRequest::Write {
            path: path_bytes,
            path_len,
            input: FatPayloadId::Primary,
            input_len: data.len() as u32,
        }
    } else {
        FatRequest::Append {
            path: path_bytes,
            path_len,
            input: FatPayloadId::Primary,
            input_len: data.len() as u32,
        }
    };
    engine.start(request).map_err(|_| ())?;
    let mut completion = FatIoCompletion::Pending;
    loop {
        match engine.advance(completion) {
            FatStep::Io(action) => completion = execute_action(action, probe, engine, data).await,
            FatStep::Continue | FatStep::Yield => completion = FatIoCompletion::Pending,
            FatStep::Complete(FatResult::Done) => return Ok(()),
            FatStep::Complete(_) => return Err(()),
        }
    }
}
