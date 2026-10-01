//! Real BLE central connection proof (`docs/plans/ble/central-input.md`). Runs
//! `platform/connectivity/ble`'s `ble::run_live_connect` against a user-designated
//! connectable device: name scan, connect, optional pairing (ADR-0016:
//! "Add pairing and bonding to the shared BLE central role"), GATT client
//! MTU exchange, service/characteristic discovery, one read, then
//! disconnect. No GATT, HID, or the existing `ble-foundation` diagnostic
//! peripheral -- see `[[bin]] name = "ble-connect-probe"` in Cargo.toml for
//! why it needs `--no-default-features --features shared-ble-runtime`.
//!
//! Reuses the product's existing PSRAM-backed allocator, same reasoning as
//! `ble_shared_runtime_probe.rs`'s identical call.
//!
//! The bond table this run passes is in-memory only, created empty and
//! discarded on exit -- ADR-0016's non-volatile persistence port for this
//! target does not exist yet, so a bond made here does not survive a
//! reboot. A pairing this probe completes still proves ADR-0016's pairing
//! phase itself works; it does not yet prove reconnect-without-repairing.
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

const SCAN_TIMEOUT_SECONDS: u64 = 15;
const CONNECT_TIMEOUT_SECONDS: u64 = 15;

/// Substring of the target device's advertised local name. Default matches
/// the user-designated "CheerTok" device; override at build time with
/// `TARGET_DEVICE_NAME=<substring> <build command>`.
const TARGET_DEVICE_NAME: &str = match option_env!("TARGET_DEVICE_NAME") {
    Some(value) => value,
    None => "CheerTok",
};

/// ADR-0016: request pairing/bonding after connecting, before service
/// discovery. Defaults on -- this probe's default target (CheerTok) is the
/// device R-004 suspects needs it. Override with `REQUEST_PAIRING=0
/// <build command>` to reproduce the pre-ADR-0016 connect-only behavior
/// (E-0006/E-0007).
const REQUEST_PAIRING: bool = !env_flag_is_zero(option_env!("REQUEST_PAIRING"));

/// `matches!` cannot compare `str` content in a `const` context (no
/// `const PartialEq` for `str` on this toolchain); byte-slice indexing is,
/// so this checks the same thing by hand.
const fn env_flag_is_zero(value: Option<&str>) -> bool {
    match value {
        Some(text) => {
            let bytes = text.as_bytes();
            bytes.len() == 1 && bytes[0] == b'0'
        }
        None => false,
    }
}

fn now_micros() -> u64 {
    esp_hal::time::Instant::now()
        .duration_since_epoch()
        .as_micros()
}

#[inline(never)]
fn report_storage() -> &'static mut ble::LiveConnectReport {
    // Outlining keeps the large initializer out of this task's async frame.
    static REPORT: static_cell::StaticCell<ble::LiveConnectReport> = static_cell::StaticCell::new();
    REPORT.init_with(ble::LiveConnectReport::new)
}

#[esp_hal::main]
fn main() -> ! {
    let hal_config = esp_hal::Config::default().with_cpu_clock(CpuClock::_240MHz);
    let peripherals = esp_hal::init(hal_config);
    console::println!(
        "BLE_CONNECT_PROBE board=inkplate-tempera chip=esp32 state=boot target={}",
        TARGET_DEVICE_NAME
    );

    let allocator_status = psram::init_allocator(peripherals.PSRAM);
    if !matches!(allocator_status.state, psram::AllocatorState::Initialized) {
        console::println!(
            "BLE_CONNECT_PROBE state=allocator_failed status={:?}",
            allocator_status
        );
        panic!("psram allocator initialization failed");
    }

    let timg0 = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);
    console::println!("BLE_CONNECT_PROBE state=rtos_started");

    embassy_executor_run(peripherals.BT);
}

fn embassy_executor_run(device: esp_hal::peripherals::BT<'static>) -> ! {
    static EXECUTOR: static_cell::StaticCell<esp_rtos::embassy::Executor> =
        static_cell::StaticCell::new();
    let executor = EXECUTOR.init(esp_rtos::embassy::Executor::new());
    executor.run(|spawner| {
        spawner.spawn(connect_task(device).unwrap());
    })
}

#[embassy_executor::task]
async fn connect_task(device: esp_hal::peripherals::BT<'static>) {
    console::println!(
        "BLE_CONNECT_PROBE state=pairing_config request_pairing={} bond_table=in_memory_only",
        REQUEST_PAIRING
    );
    let mut bond_table = ble::bond::BondTable::new();
    let report = report_storage();
    ble::run_live_connect(
        device,
        ble::LiveConnectOptions {
            target_name: TARGET_DEVICE_NAME,
            scan_timeout: embassy_time::Duration::from_secs(SCAN_TIMEOUT_SECONDS),
            connect_timeout: embassy_time::Duration::from_secs(CONNECT_TIMEOUT_SECONDS),
            now_ticks: now_micros,
            startup_complete: None,
            request_pairing: REQUEST_PAIRING,
            pair_before_discovery: false,
            hid_capture_duration: None,
            hid_input_sink: None,
            stop_requested: None,
        },
        &mut bond_table,
        report,
    )
    .await;

    let address = report.address.unwrap_or([0; 6]);
    console::println!(
        "BLE_CONNECT_PROBE state=done outcome={:?} address={:02x}{:02x}{:02x}{:02x}{:02x}{:02x} connection_state={:?} security_level={:?} bonded={} bond_table_full={} services={} characteristics={}",
        report.outcome,
        address[0],
        address[1],
        address[2],
        address[3],
        address[4],
        address[5],
        report.connection_state,
        report.security_level,
        report.bonded,
        report.bond_table_full,
        report.services.len(),
        report.characteristics.len(),
    );
    for service in report.services.iter() {
        console::println!(
            "BLE_CONNECT_PROBE_SERVICE uuid={:?} start={} end={}",
            service.uuid,
            service.start_handle,
            service.end_handle
        );
    }
    for characteristic in report.characteristics.iter() {
        console::println!(
            "BLE_CONNECT_PROBE_CHARACTERISTIC uuid={:?} handle={} readable={} writable={} notifiable={}",
            characteristic.uuid,
            characteristic.handle,
            characteristic.readable,
            characteristic.writable,
            characteristic.notifiable,
        );
    }
    console::println!(
        "BLE_CONNECT_PROBE_HID report_map_len={} reports={}",
        report.hid_report_map.len(),
        report.hid_reports.len(),
    );
    if let Some(handle) = report.read_handle {
        console::println!(
            "BLE_CONNECT_PROBE_READ handle={} len={} bytes={:02x?}",
            handle,
            report.read_bytes.len(),
            report.read_bytes.as_slice(),
        );
    }

    loop {
        embassy_time::Timer::after_secs(1).await;
    }
}
