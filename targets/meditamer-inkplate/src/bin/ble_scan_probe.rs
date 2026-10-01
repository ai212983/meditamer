//! Real BLE scanning proof (`docs/plans/ble/central-input.md`). Runs
//! `platform/connectivity/ble`'s [`ble::run_live_scan`] against real advertising
//! traffic: two sequential fixed windows with a restart between them, a
//! bounded result set, and one chosen device recognized by address if
//! `SCAN_TARGET_ADDRESS` is set. No GATT, HID, or the existing
//! `ble-foundation` diagnostic peripheral -- see
//! `[[bin]] name = "ble-scan-probe"` in Cargo.toml for why it needs
//! `--no-default-features --features shared-ble-runtime`.
//!
//! Reuses the product's existing PSRAM-backed allocator, same reasoning as
//! `ble_shared_runtime_probe.rs`'s identical call.
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

const SCAN_WINDOW_COUNT: u32 = 2;
const SCAN_WINDOW_SECONDS: u64 = 10;

/// Address of one chosen advertising device to recognize, hex-encoded
/// little-endian-as-printed (e.g. `AABBCCDDEEFF`), or empty to skip
/// recognition and just report the bounded result set. Set at build time:
/// `SCAN_TARGET_ADDRESS=AABBCCDDEEFF <build command>`.
const SCAN_TARGET_ADDRESS: Option<&str> = option_env!("SCAN_TARGET_ADDRESS");

fn now_micros() -> u64 {
    esp_hal::time::Instant::now()
        .duration_since_epoch()
        .as_micros()
}

fn parse_target_address() -> Option<[u8; 6]> {
    let text = SCAN_TARGET_ADDRESS?;
    if text.len() != 12 {
        return None;
    }
    let mut address = [0u8; 6];
    for (index, byte) in address.iter_mut().enumerate() {
        let hi = (text.as_bytes()[index * 2] as char).to_digit(16)?;
        let lo = (text.as_bytes()[index * 2 + 1] as char).to_digit(16)?;
        *byte = ((hi << 4) | lo) as u8;
    }
    Some(address)
}

#[esp_hal::main]
fn main() -> ! {
    let hal_config = esp_hal::Config::default().with_cpu_clock(CpuClock::_240MHz);
    let peripherals = esp_hal::init(hal_config);
    console::println!("BLE_SCAN_PROBE board=inkplate-tempera chip=esp32 state=boot");

    let allocator_status = psram::init_allocator(peripherals.PSRAM);
    if !matches!(allocator_status.state, psram::AllocatorState::Initialized) {
        console::println!(
            "BLE_SCAN_PROBE state=allocator_failed status={:?}",
            allocator_status
        );
        panic!("psram allocator initialization failed");
    }

    let timg0 = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);
    console::println!("BLE_SCAN_PROBE state=rtos_started");

    let target = parse_target_address();
    if let Some(address) = target {
        console::println!(
            "BLE_SCAN_PROBE state=target_configured address={:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            address[0],
            address[1],
            address[2],
            address[3],
            address[4],
            address[5],
        );
    }

    embassy_executor_run(peripherals.BT, target);
}

fn embassy_executor_run(device: esp_hal::peripherals::BT<'static>, target: Option<[u8; 6]>) -> ! {
    static EXECUTOR: static_cell::StaticCell<esp_rtos::embassy::Executor> =
        static_cell::StaticCell::new();
    let executor = EXECUTOR.init(esp_rtos::embassy::Executor::new());
    executor.run(|spawner| {
        spawner.spawn(scan_task(device, target).unwrap());
    })
}

#[embassy_executor::task]
async fn scan_task(device: esp_hal::peripherals::BT<'static>, target: Option<[u8; 6]>) {
    let report = ble::run_live_scan(
        device,
        SCAN_WINDOW_COUNT,
        embassy_time::Duration::from_secs(SCAN_WINDOW_SECONDS),
        now_micros,
    )
    .await;

    console::println!(
        "BLE_SCAN_PROBE state=done outcome={:?} windows={}",
        report.outcome,
        report.windows.len(),
    );
    for window in report.windows.iter() {
        console::println!(
            "BLE_SCAN_PROBE_WINDOW generation={} results={} overflow_count={}",
            window.generation,
            window.results.len(),
            window.overflow_count,
        );
        for result in window.results.iter() {
            console::println!(
                "BLE_SCAN_PROBE_RESULT address={:02x}{:02x}{:02x}{:02x}{:02x}{:02x} name={} rssi={} recognized={}",
                result.address[0], result.address[1], result.address[2],
                result.address[3], result.address[4], result.address[5],
                result.name.as_str(),
                result.rssi,
                target == Some(result.address),
            );
        }
    }

    loop {
        embassy_time::Timer::after_secs(1).await;
    }
}
