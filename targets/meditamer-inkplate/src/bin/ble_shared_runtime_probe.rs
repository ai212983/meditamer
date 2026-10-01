//! Shared BLE runtime start/stop probe (`docs/plans/ble/central-input.md`). Proves
//! `platform/connectivity/ble`'s thin controller runner builds and survives one
//! start/stop cycle on this chip -- no scanning, GATT, HID, Wi-Fi, or the
//! existing `ble-foundation` diagnostic peripheral. See
//! `[[bin]] name = "ble-shared-runtime-probe"` in Cargo.toml for why it
//! needs `--no-default-features --features shared-ble-runtime`.
//!
//! Reuses the product's existing PSRAM-backed allocator
//! (`meditamer_product::firmware::psram`) rather than standing up a second
//! one: this binary still links the same allocator the reviewed BLE
//! controller source needs, and Meditamer's internal-heap sizing already
//! passed the diagnostic probe's resource floor (see that probe's module
//! doc). It does not reuse `inkplate_tempera::InkplateHal`, same reasoning
//! as `src/updater/mod.rs`'s identical call: that driver allocates the
//! e-ink panel's static framebuffer and waveform tables on construction,
//! which this size-minimal probe has no use for.
#![no_std]
#![no_main]

use esp_backtrace as _;

// Panic breadcrumb hook (see `src/panic_crumb.rs`): every binary links
// esp-backtrace separately, so each needs its own copy of the symbol.
#[path = "../panic_crumb.rs"]
mod panic_crumb;
use esp_hal::clock::CpuClock;
use meditamer_product::firmware::psram;

// See `main.rs`'s identical call for why each `[[bin]]` needs its own now.
esp_bootloader_esp_idf::esp_app_desc!();

#[esp_hal::main]
fn main() -> ! {
    let hal_config = esp_hal::Config::default().with_cpu_clock(CpuClock::_240MHz);
    let peripherals = esp_hal::init(hal_config);
    console::println!("BLE_SHARED_RUNTIME_PROBE board=inkplate-tempera chip=esp32 state=boot");

    let allocator_status = psram::init_allocator(peripherals.PSRAM);
    if !matches!(allocator_status.state, psram::AllocatorState::Initialized) {
        console::println!(
            "BLE_SHARED_RUNTIME_PROBE state=allocator_failed status={:?}",
            allocator_status
        );
        panic!("psram allocator initialization failed");
    }

    let timg0 = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);
    console::println!("BLE_SHARED_RUNTIME_PROBE state=rtos_started");

    let result = ble::start_stop_probe(peripherals.BT);
    console::println!(
        "BLE_SHARED_RUNTIME_PROBE state=done result={} start_us={} stop_us={}",
        result.outcome.label(),
        result.start_micros,
        result.stop_micros,
    );

    loop {
        esp_hal::delay::Delay::new().delay_millis(1000);
    }
}
