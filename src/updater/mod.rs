//! Factory updater (ADR-0014 / docs/plans/single-production-sd-recovery-updater.md).
//!
//! Boots, identifies the running build and OTA layout, powers and mounts
//! SD, streams one candidate bundle from bounded buffers and verifies it
//! (magic, target, layout, length ceiling, ed25519 signature, SHA-256
//! digest — `bundle_stream`), and — Phase 3 — installs it: records the
//! attempted digest, erases and rewrites `ota_0`, verifies the flash
//! read-back, and activates exactly one `New` OTA record (`install`). See
//! `install`'s module doc for the full sequencing and why each step is
//! ordered the way it is; `attempted` for why a reinstall of the same
//! digest is blocked without a new explicit request.
//!
//! Deliberately does not reuse `crate::platform::inkplate::InkplateHal`: that
//! driver allocates the e-ink panel's static framebuffer and waveform tables
//! on construction, which would defeat the point of a size-minimal updater.
//! `sd_power` reimplements only the one IO-expander pin this binary needs.

#[cfg(not(feature = "sd-qual-push"))]
use bundle::HEADER_LEN;
#[cfg(not(feature = "sd-qual-push"))]
use ed25519_dalek::VerifyingKey;
use embassy_embedded_hal::adapter::BlockingAsync;
#[cfg(not(feature = "sd-qual-push"))]
use embassy_time::Timer;
#[cfg(feature = "sd-qual-push")]
use esp_hal::uart::{Config as UartConfig, RxConfig, Uart};
use esp_hal::{
    clock::CpuClock,
    dma::{DmaRxBuf, DmaTxBuf},
    gpio::{Level, Output, OutputConfig},
    i2c::master::{Config as I2cConfig, I2c, SoftwareTimeout},
    interrupt::software::SoftwareInterruptControl,
    rtc_cntl::{Rtc, RwdtStage},
    spi::{
        master::{Config as SpiConfig, Spi},
        Mode as SpiMode,
    },
    time::{Duration as HalDuration, Rate},
    timer::timg::TimerGroup,
    Blocking,
};
#[cfg(not(feature = "sd-qual-push"))]
use sdcard::fat::FatEngine;
use sdcard::probe::DefaultSdSpi;
#[cfg(not(feature = "sd-qual-push"))]
use sdcard::probe::SdCardProbe;
use static_cell::StaticCell;

#[cfg(not(feature = "sd-qual-push"))]
mod attempted;
#[cfg(not(feature = "sd-qual-push"))]
mod bundle_stream;
mod fat_io;
#[cfg(not(feature = "sd-qual-push"))]
mod install;
mod sd_power;
#[cfg(feature = "sd-qual-push")]
mod sd_push;
mod status;

include!(concat!(env!("OUT_DIR"), "/ota_build_config.rs"));

// OTA_PUBLIC_KEY is otherwise unused under sd-qual-push — only the normal
// verify path, gated out for that build variant, reads it. An `#[allow]` on
// the `include!` above can't reach the const items it expands to, so force
// a use instead. OTA_BUILD_ID and OTA_PUBLIC_KEY_CONFIGURED are used
// unconditionally by run()'s boot line and need no such shim.
#[cfg(feature = "sd-qual-push")]
const _: [u8; 32] = OTA_PUBLIC_KEY;

/// Hardware target this updater accepts bundles for. There is only one
/// board today; a second would get its own id rather than reusing this one
/// under different assumptions.
const TARGET_ID: u16 = 1;
/// Partition layout this updater accepts bundles for. Bumped whenever the
/// accepted flash layout changes shape (ADR-0014 Phase 2 introduces layout
/// 1 itself — the single-`ota_0` map this updater is written against).
const LAYOUT_ID: u16 = 1;
/// Payload-length ceiling: the single-production `ota_0` capacity
/// (`config/partitions-single-production.csv`, matching
/// `SINGLE_PRODUCTION_OTA_0_SIZE` in `src/firmware/update.rs`), less the
/// same headroom margin the pre-Phase-5 A/B ceiling used.
#[cfg(not(feature = "sd-qual-push"))]
const MAX_FIRMWARE_LEN: u32 = 0x38_0000 - 0x2000;
/// Fixed staging name for the one candidate bundle Phase 1 looks for. Phase
/// 3 owns the temporary-name/publish/rename protocol
/// (docs/plans/single-production-sd-recovery-updater.md); this is the
/// simplest thing that lets Phase 1 measure a real read+verify pass.
const BUNDLE_PATH: &str = "/UPDATE.BIN";
/// RWDT stage-0 timeout: long enough for a full bundle stream over a slow
/// card, short enough that a wedged SPI/I2C transaction still recovers.
const WATCHDOG_TIMEOUT: HalDuration = HalDuration::from_secs(30);

pub fn run() -> ! {
    let hal_config = esp_hal::Config::default().with_cpu_clock(CpuClock::_240MHz);
    let peripherals = esp_hal::init(hal_config);

    console::println!(
        "UPDATER_BOOT build_id={} key_configured={} target={} layout={}",
        OTA_BUILD_ID,
        OTA_PUBLIC_KEY_CONFIGURED,
        TARGET_ID,
        LAYOUT_ID,
    );

    let mut rtc = Rtc::new(peripherals.LPWR);
    rtc.rwdt.set_timeout(RwdtStage::Stage0, WATCHDOG_TIMEOUT);
    rtc.rwdt.enable();

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let software_interrupts = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, software_interrupts.software_interrupt0);

    crate::firmware::flash::initialize(peripherals.FLASH);
    match crate::firmware::update::status() {
        Ok(ota_status) => status::print_ota_status(&ota_status),
        Err(err) => console::println!("UPDATER_OTA_STATUS_ERROR err={:?}", err),
    }

    let i2c_cfg = I2cConfig::default()
        .with_frequency(Rate::from_khz(100))
        .with_software_timeout(SoftwareTimeout::Transaction(HalDuration::from_millis(40)));
    let i2c = I2c::new(peripherals.I2C0, i2c_cfg)
        .expect("failed to init I2C0")
        .with_sda(peripherals.GPIO21)
        .with_scl(peripherals.GPIO22);
    let i2c = BlockingAsync::new(i2c);

    let sd_spi_cfg = SpiConfig::default()
        .with_frequency(Rate::from_khz(400))
        .with_mode(SpiMode::_0);
    let sd_spi = Spi::new(peripherals.SPI2, sd_spi_cfg)
        .expect("failed to init SPI2 for SD probe")
        .with_sck(peripherals.GPIO14)
        .with_mosi(peripherals.GPIO13)
        .with_miso(peripherals.GPIO12);
    let sd_spi = {
        let (rx_buffer, rx_descriptors, tx_buffer, tx_descriptors) = esp_hal::dma_buffers!(
            sdcard::probe::SD_DMA_BUFFER_SIZE,
            sdcard::probe::SD_DMA_BUFFER_SIZE
        );
        let rx = DmaRxBuf::new(rx_descriptors, rx_buffer)
            .expect("failed to allocate SPI2 DMA RX buffer");
        let tx = DmaTxBuf::new(tx_descriptors, tx_buffer)
            .expect("failed to allocate SPI2 DMA TX buffer");
        sd_spi
            .with_dma(peripherals.DMA_SPI2)
            .with_buffers(rx, tx)
            .into_async()
    };
    let sd_cs = Output::new(peripherals.GPIO15, Level::High, OutputConfig::default());

    // Bench/qualification-only (see sd_push's module doc): a dedicated
    // UART0 instance to receive a pushed bundle. This build variant never
    // reaches the normal bundle_task, so there's no console-vs-transfer
    // baud contention to worry about the way an always-on receiver would
    // have.
    #[cfg(feature = "sd-qual-push")]
    let uart = {
        let uart_cfg = UartConfig::default()
            .with_baudrate(115_200)
            .with_rx(RxConfig::default().with_fifo_full_threshold(64));
        Uart::new(peripherals.UART0, uart_cfg)
            .expect("failed to init UART0 for sd-qual-push")
            .with_rx(peripherals.GPIO3)
            .with_tx(peripherals.GPIO1)
    };

    // embassy_time::Timer needs a real Embassy executor behind its waker
    // (the generic timer queue is not enabled) — a bare `block_on` poll
    // loop panics the first time anything here awaits a Timer. `esp_rtos`
    // already registered the timer driver above; this just gives it an
    // executor to hand wakers to, the same pattern `src/firmware/system.rs`
    // uses for its own board_runtime_task.
    static EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();
    let executor = EXECUTOR.init(esp_rtos::embassy::Executor::new());
    executor.run(move |spawner| {
        #[cfg(feature = "sd-qual-push")]
        spawner.spawn(
            sd_push_task(i2c, sd_spi, sd_cs, uart, rtc)
                .expect("factory updater task pool exhausted"),
        );
        #[cfg(not(feature = "sd-qual-push"))]
        spawner.spawn(
            bundle_task(i2c, sd_spi, sd_cs, rtc).expect("factory updater task pool exhausted"),
        );
    })
}

#[cfg(feature = "sd-qual-push")]
#[embassy_executor::task]
async fn sd_push_task(
    mut i2c: BlockingAsync<I2c<'static, Blocking>>,
    sd_spi: DefaultSdSpi<'static>,
    sd_cs: Output<'static>,
    uart: Uart<'static, Blocking>,
    mut rtc: Rtc<'static>,
) {
    sd_push::run(&mut i2c, sd_spi, sd_cs, uart, &mut rtc, BUNDLE_PATH).await
}

#[cfg(not(feature = "sd-qual-push"))]
#[embassy_executor::task]
async fn bundle_task(
    mut i2c: BlockingAsync<I2c<'static, Blocking>>,
    sd_spi: DefaultSdSpi<'static>,
    sd_cs: Output<'static>,
    mut rtc: Rtc<'static>,
) {
    confirm_if_pending_candidate().await;
    read_verify_and_install_bundle(&mut i2c, sd_spi, sd_cs, &mut rtc).await;

    loop {
        rtc.rwdt.feed();
        // Nothing left to act on for this boot (either there was no
        // installable candidate, or install failed and left otadata
        // erased — a responsive updater per invariant 5 — or install
        // succeeded and the reboot below already fired and this loop never
        // runs). Idle while keeping the watchdog satisfied and the UART
        // responsive for a console attached over USB.
        Timer::after_secs(5).await;
    }
}

#[cfg(not(feature = "sd-qual-push"))]
async fn read_verify_and_install_bundle<I2C, SPI>(
    i2c: &mut I2C,
    sd_spi: SPI,
    sd_cs: Output<'static>,
    rtc: &mut Rtc<'static>,
) where
    I2C: embedded_hal_async::i2c::I2c,
    SPI: sdcard::probe::SdSpiBus,
{
    const _: () = assert!(HEADER_LEN <= sdcard::probe::SD_SECTOR_SIZE);

    if let Err(err) = sd_power::power_on(i2c).await {
        console::println!("UPDATER_SD_POWER_ERROR err={:?}", err);
        return;
    }
    Timer::after_millis(sdcard::power::SD_POWER_SETTLE_MS).await;

    let mut probe = SdCardProbe::new(sd_spi, sd_cs);
    let probe_status = match probe.init().await {
        Ok(status) => status,
        Err(err) => {
            console::println!("UPDATER_SD_PROBE_ERROR err={:?}", err);
            return;
        }
    };
    console::println!(
        "UPDATER_SD_PROBE_OK capacity_bytes={} filesystem={:?}",
        probe_status.capacity_bytes,
        probe_status.filesystem,
    );

    let Ok(public_key) = VerifyingKey::from_bytes(&OTA_PUBLIC_KEY) else {
        console::println!(
            "UPDATER_BUNDLE_ERROR path={} reason=SigningKey",
            BUNDLE_PATH
        );
        return;
    };
    if !OTA_PUBLIC_KEY_CONFIGURED {
        console::println!(
            "UPDATER_BUNDLE_ERROR path={} reason=SigningKeyUnconfigured",
            BUNDLE_PATH
        );
        return;
    }

    let mut engine = FatEngine::new();
    let bundle = match bundle_stream::stream_and_verify(
        &mut probe,
        &mut engine,
        BUNDLE_PATH,
        TARGET_ID,
        LAYOUT_ID,
        MAX_FIRMWARE_LEN,
        &public_key,
    )
    .await
    {
        Ok(bundle) => {
            status::print_bundle_ok(BUNDLE_PATH, &bundle);
            bundle
        }
        Err(err) => {
            status::print_bundle_error(BUNDLE_PATH, &err);
            return;
        }
    };

    let attempted = attempted::read_attempted_digest(&mut probe, &mut engine).await;
    if attempted::blocks_reinstall(attempted, &bundle.header.firmware_digest) {
        console::println!(
            "UPDATER_INSTALL_BLOCKED path={} reason=already_attempted",
            BUNDLE_PATH
        );
        return;
    }

    match install::run(&mut probe, &mut engine, rtc, BUNDLE_PATH, &bundle.header).await {
        Ok(()) => {
            console::println!(
                "UPDATER_INSTALL_OK path={} bytes={}",
                BUNDLE_PATH,
                bundle.total_bytes
            );
            esp_hal::system::software_reset();
        }
        Err(err) => {
            console::println!(
                "UPDATER_INSTALL_ERROR path={} reason={:?}",
                BUNDLE_PATH,
                err
            );
        }
    }
}

/// If this binary is running as the `ota_0` candidate awaiting confirmation
/// (`OtaImageState::PendingVerify` — the bootloader sets this automatically
/// on first boot after a complete USB flash's `New` record, or after a
/// runtime SD install activates one), confirms it after a settle delay,
/// matching the production firmware's own `RUNTIME_READY` + 5s window
/// (`src/firmware/update.rs::firmware_health_task`). A no-op when running
/// from `factory` (`status.image_state` reflects `ota_0`'s candidate state
/// regardless of which partition is actually booted, so this only acts once
/// `booted == Ota0` too).
///
/// Exists so this binary can stand in for a real production image when
/// proving the ADR-0014 Phase 2 boot-state matrix on hardware, without
/// needing the full default firmware build — separately unreliable to flash
/// on this bench setup, see ADR-0014's Hardware boot section — to also be
/// layout-aware at the same time.
#[cfg(not(feature = "sd-qual-push"))]
async fn confirm_if_pending_candidate() {
    let Ok(status) = crate::firmware::update::status() else {
        return;
    };
    if status.booted != crate::firmware::update::Slot::Ota0
        || status.image_state != Some(esp_bootloader_esp_idf::ota::OtaImageState::PendingVerify)
    {
        return;
    }
    console::println!("UPDATER_CANDIDATE_PENDING settle_ms=5000");
    Timer::after_secs(5).await;
    match crate::firmware::update::confirm_pending_image() {
        Ok(confirmed) => console::println!("UPDATER_CANDIDATE_CONFIRM confirmed={confirmed}"),
        Err(err) => console::println!("UPDATER_CANDIDATE_CONFIRM_ERROR err={:?}", err),
    }
}
