//! Fake GATT client for ADR-0011's diagnostic peripheral (shared BLE
//! runtime and roles plan, Phase 4 increment 6: "Test the server with a
//! fake client, then a phone or computer"). Runs `platform/connectivity/ble`'s
//! `ble::run_diagnostic_client` against a Meditamer diagnostic peripheral:
//! name scan, connect, locate the Diagnostic Service by UUID, read Build
//! Info, write-then-read-back Echo, subscribe to and await one Lifecycle
//! Status notification, then disconnect. Distinct from `ble-connect-probe`
//! (Phase 4's generic central-connection proof, against any connectable
//! device) -- this one targets the specific service/characteristics
//! `products/meditamer/src/firmware/ble/mod.rs` builds. See `[[bin]] name =
//! "ble-diagnostic-client-probe"` in Cargo.toml for why it needs
//! `--features shared-ble-runtime`.
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
// Generous for manual bench validation (increment 6 device evidence): gives
// plenty of slack against serial-tooling round-trip time on the Inkplate
// side coordinating its own window start, rather than needing split-second
// timing between the two boards. ADR-0011's own 60s/120s deadlines are what
// actually bound the Inkplate side; this is just this fake client's own
// patience.
const SCAN_TIMEOUT_SECONDS: u64 = 100;
const CONNECT_TIMEOUT_SECONDS: u64 = 100;

/// Substring of the target device's advertised local name. Defaults to the
/// Diagnostic Service's own scan-response name; override at build time with
/// `TARGET_DEVICE_NAME=<substring> <build command>`.
const TARGET_DEVICE_NAME: &str = match option_env!("TARGET_DEVICE_NAME") {
    Some(value) => value,
    None => "Meditamer",
};

fn now_micros() -> u64 {
    esp_hal::time::Instant::now()
        .duration_since_epoch()
        .as_micros()
}

#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    esp_hal::delay::Delay::new().delay_millis(CONSOLE_SETTLE_MS);
    console::println!(
        "BLE_DIAGNOSTIC_CLIENT_PROBE board=waveshare-rlcd42 chip=esp32s3 state=boot target={}",
        TARGET_DEVICE_NAME
    );

    esp_alloc::heap_allocator!(size: INTERNAL_HEAP_BYTES);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);
    console::println!("BLE_DIAGNOSTIC_CLIENT_PROBE state=rtos_started");

    embassy_executor_run(peripherals.BT);
}

fn embassy_executor_run(device: esp_hal::peripherals::BT<'static>) -> ! {
    static EXECUTOR: static_cell::StaticCell<esp_rtos::embassy::Executor> =
        static_cell::StaticCell::new();
    let executor = EXECUTOR.init(esp_rtos::embassy::Executor::new());
    executor.run(|spawner| {
        spawner.spawn(diagnostic_client_task(device).unwrap());
    })
}

#[embassy_executor::task]
async fn diagnostic_client_task(device: esp_hal::peripherals::BT<'static>) {
    let report = ble::run_diagnostic_client(
        device,
        TARGET_DEVICE_NAME,
        embassy_time::Duration::from_secs(SCAN_TIMEOUT_SECONDS),
        embassy_time::Duration::from_secs(CONNECT_TIMEOUT_SECONDS),
        now_micros,
    )
    .await;

    let address = report.address.unwrap_or([0; 6]);
    console::println!(
        "BLE_DIAGNOSTIC_CLIENT_PROBE state=done outcome={:?} address={:02x}{:02x}{:02x}{:02x}{:02x}{:02x} echo_round_trip_ok={:?}",
        report.outcome,
        address[0], address[1], address[2], address[3], address[4], address[5],
        report.echo_round_trip_ok,
    );
    if let Some(build_info) = report.build_info {
        console::println!(
            "BLE_DIAGNOSTIC_CLIENT_PROBE_BUILD_INFO schema_version={} protocol_version={} capabilities={:#04x} digest_prefix={:02x?}",
            build_info.schema_version,
            build_info.protocol_version,
            build_info.capabilities.0,
            build_info.digest_prefix,
        );
    }
    if let Some(status) = report.lifecycle_status {
        console::println!(
            "BLE_DIAGNOSTIC_CLIENT_PROBE_LIFECYCLE_STATUS state={:?} remaining_seconds={} rx_drops={} tx_timeouts={}",
            status.state,
            status.remaining_seconds,
            status.rx_drops,
            status.tx_timeouts,
        );
    }

    loop {
        embassy_time::Timer::after_secs(1).await;
    }
}
