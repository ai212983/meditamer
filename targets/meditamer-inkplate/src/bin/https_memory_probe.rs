//! Isolated HTTPS PSRAM memory probe.
//!
//! Uses software MbedTLS with an explicit PSRAM heap, PSRAM TCP buffers and
//! a pinned PSRAM request future. Radio resources remain internal. Loads
//! stored Wi-Fi credentials before starting PSRAM/radio; accepts volatile UART
//! credentials if unprovisioned. Never writes flash.
//! The fixed endpoint is google.com:443, path /; redirects are not followed.
//!
//! Build with the current host UTC, for immediate diagnostic use only:
//! `HTTPS_PROBE_UTC=$(date +%s) FIRMWARE_BIN=https-memory-probe`
//! `CARGO_FEATURES=https-memory-probe CARGO_LOCKED=1`
//! `targets/meditamer-inkplate/build.sh release minimal`.
//! The clock is unsuitable for production or later reboots. TLS heap peak
//! includes header padding; stack headroom is sampled at allocator calls,
//! not a full watermark. This does not qualify concurrent production UI/BLE.
#![no_std]
#![no_main]

#[path = "https_memory_probe/provisioning.rs"]
mod provisioning;

use core::ffi::c_void;
use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use allocator_api2::alloc::Allocator;
use esp_backtrace as _;

// Panic breadcrumb hook (see `src/panic_crumb.rs`): every binary links
// esp-backtrace separately, so each needs its own copy of the symbol.
#[path = "../panic_crumb.rs"]
mod panic_crumb;
use esp_hal::clock::CpuClock;
use esp_hal::timer::timg::TimerGroup;
use mbedtls_rs::sys::hook::backend::embassy::timer::EmbassyTimer;
use mbedtls_rs::sys::hook::timer::hook_timer;
use mbedtls_rs::sys::hook::wall_clock::{hook_wall_clock, MbedtlsWallClock};
use mbedtls_rs::sys::{mbedtls_platform_set_calloc_free, tm};
use meditamer_product::firmware::psram;

esp_bootloader_esp_idf::esp_app_desc!();

/// Public Google Trust Services R1 and R4 roots from the Google PKI repository.
const GTS_ROOTS_PEM: &core::ffi::CStr = match core::ffi::CStr::from_bytes_with_nul(
    concat!(include_str!("gts_roots.pem"), "\0").as_bytes(),
) {
    Ok(roots) => roots,
    Err(_) => panic!("root PEM terminator"),
};

// ---------------------------------------------------------------------------
// MbedTLS PSRAM heap: explicit external allocation with header + counters.
// ---------------------------------------------------------------------------

const TLS_HEAP_ALIGN: usize = 16;
const TLS_HEAP_MAGIC: u32 = 0x544C_5348;

#[repr(C, align(16))]
struct TlsHeapHeader {
    total: usize,
    magic: u32,
    _reserved: u32,
}

const _: () = assert!(core::mem::size_of::<TlsHeapHeader>() == 16);

static TLS_HEAP_CURRENT: AtomicUsize = AtomicUsize::new(0);
static TLS_HEAP_PEAK: AtomicUsize = AtomicUsize::new(0);
static TLS_STACK_SAMPLE_MIN: AtomicUsize = AtomicUsize::new(usize::MAX);

#[inline(always)]
fn sample_tls_stack() {
    unsafe extern "C" {
        static __stack_chk_guard: u32;
    }
    let sp = esp_hal::xtensa_lx::get_stack_pointer() as usize;
    let guard = core::ptr::addr_of!(__stack_chk_guard) as usize;
    TLS_STACK_SAMPLE_MIN.fetch_min(sp.saturating_sub(guard), Ordering::Relaxed);
}

fn tls_heap_stats() -> (usize, usize) {
    (
        TLS_HEAP_CURRENT.load(Ordering::Relaxed),
        TLS_HEAP_PEAK.load(Ordering::Relaxed),
    )
}

unsafe extern "C" fn tls_psram_calloc(n: usize, size: usize) -> *mut c_void {
    sample_tls_stack();
    let Some(bytes) = n.checked_mul(size) else {
        return core::ptr::null_mut();
    };
    let bytes = bytes.max(1);
    let Some(unpadded) = bytes.checked_add(core::mem::size_of::<TlsHeapHeader>()) else {
        return core::ptr::null_mut();
    };
    let Some(total) = unpadded.checked_next_multiple_of(TLS_HEAP_ALIGN) else {
        return core::ptr::null_mut();
    };
    let Ok(layout) = core::alloc::Layout::from_size_align(total, TLS_HEAP_ALIGN) else {
        return core::ptr::null_mut();
    };
    let Ok(block) = esp_alloc::ExternalMemory.allocate(layout) else {
        return core::ptr::null_mut();
    };
    let base = block.as_ptr() as *mut u8;
    core::ptr::write_bytes(base, 0, total);
    let header = base as *mut TlsHeapHeader;
    core::ptr::addr_of_mut!((*header).total).write(total);
    core::ptr::addr_of_mut!((*header).magic).write(TLS_HEAP_MAGIC);
    let current = TLS_HEAP_CURRENT.fetch_add(total, Ordering::Relaxed) + total;
    TLS_HEAP_PEAK.fetch_max(current, Ordering::Relaxed);
    // SAFETY: `base` is a fresh 16-byte-aligned `total`-byte block, so
    // `base + 16` stays 16-byte aligned and in bounds.
    base.add(core::mem::size_of::<TlsHeapHeader>()) as *mut c_void
}

unsafe extern "C" fn tls_psram_free(ptr: *mut c_void) {
    sample_tls_stack();
    if ptr.is_null() {
        return;
    }
    // SAFETY: non-null pointers handed out above are exactly
    // `size_of::<TlsHeapHeader>()` past a live block base.
    let base = (ptr as *mut u8).sub(core::mem::size_of::<TlsHeapHeader>());
    let header = base as *mut TlsHeapHeader;
    assert_eq!(core::ptr::addr_of!((*header).magic).read(), TLS_HEAP_MAGIC);
    let total = core::ptr::addr_of!((*header).total).read();
    let layout = core::alloc::Layout::from_size_align(total, TLS_HEAP_ALIGN)
        .expect("TLS free layout invariant");
    TLS_HEAP_CURRENT.fetch_sub(total, Ordering::Relaxed);
    // SAFETY: `base`/`layout` reproduce the original allocation.
    let base = core::ptr::NonNull::new(base).expect("TLS free pointer invariant");
    esp_alloc::ExternalMemory.deallocate(base, layout);
}

// ---------------------------------------------------------------------------
// Certificate time: compile-time host clock + monotonic elapsed, fail closed.
// ---------------------------------------------------------------------------

fn parse_u32_dec(text: &str) -> Option<u32> {
    if text.is_empty() {
        return None;
    }
    let mut value: u32 = 0;
    for byte in text.bytes() {
        if !byte.is_ascii_digit() {
            return None;
        }
        value = value.checked_mul(10)?.checked_add((byte - b'0') as u32)?;
    }
    Some(value)
}

/// Build-time unix seconds from the host clock. Baked by the build invoker
/// (`HTTPS_PROBE_UTC=$(date +%s)`); `None` when absent or malformed, in
/// which case certificate validation fails closed.
fn probe_utc_base() -> Option<u32> {
    let raw = option_env!("HTTPS_PROBE_UTC")?;
    let base = parse_u32_dec(raw.trim())?;
    if base == 0 {
        None
    } else {
        Some(base)
    }
}

static BOOT_SECONDS: AtomicU32 = AtomicU32::new(0);

fn monotonic_elapsed_secs() -> u64 {
    embassy_time::Instant::now()
        .as_secs()
        .saturating_sub(BOOT_SECONDS.load(Ordering::Relaxed) as u64)
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn unix_to_tm(total: u64) -> Option<tm> {
    if total > i64::MAX as u64 {
        return None;
    }
    let days = (total / 86_400) as i64;
    let secs = (total % 86_400) as i32;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as i64;
    let year = (if m <= 2 { y + 1 } else { y }) as i32;
    if !(1970..=2100).contains(&year) {
        return None;
    }
    let yday = (days - days_from_civil(year as i64, 1, 1)) as i32;
    let wday = ((days + 4) % 7) as i32;
    Some(tm {
        tm_sec: secs % 60,
        tm_min: (secs / 60) % 60,
        tm_hour: secs / 3_600,
        tm_mday: d as i32,
        tm_mon: (m - 1) as i32,
        tm_year: year - 1_900,
        tm_wday: wday,
        tm_yday: yday,
        tm_isdst: 0,
    })
}

struct ProbeWallClock;

impl MbedtlsWallClock for ProbeWallClock {
    fn instant(&self) -> Option<tm> {
        let base = probe_utc_base()?;
        let total = (base as u64).checked_add(monotonic_elapsed_secs())?;
        unix_to_tm(total)
    }
}

static PROBE_WALL_CLOCK: ProbeWallClock = ProbeWallClock;
static PROBE_TIMER: EmbassyTimer = EmbassyTimer;

// ---------------------------------------------------------------------------
// Small internal RNG: esp-hal hardware RNG behind a rand_core 0.10 adapter.
// ---------------------------------------------------------------------------

struct ProbeRng {
    inner: esp_hal::rng::Rng,
}

impl rand_core::TryRng for ProbeRng {
    type Error = core::convert::Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        Ok(self.inner.random())
    }

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        let lo = self.inner.random() as u64;
        let hi = self.inner.random() as u64;
        Ok(lo | (hi << 32))
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        self.inner.read(dst);
        Ok(())
    }
}

impl rand_core::TryCryptoRng for ProbeRng {}

static RNG_CELL: static_cell::StaticCell<ProbeRng> = static_cell::StaticCell::new();

fn log_mem(tag: &str) {
    let snap = psram::allocator_memory_snapshot();
    let (tls_current, tls_peak) = tls_heap_stats();
    console::println!(
        "HTTPS_PROBE mem tag={} internal_free={} internal_min={} psram_free={} tls_current={} tls_peak={} tls_stack_sample_min={}",
        tag,
        snap.free_internal_bytes,
        snap.min_free_internal_bytes,
        snap.free_external_bytes,
        tls_current,
        tls_peak,
        TLS_STACK_SAMPLE_MIN.load(Ordering::Relaxed),
    );
}

#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::_240MHz));
    console::println!("HTTPS_PROBE state=boot standalone=true");
    // Read durable configuration before PSRAM or radio state can be live.
    meditamer_product::firmware::flash::initialize(peripherals.FLASH);
    let credentials = meditamer_product::firmware::flash::load_wifi_credentials()
        .expect("credential partition read failed");
    let status = psram::init_allocator(peripherals.PSRAM);
    assert!(matches!(status.state, psram::AllocatorState::Initialized));
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);
    BOOT_SECONDS.store(
        embassy_time::Instant::now().as_secs() as u32,
        Ordering::Relaxed,
    );
    assert!(
        probe_utc_base().is_some(),
        "set HTTPS_PROBE_UTC to current host UTC"
    );
    // Installed once before any TLS allocation; all hook objects live for the boot.
    unsafe {
        assert_eq!(
            mbedtls_platform_set_calloc_free(Some(tls_psram_calloc), Some(tls_psram_free)),
            0
        );
        hook_timer(Some(&PROBE_TIMER));
        hook_wall_clock(Some(&PROBE_WALL_CLOCK));
    }
    // Retain the UART clock guard through the diverging executor.
    // Dropping the reader would disable UART0 underneath the ROM console.
    let (credentials, _uart_guard) = match credentials {
        Some(credentials) => (credentials, None),
        None => {
            let (credentials, uart) = provisioning::receive(peripherals.UART0, peripherals.GPIO3);
            (credentials, Some(uart))
        }
    };
    let (controller, device) =
        netstack::wifi::initialize_runtime_sta(peripherals.WIFI).expect("wifi init failed");
    let (stack, runner) = embassy_net::new(
        device,
        embassy_net::Config::dhcpv4(Default::default()),
        netstack::stack_resources(),
        esp_hal::rng::Rng::new().random() as u64,
    );
    static EXECUTOR: static_cell::StaticCell<esp_rtos::embassy::Executor> =
        static_cell::StaticCell::new();
    EXECUTOR
        .init(esp_rtos::embassy::Executor::new())
        .run(|spawner| {
            spawner.spawn(network(runner).unwrap());
            spawner.spawn(probe(controller, stack, credentials).unwrap());
        })
}

#[embassy_executor::task]
async fn network(mut runner: embassy_net::Runner<'static, netstack::wifi::WifiDevice>) {
    runner.run().await
}

#[embassy_executor::task]
async fn probe(
    mut controller: netstack::wifi::WifiController<'static>,
    stack: embassy_net::Stack<'static>,
    credentials: netstack::config::WifiCredentials,
) {
    use embassy_time::{with_timeout, Duration};
    use netstack::wifi;
    let ssid = core::str::from_utf8(&credentials.ssid[..credentials.ssid_len as usize]).unwrap();
    let password =
        core::str::from_utf8(&credentials.password[..credentials.password_len as usize]).unwrap();
    let config = wifi::wifi_client_mode_config(
        ssid,
        password,
        esp_radio::wifi::AuthenticationMethod::Wpa2Personal,
        None,
        None,
    )
    .expect("probe credentials must form a valid station config");
    wifi::wifi_set_config(&mut controller, &config).unwrap();
    console::println!("HTTPS_PROBE state=connecting");
    let result = with_timeout(
        Duration::from_secs(30),
        wifi::wifi_connect_async(&mut controller),
    )
    .await;
    if !matches!(result, Ok(Ok(()))) {
        console::println!("HTTPS_PROBE state=connect_failed");
        return;
    }
    if with_timeout(Duration::from_secs(30), stack.wait_config_up())
        .await
        .is_err()
    {
        console::println!("HTTPS_PROBE state=dhcp_failed");
        return;
    }
    log_mem("wifi_ready");
    // TLS and HTTP futures run only while RF is enabled and no flash operations occur.
    // Neither scratch nor futures are DMA sources; the Wi-Fi driver owns its staging.
    let mut work =
        psram::ExternalValue::try_new_with(|| requests(stack)).expect("PSRAM future allocation");
    console::println!(
        "HTTPS_PROBE request_future_bytes={}",
        core::mem::size_of_val(&*work)
    );
    work.pin_mut().await;
    drop(work);
    log_mem("probe_done");
    console::println!("HTTPS_PROBE state=done");
}

async fn requests(stack: embassy_net::Stack<'static>) {
    use embassy_time::{with_timeout, Duration, Timer};
    let addresses = match with_timeout(
        Duration::from_secs(10),
        stack.dns_query("google.com", embassy_net::dns::DnsQueryType::A),
    )
    .await
    {
        Ok(Ok(addresses)) if !addresses.is_empty() => addresses,
        _ => {
            console::println!("HTTPS_PROBE state=dns_failed");
            return;
        }
    };
    console::println!("HTTPS_PROBE peer={}", addresses[0]);
    // RF remains enabled through every TLS use, satisfying Rng's entropy condition.
    let rng = RNG_CELL.init(ProbeRng {
        inner: esp_hal::rng::Rng::new(),
    });
    let tls = mbedtls_rs::Tls::new(rng).expect("TLS init");
    let tls_baseline = TLS_HEAP_CURRENT.load(Ordering::Relaxed);
    for cycle in 1..=3 {
        TLS_HEAP_PEAK.store(tls_baseline, Ordering::Relaxed);
        log_mem("cycle_before");
        let result = with_timeout(
            Duration::from_secs(40),
            request(stack, addresses[0], tls.reference()),
        )
        .await;
        console::println!("HTTPS_PROBE cycle={} result={:?}", cycle, result);
        log_mem("cycle_after");
        assert_eq!(
            TLS_HEAP_CURRENT.load(Ordering::Relaxed),
            tls_baseline,
            "TLS allocations leaked"
        );
        Timer::after_secs(1).await;
    }
    drop(tls);
    assert_eq!(TLS_HEAP_CURRENT.load(Ordering::Relaxed), 0);
    log_mem("tls_dropped");
}

#[inline(never)]
async fn request(
    stack: embassy_net::Stack<'static>,
    ip: embassy_net::IpAddress,
    tls: mbedtls_rs::TlsReference<'_>,
) -> Result<(), &'static str> {
    use embedded_io_async::Write;
    use mbedtls_rs::{Certificate, ClientSessionConfig, SessionConfig, TlsVersion, X509};
    let mut rx =
        psram::ExternalValue::try_new_with(|| [0u8; 4096]).map_err(|_| "TCP RX allocation")?;
    let mut tx =
        psram::ExternalValue::try_new_with(|| [0u8; 2048]).map_err(|_| "TCP TX allocation")?;
    let mut tcp = embassy_net::tcp::TcpSocket::new(stack, &mut rx[..], &mut tx[..]);
    tcp.set_timeout(Some(embassy_time::Duration::from_secs(15)));
    tcp.connect((ip, 443)).await.map_err(|_| "TCP connect")?;
    let config = SessionConfig::Client(ClientSessionConfig {
        ca_chain: Some(Certificate::new(X509::PEM(GTS_ROOTS_PEM)).map_err(|error| {
            console::println!("HTTPS_PROBE state=ca_parse_failed error={:?}", error);
            "CA parse"
        })?),
        server_name: Some(c"google.com"),
        min_version: TlsVersion::Tls1_3,
        max_version: Some(TlsVersion::Tls1_3),
        ..Default::default()
    });
    let mut session =
        mbedtls_rs::Session::new(tls, &mut tcp, &config).map_err(|_| "session init")?;
    if let Err(error) = session.connect().await {
        console::println!(
            "HTTPS_PROBE state=handshake_failed error={:?} verify={}",
            error,
            session.tls_verification_details()
        );
        return Err("TLS handshake");
    }
    assert_eq!(session.tls_verification_details(), 0);
    console::println!(
        "HTTPS_PROBE state=authenticated version={:?}",
        session.tls_version()
    );
    log_mem("handshake_done");
    session
        .write_all(b"GET / HTTP/1.1\r\nHost: google.com\r\nConnection: close\r\n\r\n")
        .await
        .map_err(|_| "HTTP write")?;
    session.flush().await.map_err(|_| "HTTP flush")?;
    let mut response = [0u8; 256];
    let mut count = 0;
    let end = loop {
        if count == response.len() {
            return Err("HTTP status too long");
        }
        let n = session
            .read(&mut response[count..])
            .await
            .map_err(|_| "HTTP read")?;
        if n == 0 {
            return Err("EOF before HTTP status");
        }
        count += n;
        if let Some(end) = response[..count].windows(2).position(|w| w == b"\r\n") {
            break end;
        }
    };
    let status = &response[..end];
    let status = core::str::from_utf8(status).map_err(|_| "HTTP status encoding")?;
    if !status.starts_with("HTTP/1.1 ") && !status.starts_with("HTTP/1.0 ") {
        return Err("HTTP status");
    }
    console::println!(
        "HTTPS_PROBE state=response status={} bytes={}",
        status,
        count
    );
    // Closing is bounded too; dropping always releases the owned session state.
    let _ = embassy_time::with_timeout(embassy_time::Duration::from_secs(3), session.close()).await;
    drop(session);
    tcp.abort();
    Ok(())
}
