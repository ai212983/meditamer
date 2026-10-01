//! Smoke test: can a "Guru Meditation" crash screen be drawn **from inside
//! the panic handler itself**, with no reboot, on this board's ST7305 panel?
//!
//! This is deliberately narrow. It does not attempt to reproduce the
//! esp-rtos scheduler-corruption bug (`docs/archive/architecture/
//! 0014-single-production-sd-recovery-updater.md`, the two addenda) that
//! actually took Medinote down -- that fault originates *inside* interrupt
//! dispatch (`__level_1_interrupt`), a much less certain context than a
//! plain task-level `panic!()`. This probe answers the easier, prerequisite
//! question first: for an ordinary panic, is drawing from the panic handler
//! even viable at all -- can peripherals be re-acquired, can the panel driver
//! be re-initialised, does a synchronous SPI transfer complete -- before
//! spending effort on the harder case.
//!
//! **Method.** Boot normally, draw a known pattern (horizontal stripes) with
//! the panel driver used the ordinary way, hold it on screen, then call
//! `panic!()` deliberately. `esp-backtrace`'s `custom-pre-backtrace` hook
//! (fired before the panic banner prints, before the handler's final `loop
//! {}`) `unsafe`-steals a **second**, independent set of peripheral handles
//! for the exact same SPI2/GPIO pins main.rs uses, builds a fresh `St7305`
//! over a separate static framebuffer, re-runs the same `init()` sequence
//! every cold boot already exercises, and draws a different pattern
//! (checkerboard). No LVGL, no allocator, no embassy/esp-rtos -- this probe
//! never starts an executor at all, so there is nothing concurrent to race
//! against the panic-time draw.
//!
//! **What a result proves.** If the panel shows checkerboard after the
//! stripes hold, in-place panic-time drawing works for this class of panic:
//! peripheral-steal + fresh driver + blocking SPI is a viable foundation for
//! a real crash screen. It does **not** prove the interrupt-corruption case
//! works the same way -- that needs a separate, harder probe once this one
//! passes.
//!
//! **What a hang or garbage screen means.** The serial log below is
//! ordered and printed at every stage (steal, construct, hardware_reset,
//! init, set_frame_rates, flush) specifically so a hang shows exactly which
//! step it was in rather than just going dark. Compare against the last
//! line printed before the port went silent.
//!
//! Build and flash:
//!
//! ```text
//! targets/medinote-waveshare/build.sh --locked --bin panic-screen-probe \
//!   --no-default-features --features panic-screen-probe
//! ESPFLASH_PORT=/dev/cu.usbmodem21101 espflash flash --chip esp32s3 \
//!   --partition-table targets/medinote-waveshare/partitions.csv \
//!   --target-app-partition factory \
//!   targets/medinote-waveshare/target/xtensa-esp32s3-none-elf/release/panic-screen-probe
//! ```

#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::delay::Delay;
use esp_hal::gpio::{Level, Output, OutputConfig, Pull};
use esp_hal::peripherals::Peripherals;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::time::Rate;

use waveshare_rlcd42::panel;

/// See `main.rs`'s identical constant: esp-println's jtag-serial backend
/// drops output written before the USB CDC host finishes enumerating.
const CONSOLE_SETTLE_MS: u32 = 800;

/// How long the pre-panic stripes pattern holds before the deliberate panic,
/// so there is time to look at (or photograph) stage one before stage two
/// overwrites it.
const STRIPES_HOLD_MS: u32 = 3000;

// SCK=11, MOSI=12, DC=5, CS=40, RST=41 -- identical to main.rs. Both stages
// of this probe use the same physical pins; only the peripheral handles
// differ (the second set is `unsafe`-stolen, not the same Rust-level
// ownership as the first).
const PIN_SCK: u8 = 11;
const PIN_MOSI: u8 = 12;
const PIN_DC: u8 = 5;
const PIN_CS: u8 = 40;
const PIN_RST: u8 = 41;

fn build_panel<'d>(
    peripherals_spi2: esp_hal::peripherals::SPI2<'d>,
    sck: esp_hal::peripherals::GPIO11<'d>,
    mosi: esp_hal::peripherals::GPIO12<'d>,
    dc: esp_hal::peripherals::GPIO5<'d>,
    cs: esp_hal::peripherals::GPIO40<'d>,
    rst: esp_hal::peripherals::GPIO41<'d>,
    framebuffer: &'d mut [u8; panel::FRAMEBUFFER_BYTES],
) -> panel::St7305<'d> {
    let spi = Spi::new(
        peripherals_spi2,
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

fn draw_stripes(display: &mut panel::St7305) {
    for y in 0..panel::HEIGHT {
        let on = (y / 8) % 2 == 0;
        for x in 0..panel::WIDTH {
            display.set_pixel(x, y, on);
        }
    }
}

fn draw_checkerboard(display: &mut panel::St7305) {
    for y in 0..panel::HEIGHT {
        for x in 0..panel::WIDTH {
            let on = ((x / 10) + (y / 10)) % 2 == 0;
            display.set_pixel(x, y, on);
        }
    }
}

/// esp-backtrace's `custom-pre-backtrace` hook: called after a panic is
/// caught, before the banner/backtrace print and before the final `loop
/// {}`. This is the one place this probe exists to test.
///
/// Takes `&PanicInfo` because `vendor/esp-backtrace-0.20.0-custom-pre-backtrace-info`
/// is patched to pass it through (see that crate's MEDITAMER_PATCH.md) --
/// this probe doesn't need the message itself, just needs a signature that
/// matches the extern declaration.
#[no_mangle]
pub extern "Rust" fn custom_pre_backtrace(_info: &core::panic::PanicInfo) {
    console::println!("PANIC_SCREEN_PROBE stage=steal");

    // SAFETY: this is the entire point of the probe -- re-acquire handles to
    // hardware the normal `main()` flow already owns, from inside a panic
    // handler, and see what happens. Nothing else runs concurrently with
    // this: no executor was ever started, so there is no other code that
    // could be mid-transaction on SPI2 or the GPIO pins right now.
    let peripherals = unsafe { Peripherals::steal() };
    console::println!("PANIC_SCREEN_PROBE stage=stolen");

    static mut PANIC_FRAMEBUFFER: [u8; panel::FRAMEBUFFER_BYTES] = [0; panel::FRAMEBUFFER_BYTES];
    // SAFETY: written and read only here, once, on the single core that
    // reaches this handler.
    let framebuffer = unsafe { &mut *core::ptr::addr_of_mut!(PANIC_FRAMEBUFFER) };

    let mut display = build_panel(
        peripherals.SPI2,
        peripherals.GPIO11,
        peripherals.GPIO12,
        peripherals.GPIO5,
        peripherals.GPIO40,
        peripherals.GPIO41,
        framebuffer,
    );
    console::println!("PANIC_SCREEN_PROBE stage=constructed");

    display.init();
    console::println!("PANIC_SCREEN_PROBE stage=init_done");

    display.set_frame_rates(panel::HighPowerRate::Full, panel::LowPowerRate::Hz0_25);
    console::println!("PANIC_SCREEN_PROBE stage=frame_rates_set");

    draw_checkerboard(&mut display);
    console::println!("PANIC_SCREEN_PROBE stage=pattern_drawn");

    display.flush();
    console::println!("PANIC_SCREEN_PROBE stage=flush_done result=success");
}

#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    Delay::new().delay_millis(CONSOLE_SETTLE_MS);
    console::println!("PANIC_SCREEN_PROBE board=waveshare-rlcd42 chip=esp32s3");
    console::println!(
        "PANIC_SCREEN_PROBE pins sck={PIN_SCK} mosi={PIN_MOSI} dc={PIN_DC} cs={PIN_CS} rst={PIN_RST}"
    );

    static mut BOOT_FRAMEBUFFER: [u8; panel::FRAMEBUFFER_BYTES] = [0; panel::FRAMEBUFFER_BYTES];
    // SAFETY: single-threaded, no executor started; only `main` touches this.
    let framebuffer = unsafe { &mut *core::ptr::addr_of_mut!(BOOT_FRAMEBUFFER) };

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
    console::println!("PANIC_SCREEN_PROBE stage=boot_init_done");

    draw_stripes(&mut display);
    display.flush();
    console::println!("PANIC_SCREEN_PROBE stage=boot_pattern_shown, holding {STRIPES_HOLD_MS}ms");

    Delay::new().delay_millis(STRIPES_HOLD_MS);

    console::println!("PANIC_SCREEN_PROBE stage=panicking_now");
    panic!("panic-screen-probe: deliberate test panic, watch the panel");
}
