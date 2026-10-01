//! Shared BLE runtime start/stop probe (`docs/plans/ble/central-input.md`). Proves
//! `platform/connectivity/ble`'s thin controller runner builds and survives one
//! start/stop cycle on this chip -- no scanning, GATT, HID, panel, sensor,
//! or product task. See `[[bin]] name = "ble-shared-runtime-probe"` in
//! Cargo.toml for why it needs `--features shared-ble-runtime`.
//!
//! This target has no allocator otherwise (shared BLE runtime and roles
//! ledger, B-003): brings up a bare internal-SRAM heap sized only for BLE
//! controller bring-up (`INTERNAL_HEAP_BYTES`), not a product-wide budget.
//! `INTERNAL_HEAP_BYTES` is a first-cut Phase 1 constant -- Phase 2 measures
//! and records the real linked/high-water numbers this target needs
//! (`platform/connectivity/ble`'s module doc; the plan's Phase 2 exit criteria).
#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::timer::timg::TimerGroup;

/// See `main.rs`'s identical constant for why the console needs settling
/// time: esp-println's jtag-serial backend drops output written before the
/// USB CDC host finishes enumerating.
const CONSOLE_SETTLE_MS: u32 = 800;

/// First-cut internal heap for BLE controller bring-up only -- see this
/// file's module doc.
const INTERNAL_HEAP_BYTES: usize = 65536;

#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    esp_hal::delay::Delay::new().delay_millis(CONSOLE_SETTLE_MS);
    console::println!("BLE_SHARED_RUNTIME_PROBE board=waveshare-rlcd42 chip=esp32s3 state=boot");

    esp_alloc::heap_allocator!(size: INTERNAL_HEAP_BYTES);
    console::println!(
        "BLE_SHARED_RUNTIME_PROBE state=allocator_ready heap_bytes={}",
        INTERNAL_HEAP_BYTES
    );

    let timg0 = TimerGroup::new(peripherals.TIMG0);
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
