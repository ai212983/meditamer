//! Bring-up for the Waveshare ESP32-S3-RLCD-4.2, and the first evidence that
//! ADR-0015's platform crates are not tied to the chip they grew up on.
//!
//! Deliberately not a product. It boots the RTOS, drives the platform layer
//! through a few real operations, and reports over the S3's native
//! USB-Serial-JTAG. What it proves is portability: `console`, `shell`, and
//! `arbitration` compile and run on Xtensa LX7 with no source changes, having
//! only ever run on the LX6 Inkplate.
//!
//! Medinote's UI will grow from here; the panel driver does not exist yet.

#![no_std]
#![no_main]

mod panel;

use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::gpio::{Level, Output, OutputConfig};
use esp_hal::interrupt::software::SoftwareInterruptControl;
use esp_hal::spi::master::{Config as SpiConfig, Spi};
use esp_hal::spi::Mode as SpiMode;
use esp_hal::time::Rate;
use esp_hal::timer::timg::TimerGroup;
use board::{Panel, RefreshMode};
use static_cell::StaticCell;

use arbitration::claim::{self, Ownership};
use shell::registry::SurfaceRegistry;
use shell::types::{
    ProviderId, RefreshHint, SurfaceCapabilities, SurfaceRef, SurfaceRole, SurfaceSpec,
};

/// Matches Meditamer's shell sizing closely enough to exercise the same code
/// paths; Medinote will pick its own once it has screens.
const PROVIDER_CAPACITY: usize = 4;
const SURFACE_CAPACITY: usize = 8;

#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));

    // esp-println's jtag-serial backend drops output when the host has not
    // finished attaching; give the CDC port time to enumerate after the reset
    // that got us here.
    esp_hal::delay::Delay::new().delay_millis(800);

    console::println!("BOARD_BOOT board=waveshare-rlcd42 chip=esp32s3");
    console::println!("CPU_CLOCK hz={}", esp_hal::clock::cpu_clock().as_hz());

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let software_interrupts = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, software_interrupts.software_interrupt0);
    console::println!("RTOS_STARTED core=0");

    exercise_arbitration();
    exercise_shell();
    console::println!("PLATFORM_OK crates=console,shell,arbitration chip=esp32s3");

    // SCK=11, MOSI=12, DC=5, CS=40, RST=41 per Waveshare's user_config.h.
    let spi = Spi::new(
        peripherals.SPI2,
        SpiConfig::default()
            .with_frequency(Rate::from_mhz(24))
            .with_mode(SpiMode::_0),
    )
    .expect("spi2")
    .with_sck(peripherals.GPIO11)
    .with_mosi(peripherals.GPIO12);
    let output = OutputConfig::default();
    static FRAMEBUFFER: StaticCell<[u8; panel::FRAMEBUFFER_BYTES]> = StaticCell::new();
    let framebuffer = FRAMEBUFFER.init([0u8; panel::FRAMEBUFFER_BYTES]);
    let mut display = panel::St7305::new(
        framebuffer,
        spi,
        Output::new(peripherals.GPIO5, Level::Low, output),
        Output::new(peripherals.GPIO40, Level::High, output),
        Output::new(peripherals.GPIO41, Level::High, output),
    );
    display.init();
    console::println!("PANEL_INIT controller=st7305 {}x{}", panel::WIDTH, panel::HEIGHT);

    draw_border(display.framebuffer_mut());

    let panel: &mut dyn Panel = &mut display;

    // Draw a large blocky "F" through `blit_l8`, the trait's real entry point.
    // An F has no symmetry in either axis, so a mirror, a flip and a transpose
    // are each obvious at a glance -- which a border and a diagonal are not.
    //
    //   x=60  100        240
    //   +-----+-----------+   y=60
    //   |#####|###########|
    //   |#####+-----------+   y=100
    //   |#####|
    //   |#####+--------+      y=180
    //   |#####|########|
    //   |#####+--------+      y=220   (mid bar ends at x=200)
    //   |#####|
    //   +-----+               y=340
    //
    // Uniform ink, so one buffer serves every rectangle.
    static INK: StaticCell<[u8; 40 * 280]> = StaticCell::new();
    let ink = INK.init([0x00u8; 40 * 280]);

    let strokes = [
        (60, 40, 100, 260),  // stem
        (60, 40, 280, 80),   // top bar
        (60, 130, 220, 170), // middle bar
    ];
    let mut all_ok = true;
    for (x1, y1, x2, y2) in strokes {
        let ok = unsafe {
            panel.blit_l8(
                board::DirtyArea {
                    x1,
                    y1,
                    x2: x2 - 1,
                    y2: y2 - 1,
                },
                ink.as_ptr(),
            )
        };
        all_ok &= ok;
    }

    // A rectangle straddling the right edge must clip rather than overrun. If
    // it renders on the left instead, the X axis is mirrored.
    let clipped = unsafe {
        panel.blit_l8(
            board::DirtyArea {
                x1: panel::WIDTH as i32 - 10,
                y1: 270,
                x2: panel::WIDTH as i32 + 29,
                y2: 289,
            },
            ink.as_ptr(),
        )
    };

    let geometry = panel.geometry();
    let partial = panel.supports(RefreshMode::Partial);
    let refreshed = panel.refresh(RefreshMode::Full).is_ok();
    let rejected = panel.refresh(RefreshMode::Partial);
    console::println!(
        "PANEL_TRAIT {}x{} glyph=F strokes_ok={} clip_ok={} full_ok={} partial_supported={} rejected={:?}",
        geometry.width,
        geometry.height,
        all_ok,
        clipped,
        refreshed,
        partial,
        rejected
    );
    assert!(all_ok && clipped && refreshed && !partial);
    assert!(matches!(rejected, Err(board::RefreshError::Unsupported(_))));
    loop {
        esp_hal::delay::Delay::new().delay_millis(1000);
    }
}

/// A one-pixel frame, so the edges are visible without competing with the
/// glyph the blit draws.
fn draw_border(framebuffer: &mut [u8; panel::FRAMEBUFFER_BYTES]) {
    for x in 0..panel::WIDTH {
        panel::set_pixel(framebuffer, x, 0, true);
        panel::set_pixel(framebuffer, x, panel::HEIGHT - 1, true);
    }
    for y in 0..panel::HEIGHT {
        panel::set_pixel(framebuffer, 0, y, true);
        panel::set_pixel(framebuffer, panel::WIDTH - 1, y, true);
    }
}

/// Drive the claim registry through the transitions the Inkplate performs, and
/// check the arbiter answers as the model says it should.
fn exercise_arbitration() {
    // Nothing published yet: ownership must read Unknown, never "free".
    let initial = claim::ble_ownership();

    claim::set_ble_ownership(Ownership::Active);
    let active = claim::ble_ownership();

    claim::publish_exclusive_lease(0xABCD, 7);
    let lease_ok = claim::exclusive_lease_matches(0xABCD, 7);
    let lease_wrong_epoch = claim::exclusive_lease_matches(0xABCD, 8);

    // Exclusive ownership must be refused while the supervisor is resident.
    claim::set_residency(true, true);
    claim::set_wifi_link(true);
    claim::set_service_listening(true);
    let confirmed_while_busy = claim::exclusive_ownership_confirmed(0xABCD, 7);

    // ... and granted once everything is down.
    claim::set_residency(false, false);
    claim::set_wifi_link(false);
    claim::set_service_listening(false);
    let confirmed_when_idle = claim::exclusive_ownership_confirmed(0xABCD, 7);

    console::println!(
        "ARBITRATION initial={:?} active={:?} lease_ok={} lease_wrong_epoch={} busy={} idle={}",
        initial,
        active,
        lease_ok,
        lease_wrong_epoch,
        confirmed_while_busy,
        confirmed_when_idle
    );
    assert!(matches!(initial, Ownership::Unknown));
    assert!(matches!(active, Ownership::Active));
    assert!(lease_ok && !lease_wrong_epoch);
    assert!(!confirmed_while_busy && confirmed_when_idle);
}

/// Register a provider and resolve a surface through the same registry the
/// Inkplate's launcher uses.
fn exercise_shell() {
    let mut registry: SurfaceRegistry<PROVIDER_CAPACITY, SURFACE_CAPACITY> = SurfaceRegistry::new();

    let token = registry
        .register_provider(
            ProviderId(1),
            &[SurfaceSpec::new(
                1,
                SurfaceRole::AppRoot,
                SurfaceCapabilities::LAUNCHABLE,
                RefreshHint::Content,
            )],
        )
        .expect("provider registration");

    let resolved = registry
        .resolve(SurfaceRef {
            owner: token,
            id: shell::types::SurfaceId(1),
        })
        .is_ok();
    console::println!(
        "SHELL provider_registered=true surface_resolved={} capacity={}x{}",
        resolved,
        PROVIDER_CAPACITY,
        SURFACE_CAPACITY
    );
    assert!(resolved);
}
