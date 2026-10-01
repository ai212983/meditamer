//! Stage 1 image gallery: streams pre-rendered full-panel frames off the SD
//! card and shows them, advancing on a swipe. Built to look at font specimens
//! on the real glass at the real viewing distance
//! (`docs/plans/font-legibility-improvements.md`, "Current specimen sizes"), but the
//! frame format is generic -- anything the host can rasterise to a panel
//! buffer can be viewed this way.
//!
//! Why SD rather than embedding the frames: a 3-bit frame is 180,000 bytes,
//! so four faces in both depths is ~900 KB. That fits `ota_0` (0x380000), but
//! every tweak would cost an Inkplate reflash. On SD a tweak is a file copy --
//! or a Wi-Fi upload, since `asset-upload-http` is in this target's default
//! features (`docs/guides/network/asset-upload.md`).
//!
//! Deliberately minimal in the same spirit as the updater: no LVGL, no
//! product runtime, no catalogue integration. It reuses the updater's own
//! `fat_io` and `sd_power` helpers verbatim (both are self-contained and
//! depend only on `sdcard` / `embedded_hal_async`), so the SD path here is
//! the one already exercised in production recovery.
//!
//! ## Frame format
//!
//! 16-byte header then a raw panel buffer, written by
//! `tools/font_probe/render_specimens.py`:
//!
//! ```text
//! 0..4   b"MDFR"
//! 4      version = 1
//! 5      format: 0 = binary (1bpp), 1 = Gray4 (4bpp)
//! 6..8   width  u16 LE
//! 8..10  height u16 LE
//! 10..16 reserved
//! ```
//!
//! Both payload layouts match `panel_waveform_fixture.rs`'s reference
//! patterns exactly -- binary is **LSB-first** within each byte
//! (`1 << (x % 8)`), and Gray4 packs two pixels per byte with the **high
//! nibble holding the even (left) x**, with the panel's eight physical levels
//! on the even values of the nibble. Getting either backwards produces a
//! mirrored or washed-out frame rather than an error, which is why the host
//! side carries the same note.
//!
//! Build and flash:
//!
//! ```text
//! This target is **ESP32**, not ESP32-S3, and `build.sh` takes a
//! profile vocabulary rather than cargo flags, so build the single bin
//! directly with the same environment it sets up:
//!
//! . "$HOME/export-esp.sh"
//! export CROSS_COMPILE=xtensa-esp32-elf
//! export LV_SYSROOT="$(xtensa-esp32-elf-gcc -print-sysroot)"
//! cd targets/meditamer-inkplate && CARGO_TARGET_DIR=./target \
//!   rustup run esp cargo build -Zbuild-std=core,alloc \
//!   --target xtensa-esp32-none-elf --release --no-default-features \
//!   --bin gallery
//! ESPFLASH_PORT=/dev/cu.usbserial-8310 espflash flash --chip esp32 \
//!   --partition-table config/partitions-single-production.csv \
//!   --target-app-partition ota_0 \
//!   targets/meditamer-inkplate/target/xtensa-esp32-none-elf/release/gallery
//! ```

#![no_std]
#![no_main]

use embassy_time::Timer;
use esp_backtrace as _;

// Panic breadcrumb hook (see `src/panic_crumb.rs`): every binary links
// esp-backtrace separately, so each needs its own copy of the symbol.
#[path = "../panic_crumb.rs"]
mod panic_crumb;
use esp_hal::clock::CpuClock;
use esp_hal::dma::aligned::DmaAlignedMut;
use esp_hal::dma::{DmaRxBuf, DmaTxBuf};
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::i2c::master::I2c;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use inkplate_tempera::{
    adapters::BusyDelay,
    expander::PcalExpander,
    frame::{self, Format, Header, HEADER_LEN},
    touch::InkplateTouch,
    InkplateHal, E_INK_HEIGHT, E_INK_WIDTH, FRAMEBUFFER_BYTES, GRAYSCALE_FRAMEBUFFER_BYTES,
};
use meditamer_product::firmware::psram;
use meditamer_product::firmware::types::{
    i2c_config, panel_i2c_device, shared_i2c_bus, shared_i2c_device, ConfiguredI2cDevice,
    SharedI2cBus,
};
use sdcard::{
    fat::{FatEngine, FatIoCompletion, FatRequest, FatStep},
    probe::{DefaultSdSpi, SdCardProbe},
};
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

#[path = "../updater/fat_io.rs"]
mod fat_io;
#[path = "../updater/sd_power.rs"]
mod sd_power;

// The generated session names exact content-derived SD files and case IDs.
#[path = "../../specimens/calibration.rs"]
mod calibration;
use calibration::SLIDES;

/// The Elan controller reports in its own resolution (1152x1152 as read back
/// at init), not panel pixels, so samples are scaled before any threshold is
/// applied. A swipe is a horizontal release far enough from where it started:
/// 80 panel px is comfortably past a tap's jitter while still being easy.
const SWIPE_MIN_DX: i32 = 80;
const SWIPE_MAX_DY: i32 = 60;

struct Frame {
    format: Format,
    len: usize,
}

/// Parses the header and streams the payload into `dest`.
async fn load_slide<SPI>(
    probe: &mut SdCardProbe<'static, SPI>,
    engine: &mut FatEngine,
    path: &str,
    dest: &mut [u8],
) -> Option<Frame>
where
    SPI: sdcard::probe::SdSpiBus,
{
    let (path_bytes, path_len) = fat_io::encode_path(path)?;
    engine
        .start(FatRequest::Stream {
            path: path_bytes,
            path_len,
        })
        .ok()?;

    let mut completion = FatIoCompletion::Pending;
    let mut delivered = 0u32;
    let mut header = [0u8; HEADER_LEN];
    let mut header_seen = 0usize;
    let mut format = Format::Binary;
    let mut written = 0usize;

    loop {
        match engine.advance(completion) {
            FatStep::Io(action) => {
                completion = fat_io::execute_action(action, probe, engine, &[]).await;
            }
            FatStep::Continue | FatStep::Yield => {
                let now = engine.stream_bytes_delivered();
                if now > delivered {
                    let chunk_len = engine.stream_chunk_len() as usize;
                    let chunk = &engine.workspace().sector[..chunk_len];
                    let mut offset = 0usize;
                    if header_seen < HEADER_LEN {
                        let take = (HEADER_LEN - header_seen).min(chunk.len());
                        header[header_seen..header_seen + take].copy_from_slice(&chunk[..take]);
                        header_seen += take;
                        offset = take;
                        if header_seen == HEADER_LEN {
                            match Header::parse(&header) {
                                Ok(parsed) => format = parsed.format,
                                Err(err) => {
                                    console::println!(
                                        "GALLERY_BAD_HEADER path={} err={:?}",
                                        path,
                                        err
                                    );
                                    return None;
                                }
                            }
                        }
                    }
                    let body = &chunk[offset..];
                    let room = dest.len().saturating_sub(written);
                    let take = body.len().min(room);
                    dest[written..written + take].copy_from_slice(&body[..take]);
                    written += take;
                    delivered = now;
                }
                completion = FatIoCompletion::Pending;
            }
            FatStep::Complete(result) => {
                if let sdcard::fat::FatResult::Error(err) = result {
                    console::println!("GALLERY_STREAM_ERROR path={} err={:?}", path, err);
                    return None;
                }
                break;
            }
        }
    }
    Some(Frame {
        format,
        len: written,
    })
}

/// Touch samples arrive in the controller's own coordinate space; map them
/// onto panel pixels so the thresholds below mean what they say.
fn scale_to_panel(p: inkplate_tempera::TouchPoint) -> (u16, u16) {
    const TOUCH_RES: u32 = 1152;
    let x = (p.x as u32 * E_INK_WIDTH as u32 / TOUCH_RES).min(E_INK_WIDTH as u32 - 1);
    let y = (p.y as u32 * E_INK_HEIGHT as u32 / TOUCH_RES).min(E_INK_HEIGHT as u32 - 1);
    (x as u16, y as u16)
}

/// -1 previous, +1 next, 0 nothing yet. Blocks until a gesture completes.
async fn wait_for_gesture<I2C>(touch: &mut InkplateTouch<I2C>) -> i32
where
    I2C: embedded_hal_async::i2c::I2c,
{
    let mut down: Option<(u16, u16)> = None;
    let mut last: (u16, u16) = (0, 0);
    loop {
        let sample = match touch.read_sample(0).await {
            Ok(sample) => sample,
            Err(_) => {
                Timer::after_millis(40).await;
                continue;
            }
        };
        if sample.touch_count > 0 {
            let p = scale_to_panel(sample.points[0]);
            if down.is_none() {
                down = Some(p);
            }
            last = p;
        } else if let Some(start) = down.take() {
            let dx = last.0 as i32 - start.0 as i32;
            let dy = last.1 as i32 - start.1 as i32;
            console::println!(
                "GALLERY_SWIPE start=({},{}) end=({},{}) dx={} dy={}",
                start.0,
                start.1,
                last.0,
                last.1,
                dx,
                dy
            );
            if dx.abs() >= SWIPE_MIN_DX && dy.abs() <= SWIPE_MAX_DY {
                // Swiping left drags the current image away to the left,
                // which reads as "next".
                return if dx < 0 { 1 } else { -1 };
            }
        }
        Timer::after_millis(25).await;
    }
}

#[embassy_executor::task]
async fn gallery_task(
    i2c: I2c<'static, esp_hal::Blocking>,
    sd_spi: DefaultSdSpi<'static>,
    sd_cs: Output<'static>,
) {
    static I2C_BUS: StaticCell<SharedI2cBus> = StaticCell::new();
    let i2c_bus = I2C_BUS.init(shared_i2c_bus(i2c));

    static PCAL: StaticCell<
        embassy_sync::mutex::Mutex<
            embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
            PcalExpander<ConfiguredI2cDevice<400>>,
        >,
    > = StaticCell::new();
    let expander = PCAL.init(embassy_sync::mutex::Mutex::new(PcalExpander::new(
        panel_i2c_device(i2c_bus),
    )));

    let mut display = match InkplateHal::new(panel_i2c_device(i2c_bus), BusyDelay::new(), expander)
    {
        Ok(driver) => driver,
        Err(_) => {
            console::println!("GALLERY_PANEL_INIT_FAILED");
            return;
        }
    };
    // The TPS65186 at 0x48 does not ACK until `init_core` has configured the
    // expander pins and pulsed WAKEUP; without it every display call fails
    // with AcknowledgeCheckFailed.
    if display.init_core().await.is_err() {
        console::println!("GALLERY_PANEL_INIT_CORE_FAILED");
        return;
    }

    let mut touch = InkplateTouch::new(shared_i2c_device(i2c_bus));
    match touch.init_with_status().await {
        Ok(status) => console::println!("GALLERY_TOUCH status={:?}", status),
        Err(_) => console::println!("GALLERY_TOUCH init_error (corner tap still works)"),
    }

    // One PSRAM buffer, sized for the larger of the two formats.
    let buffer = match psram::alloc_large_byte_buffer(GRAYSCALE_FRAMEBUFFER_BYTES) {
        Ok(buffer) => buffer,
        Err(error) => {
            console::println!("GALLERY_PSRAM_FAILED err={:?}", error);
            return;
        }
    };
    let rotated_buffer = match psram::alloc_large_byte_buffer(GRAYSCALE_FRAMEBUFFER_BYTES) {
        Ok(buffer) => buffer,
        Err(error) => {
            console::println!("GALLERY_PSRAM_ROT_FAILED err={:?}", error);
            return;
        }
    };
    console::println!(
        "GALLERY_PSRAM placement={:?} rotated={:?}",
        buffer.placement(),
        rotated_buffer.placement()
    );
    let frame = buffer.into_static_mut_slice();
    let rotated = rotated_buffer.into_static_mut_slice();

    let mut sd_device = shared_i2c_device(i2c_bus);
    if let Err(err) = sd_power::power_on(&mut sd_device).await {
        console::println!("GALLERY_SD_POWER_ERROR err={:?}", err);
        return;
    }
    Timer::after_millis(sdcard::SD_POWER_SETTLE_MS).await;

    let mut probe = SdCardProbe::new(sd_spi, sd_cs);
    match probe.init().await {
        Ok(status) => console::println!(
            "GALLERY_SD_OK capacity_bytes={} filesystem={:?}",
            status.capacity_bytes,
            status.filesystem
        ),
        Err(err) => {
            console::println!("GALLERY_SD_ERROR err={:?}", err);
            return;
        }
    }

    let mut engine = FatEngine::new();
    let mut index = 0usize;
    let mut misses = 0usize;

    loop {
        let (case_id, path) = SLIDES[index % SLIDES.len()];
        frame.fill(0);
        match load_slide(&mut probe, &mut engine, path, frame).await {
            Some(f) => {
                misses = 0;
                console::println!(
                    "GALLERY_SHOW case={} path={} format={:?} bytes={}",
                    case_id,
                    path,
                    f.format,
                    f.len
                );
                let result = match f.format {
                    Format::Gray4 => {
                        frame::rotate_gray4(
                            &frame[..GRAYSCALE_FRAMEBUFFER_BYTES],
                            &mut rotated[..GRAYSCALE_FRAMEBUFFER_BYTES],
                        );
                        display
                            .display_gray4_async(&rotated[..GRAYSCALE_FRAMEBUFFER_BYTES], true)
                            .await
                    }
                    Format::Binary => {
                        frame::rotate_binary(
                            &frame[..FRAMEBUFFER_BYTES],
                            &mut rotated[..FRAMEBUFFER_BYTES],
                        );
                        display.framebuffer_bw_mut()[..FRAMEBUFFER_BYTES]
                            .copy_from_slice(&rotated[..FRAMEBUFFER_BYTES]);
                        display.display_bw_async(true).await
                    }
                };
                if result.is_err() {
                    console::println!("GALLERY_DISPLAY_ERROR path={}", path);
                }
                let step = wait_for_gesture(&mut touch).await;
                index = (index as i32 + step).rem_euclid(SLIDES.len() as i32) as usize;
            }
            None => {
                // Missing slide: skip it. Bail out if every slot is empty
                // rather than spinning on a card with no frames on it.
                misses += 1;
                if misses >= SLIDES.len() {
                    console::println!("GALLERY_NO_SLIDES dir=/assets/spec");
                    return;
                }
                index = (index + 1) % SLIDES.len();
            }
        }
    }
}

#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::_240MHz));

    let allocator_status = psram::init_allocator(peripherals.PSRAM);
    psram::log_allocator_status();
    if !matches!(allocator_status.state, psram::AllocatorState::Initialized) {
        console::println!("GALLERY_PSRAM_INIT_FAILED status={:?}", allocator_status);
    }

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);
    console::println!("GALLERY board=inkplate-tempera chip=esp32");

    let i2c = I2c::new(peripherals.I2C0, i2c_config(100))
        .expect("failed to init I2C0")
        .with_sda(peripherals.GPIO21)
        .with_scl(peripherals.GPIO22);

    let sd_spi_cfg = SpiConfig::default()
        .with_frequency(Rate::from_khz(400))
        .with_mode(SpiMode::_0);
    let sd_spi = Spi::new(peripherals.SPI2, sd_spi_cfg)
        .expect("failed to init SPI2 for SD")
        .with_sck(peripherals.GPIO14)
        .with_mosi(peripherals.GPIO13)
        .with_miso(peripherals.GPIO12);
    let sd_spi = {
        let (rx_buffer, rx_descriptors, tx_buffer, tx_descriptors) = esp_hal::dma_buffers!(
            sdcard::probe::SD_DMA_BUFFER_SIZE,
            sdcard::probe::SD_DMA_BUFFER_SIZE
        );
        let rx_descriptors =
            DmaAlignedMut::new(rx_descriptors).expect("misaligned SPI2 DMA RX descriptors");
        let rx_buffer = DmaAlignedMut::new(rx_buffer).expect("misaligned SPI2 DMA RX buffer");
        let tx_descriptors =
            DmaAlignedMut::new(tx_descriptors).expect("misaligned SPI2 DMA TX descriptors");
        let tx_buffer = DmaAlignedMut::new(tx_buffer).expect("misaligned SPI2 DMA TX buffer");
        let rx = DmaRxBuf::new(rx_descriptors, rx_buffer).expect("SPI2 DMA RX buffer");
        let tx = DmaTxBuf::new(tx_descriptors, tx_buffer).expect("SPI2 DMA TX buffer");
        sd_spi
            .with_dma(peripherals.DMA_SPI2)
            .with_buffers(rx, tx)
            .into_async()
    };
    let sd_cs = Output::new(peripherals.GPIO15, Level::High, OutputConfig::default());

    static EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();
    let executor = EXECUTOR.init(esp_rtos::embassy::Executor::new());
    executor.run(move |spawner| {
        spawner.spawn(gallery_task(i2c, sd_spi, sd_cs).expect("gallery task pool exhausted"));
    })
}
