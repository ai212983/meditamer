//! Display the configured calibration session, cycling every 12 seconds.
//! Host pages come from tools/font_probe/render_specimens.py and carry exact
//! case IDs plus normal/inverse samples. The generated calibration module
//! selects flash-resident frames; the existing framebuffer is reused.
//! Build with the target build.sh and flash through scripts/device/flash.sh.
//! See tools/font_probe/README.md for reproduction and session selection.

#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Level, Output, OutputConfig, Pull};
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::time::Rate;

use waveshare_rlcd42::panel;

/// esp-println's jtag-serial backend drops output written before the USB CDC
/// host finishes enumerating.
const CONSOLE_SETTLE_MS: u32 = 800;

/// How long each specimen stays on the glass. Long enough to walk back to
/// 80 cm and look at it properly.
const HOLD_MS: u32 = 12_000;

const PIN_SCK: u8 = 11;
const PIN_MOSI: u8 = 12;
const PIN_DC: u8 = 5;
const PIN_CS: u8 = 40;
const PIN_RST: u8 = 41;

/// Packed 1bpp, row-major, MSB first, 400x300 -- `Canvas::write_bin`.
const STRIDE: usize = (panel::WIDTH + 7) / 8;
const FRAME_BYTES: usize = STRIDE * panel::HEIGHT;

#[path = "../../specimens/calibration.rs"]
mod calibration;
use calibration::SPECIMENS;

fn build_panel<'d>(
    spi2: esp_hal::peripherals::SPI2<'d>,
    sck: esp_hal::peripherals::GPIO11<'d>,
    mosi: esp_hal::peripherals::GPIO12<'d>,
    dc: esp_hal::peripherals::GPIO5<'d>,
    cs: esp_hal::peripherals::GPIO40<'d>,
    rst: esp_hal::peripherals::GPIO41<'d>,
    framebuffer: &'d mut [u8; panel::FRAMEBUFFER_BYTES],
) -> panel::St7305<'d> {
    let spi = Spi::new(
        spi2,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(24))
            .with_mode(SpiMode::_0),
    )
    .expect("spi2")
    .with_sck(sck)
    .with_mosi(mosi);
    let output = OutputConfig::default();
    let idle_high_output = OutputConfig::default().with_pull(Pull::Up);
    panel::St7305::new(
        framebuffer,
        spi,
        Output::new(dc, Level::Low, output),
        Output::new(cs, Level::High, idle_high_output),
        Output::new(rst, Level::High, idle_high_output),
    )
}

/// Unpack one host frame into the panel's own framebuffer layout.
/// `panel::set_pixel` owns the native rotation and page packing.
fn draw(display: &mut panel::St7305<'_>, bits: &[u8; FRAME_BYTES]) {
    let framebuffer = display.framebuffer_mut();
    framebuffer.fill(0);
    for y in 0..panel::HEIGHT {
        for x in 0..panel::WIDTH {
            let on = bits[y * STRIDE + (x >> 3)] & (0x80 >> (x & 7)) != 0;
            panel::set_pixel(framebuffer, x, y, on);
        }
    }
}

#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    Delay::new().delay_millis(CONSOLE_SETTLE_MS);
    console::println!("SPECIMEN_PROBE board=waveshare-rlcd42 chip=esp32s3");
    console::println!(
        "SPECIMEN_PROBE pins sck={PIN_SCK} mosi={PIN_MOSI} dc={PIN_DC} cs={PIN_CS} rst={PIN_RST}"
    );
    console::println!(
        "SPECIMEN_PROBE panel={}x{} frame_bytes={FRAME_BYTES} specimens={}",
        panel::WIDTH,
        panel::HEIGHT,
        SPECIMENS.len()
    );

    static mut FRAMEBUFFER: [u8; panel::FRAMEBUFFER_BYTES] = [0; panel::FRAMEBUFFER_BYTES];
    // SAFETY: single-threaded, no executor started; only `main` touches this.
    let framebuffer = unsafe { &mut *core::ptr::addr_of_mut!(FRAMEBUFFER) };

    let mut display = build_panel(
        peripherals.SPI2,
        peripherals.GPIO11,
        peripherals.GPIO12,
        peripherals.GPIO5,
        peripherals.GPIO40,
        peripherals.GPIO41,
        framebuffer,
    );
    display.init();
    display.set_frame_rates(panel::HighPowerRate::Full, panel::LowPowerRate::Hz0_25);
    console::println!("SPECIMEN_PROBE stage=init_done");

    let mut i = 0usize;
    loop {
        let s = &SPECIMENS[i % SPECIMENS.len()];
        draw(&mut display, s.1);
        display.flush();
        console::println!("SPECIMEN_PROBE showing={} holding={HOLD_MS}ms", s.0);
        Delay::new().delay_millis(HOLD_MS);
        i += 1;
    }
}
