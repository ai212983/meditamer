//! Real BLE central connection proof (`docs/plans/ble/central-input.md`). Runs
//! `platform/connectivity/ble`'s `ble::run_live_connect` against a user-designated
//! connectable device: name scan, connect, optional pairing (ADR-0016:
//! "Add pairing and bonding to the shared BLE central role"), GATT client
//! MTU exchange, service/characteristic discovery, one read, then
//! disconnect. See `[[bin]] name = "ble-connect-probe"` in Cargo.toml for
//! why it needs `--features shared-ble-runtime`.
//!
//! The bond table this run passes is in-memory only, created empty and
//! discarded on exit -- ADR-0016's non-volatile persistence port for this
//! target does not exist yet, so a bond made here does not survive a
//! reboot. A pairing this probe completes still proves ADR-0016's pairing
//! phase itself works; it does not yet prove reconnect-without-repairing.
#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::timer::timg::TimerGroup;

/// See `main.rs`'s identical constant for why the console needs settling
/// time: esp-println's jtag-serial backend drops output written before the
/// USB CDC host finishes enumerating.
const CONSOLE_SETTLE_MS: u32 = 800;

const INTERNAL_HEAP_BYTES: usize = 65536;
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

/// Experiment switch: request pairing immediately after connecting instead
/// of waiting for an ATT security error. Defaults off.
const PAIR_BEFORE_DISCOVERY: bool = env_flag_is_one(option_env!("PAIR_BEFORE_DISCOVERY"));

/// Enable a bounded raw HID-notification capture after discovery. Kept off
/// by default so the generic connection probe retains its prior timing.
const CAPTURE_HID_REPORTS: bool = env_flag_is_one(option_env!("CAPTURE_HID_REPORTS"));

struct ProbeHidSink;

impl ble::HidInputSink for ProbeHidSink {
    fn input_ready(&self, now_us: u64) {
        console::println!("BLE_CONNECT_PROBE_HID_STREAM state=ready at_us={}", now_us);
    }

    fn report(&self, report_id: u8, now_us: u64, bytes: &[u8]) {
        console::println!(
            "BLE_CONNECT_PROBE_HID_STREAM report_id={} at_us={} bytes={:02x?}",
            report_id,
            now_us,
            bytes,
        );
        if report_id == 3 && bytes.len() == 7 {
            let x_raw = u16::from(bytes[2]) | (u16::from(bytes[3] & 0x0f) << 8);
            let y_raw = u16::from(bytes[3] >> 4) | (u16::from(bytes[4]) << 4);
            let x = ((x_raw as i16) << 4) >> 4;
            let y = ((y_raw as i16) << 4) >> 4;
            console::println!(
                "BLE_CONNECT_PROBE_MOUSE at_us={} buttons={:#05b} wheel={} x={} y={} pan={} zoom={}",
                now_us,
                bytes[0] & 0x07,
                bytes[1] as i8,
                x,
                y,
                bytes[5] as i8,
                bytes[6] as i8,
            );
        }
    }

    fn input_closed(&self, now_us: u64) {
        console::println!("BLE_CONNECT_PROBE_HID_STREAM state=closed at_us={}", now_us);
    }
}

static PROBE_HID_SINK: ProbeHidSink = ProbeHidSink;

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

const fn env_flag_is_one(value: Option<&str>) -> bool {
    match value {
        Some(text) => {
            let bytes = text.as_bytes();
            bytes.len() == 1 && bytes[0] == b'1'
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
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    esp_hal::delay::Delay::new().delay_millis(CONSOLE_SETTLE_MS);
    console::println!(
        "BLE_CONNECT_PROBE board=waveshare-rlcd42 chip=esp32s3 state=boot target={}",
        TARGET_DEVICE_NAME
    );

    esp_alloc::heap_allocator!(size: INTERNAL_HEAP_BYTES);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
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
        "BLE_CONNECT_PROBE state=pairing_config request_pairing={} pair_before_discovery={} bond_table=in_memory_only",
        REQUEST_PAIRING,
        PAIR_BEFORE_DISCOVERY,
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
            pair_before_discovery: PAIR_BEFORE_DISCOVERY,
            hid_capture_duration: None,
            hid_input_sink: CAPTURE_HID_REPORTS.then_some(&PROBE_HID_SINK),
            stop_requested: None,
        },
        &mut bond_table,
        report,
    )
    .await;

    let address = report.address.unwrap_or([0; 6]);
    console::println!(
        "BLE_CONNECT_PROBE state=done outcome={:?} address={:02x}{:02x}{:02x}{:02x}{:02x}{:02x} connection_state={:?} security_level={:?} bonded={} bond_table_full={} pairing_elapsed_us={:?} pairing_disconnect_reason={:?} services={} characteristics={} discovery_diagnostic={:?}",
        report.outcome,
        address[0], address[1], address[2], address[3], address[4], address[5],
        report.connection_state,
        report.security_level,
        report.bonded,
        report.bond_table_full,
        report.pairing_elapsed_us,
        report.pairing_disconnect_reason,
        report.services.len(),
        report.characteristics.len(),
        report.service_discovery_diagnostic,
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
        "BLE_CONNECT_PROBE_HID report_map_len={} report_map={:02x?} reports={}",
        report.hid_report_map.len(),
        report.hid_report_map.as_slice(),
        report.hid_reports.len(),
    );
    match ble::hid::HidDescriptor::parse(report.hid_report_map.as_slice()) {
        Ok(descriptor) => {
            for layout in descriptor.layouts() {
                console::println!("BLE_CONNECT_PROBE_HID_LAYOUT {:?}", layout);
            }
        }
        Err(error) => console::println!("BLE_CONNECT_PROBE_HID_PARSE error={:?}", error),
    }
    for report_ref in report.hid_reports.iter() {
        console::println!("BLE_CONNECT_PROBE_HID_REPORT {:?}", report_ref);
    }
    console::println!(
        "BLE_CONNECT_PROBE_HID_CAPTURE subscriptions={} notifications={}",
        report.hid_subscriptions,
        report.hid_notifications.len(),
    );
    for notification in report.hid_notifications.iter() {
        console::println!("BLE_CONNECT_PROBE_HID_NOTIFICATION {:?}", notification);
    }
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
