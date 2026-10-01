//! Bounded S3 Wi-Fi lifecycle probe.
//!
//! This binary composes the shared network owner without the Medinote UI, SD
//! service, or BLE roles. Credentials are supplied at build time through the
//! diagnostic-only `MEDITAMER_WIFI_SSID` and `MEDITAMER_WIFI_PASSWORD`
//! variables; their values are never printed. Production firmware does not
//! use this fallback. The HTTP listener is deliberately disabled so this
//! probe measures radio, DHCP, and owner state only.
#![no_std]
#![no_main]

#[path = "../network_memory.rs"]
mod network_memory;
use core::future::Future;
use core::sync::atomic::{AtomicBool, Ordering};
static ENABLED: AtomicBool = AtomicBool::new(true);

use embassy_net::Stack;
use embassy_time::{Duration, Timer};
use esp_backtrace as _;
use esp_bootloader_esp_idf as _;
use esp_hal::{clock::CpuClock, timer::timg::TimerGroup};
use netstack::host::{AllocatorMemorySnapshot, InternalBlockProbe, NetHost, ProductState};

const CONSOLE_SETTLE_MS: u32 = 800;
const INTERNAL_HEAP_BYTES: usize = 65_536;
const SAMPLE_COUNT: u32 = 60;

fn diagnostic_credentials() -> Option<netstack::config::WifiCredentials> {
    let ssid = option_env!("MEDITAMER_WIFI_SSID")?;
    let password = option_env!("MEDITAMER_WIFI_PASSWORD").unwrap_or("");
    if ssid.is_empty()
        || ssid.len() > netstack::config::WIFI_SSID_MAX
        || password.len() > netstack::config::WIFI_PASSWORD_MAX
    {
        return None;
    }
    let mut credentials = netstack::config::WifiCredentials {
        ssid: [0; netstack::config::WIFI_SSID_MAX],
        ssid_len: ssid.len() as u8,
        password: [0; netstack::config::WIFI_PASSWORD_MAX],
        password_len: password.len() as u8,
    };
    credentials.ssid[..ssid.len()].copy_from_slice(ssid.as_bytes());
    credentials.password[..password.len()].copy_from_slice(password.as_bytes());
    Some(credentials)
}

#[derive(Clone, Copy, Default)]
struct WifiProbeHost;

impl NetHost for WifiProbeHost {
    fn serve(&self, stack: Stack<'_>) -> impl Future<Output = ()> {
        run_diagnostic_service(stack)
    }

    async fn abort_service_work(&self) -> bool {
        true
    }
}

async fn run_diagnostic_service(_stack: Stack<'_>) {
    // Keeping this future alive is intentional: the owner treats a returned
    // service as an ownership fault and would restart the whole epoch.
    loop {
        Timer::after(Duration::from_secs(60)).await;
    }
}

fn allocator_memory_snapshot() -> AllocatorMemorySnapshot {
    network_memory::snapshot()
}
fn probe_internal_block_above_reserve(reserve_bytes: usize) -> InternalBlockProbe {
    network_memory::probe(reserve_bytes)
}

static PRODUCT_STATE: ProductState = ProductState {
    active_http_connections: || 0,
    active_sd_roundtrips: || 0,
    upload_session_active: || false,
    transport_quiet: || false,
    upload_enabled: || ENABLED.load(Ordering::Relaxed),
    allocator_memory_snapshot,
    probe_internal_block_above_reserve,
};

#[embassy_executor::task]
async fn network_owner_task(
    wifi: esp_hal::peripherals::WIFI<'static>,
    resources: &'static mut embassy_net::StackResources<{ netstack::NET_STACK_SOCKETS }>,
) {
    netstack::host::install(&PRODUCT_STATE);
    netstack::run_network_owner(&WifiProbeHost, wifi, resources).await;
}

#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    esp_hal::delay::Delay::new().delay_millis(CONSOLE_SETTLE_MS);
    console::println!(
        "WIFI_PROBE board=waveshare-rlcd42 chip=esp32s3 state=boot heap_bytes={}",
        INTERNAL_HEAP_BYTES
    );

    network_memory::init_heap();
    let bytes = network_memory::init_bytes(peripherals.PSRAM, 131_072);
    for (i, byte) in bytes.iter_mut().enumerate() {
        *byte = (i as u8).wrapping_mul(37);
    }
    assert!(bytes
        .iter()
        .enumerate()
        .all(|(i, b)| *b == (i as u8).wrapping_mul(37)));
    console::println!("WIFI_PROBE psram_check=ok bytes={}", bytes.len());
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);
    netstack::wifi::remember_runtime_config(netstack::config::NetConfigSet {
        credentials: diagnostic_credentials(),
        policy: netstack::config::WifiRuntimePolicy::defaults(),
    });
    netstack::host::set_listener_enabled(false);
    console::println!("WIFI_PROBE state=owner_start listener_enabled=false");

    static EXECUTOR: static_cell::StaticCell<esp_rtos::embassy::Executor> =
        static_cell::StaticCell::new();
    let executor = EXECUTOR.init(esp_rtos::embassy::Executor::new());
    let resources = netstack::stack_resources();
    executor.run(|spawner| {
        spawner.spawn(
            network_owner_task(peripherals.WIFI, resources)
                .expect("wifi owner task pool exhausted"),
        );
        spawner.spawn(sample_task().expect("sample task pool exhausted"));
    });
}

#[embassy_executor::task]
async fn sample_task() -> ! {
    for sample in 1..=SAMPLE_COUNT {
        Timer::after(Duration::from_secs(1)).await;
        if sample == 25 || sample == 30 {
            ENABLED.store(sample == 30, Ordering::Relaxed);
            console::println!(
                "WIFI_PROBE control=enabled sample={} value={}",
                sample,
                sample == 30
            );
        }
        let status = netstack::wifi::net_status_snapshot();
        let heap = allocator_memory_snapshot();
        console::println!(
            "WIFI_PROBE sample={} state={} link={} ipv4={}.{}.{}.{} listener={} failure={} code={} attempt={} heap_free={} heap_used={} heap_peak={}",
            sample,
            status.state,
            status.link,
            status.ipv4[0],
            status.ipv4[1],
            status.ipv4[2],
            status.ipv4[3],
            status.listener,
            status.failure_class,
            status.failure_code,
            status.attempt,
            heap.free_internal_bytes,
            heap.used_bytes,
            heap.peak_used_bytes,
        );
    }
    let status = netstack::wifi::net_status_snapshot();
    console::println!(
        "WIFI_PROBE state=done result={} samples={} link={} ipv4={}.{}.{}.{} attempts={} successes={}",
        if status.link && status.ipv4 != [0; 4] && netstack::telemetry::snapshot().wifi_connect_successes >= 2 { "ok" } else { "error" },
        SAMPLE_COUNT,
        status.link,
        status.ipv4[0],
        status.ipv4[1],
        status.ipv4[2],
        status.ipv4[3],
        status.attempt,
        netstack::telemetry::snapshot().wifi_connect_successes,
    );
    loop {
        Timer::after(Duration::from_secs(60)).await;
    }
}
