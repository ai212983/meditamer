use embassy_net::{tcp::TcpSocket, IpListenEndpoint, Stack};
use embassy_time::{with_timeout, Duration, Instant, Timer};
use static_cell::StaticCell;

mod connection;
mod helpers;

#[cfg(feature = "psram-alloc")]
use super::super::super::types::SD_UPLOAD_CHUNK_MAX;
use crate::firmware::psram;
use crate::firmware::runtime::service_mode;
use crate::firmware::telemetry;

const UPLOAD_HTTP_PORT: u16 = 8080;
const UPLOAD_HTTP_ROOT: &str = "/assets";
const UPLOAD_HTTP_TOKEN_HEADER: &str = "x-upload-token";
#[cfg(feature = "psram-alloc")]
const HTTP_HEADER_MAX: usize = 1024;
#[cfg(not(feature = "psram-alloc"))]
const HTTP_HEADER_MAX: usize = 2048;
#[cfg(feature = "psram-alloc")]
const HTTP_RW_BUF_FALLBACK: usize = 512;
#[cfg(not(feature = "psram-alloc"))]
const HTTP_RW_BUF_FALLBACK: usize = 2048;
#[cfg(feature = "psram-alloc")]
// Keep a large RX window in PSRAM so /upload can continue receiving while the
// SD writer is busy; this trims body-read stalls between chunk roundtrips.
const HTTP_RX_BUF_TARGET: usize = 65_536;
#[cfg(feature = "psram-alloc")]
// TX path is response-only for this listener, so a smaller buffer is enough.
const HTTP_TX_BUF_TARGET: usize = 4_096;
#[cfg(not(feature = "psram-alloc"))]
const HTTP_RX_BUF_TARGET: usize = HTTP_RW_BUF_FALLBACK;
#[cfg(not(feature = "psram-alloc"))]
const HTTP_TX_BUF_TARGET: usize = HTTP_RW_BUF_FALLBACK;
#[cfg(feature = "psram-alloc")]
const HTTP_CHUNK_BUF_FALLBACK: usize = 1024;
#[cfg(not(feature = "psram-alloc"))]
const HTTP_CHUNK_BUF_FALLBACK: usize = 4096;
#[cfg(feature = "psram-alloc")]
const HTTP_CHUNK_BUF_TARGET: usize = SD_UPLOAD_CHUNK_MAX;
#[cfg(not(feature = "psram-alloc"))]
const HTTP_CHUNK_BUF_TARGET: usize = HTTP_CHUNK_BUF_FALLBACK;
const HTTP_SOCKET_TIMEOUT_SECS: u64 = 60;
const DHCP_POLL_MS: u64 = 250;

enum HttpBuffer<const N: usize> {
    #[cfg(feature = "psram-alloc")]
    Psram(psram::LargeByteBuffer),
    Internal(&'static mut [u8; N]),
}

impl<const N: usize> HttpBuffer<N> {
    fn as_mut_slice(&mut self) -> &mut [u8] {
        match self {
            #[cfg(feature = "psram-alloc")]
            Self::Psram(buffer) => buffer.as_mut_slice(),
            Self::Internal(buffer) => &mut buffer[..],
        }
    }
}

fn init_http_buffer<const N: usize>(
    cell: &'static StaticCell<[u8; N]>,
    #[cfg_attr(not(feature = "psram-alloc"), allow(unused_variables))] alloc_bytes: usize,
    #[cfg_attr(not(feature = "psram-alloc"), allow(unused_variables))] tag: &'static str,
) -> HttpBuffer<N> {
    #[cfg(feature = "psram-alloc")]
    {
        match psram::alloc_large_byte_buffer(alloc_bytes) {
            Ok(buffer) => {
                if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_HTTP) {
                    esp_println::println!(
                        "upload_http: {} buffer placement={:?} bytes={}",
                        tag,
                        buffer.placement(),
                        alloc_bytes
                    );
                }
                psram::log_allocator_high_water(tag);
                return HttpBuffer::Psram(buffer);
            }
            Err(err) => {
                if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_HTTP) {
                    esp_println::println!(
                        "upload_http: {} psram alloc failed ({:?}); using internal ram",
                        tag,
                        err
                    );
                }
            }
        }
    }

    HttpBuffer::Internal(cell.init([0u8; N]))
}

struct HttpServerLoopState {
    listening_logged: bool,
    waiting_dhcp_logged: bool,
    dhcp_wait_started_at: Option<Instant>,
    dhcp_ready: bool,
}

impl HttpServerLoopState {
    fn new() -> Self {
        Self {
            listening_logged: false,
            waiting_dhcp_logged: false,
            dhcp_wait_started_at: None,
            dhcp_ready: false,
        }
    }

    fn reset_all(&mut self) {
        self.listening_logged = false;
        self.waiting_dhcp_logged = false;
        self.dhcp_wait_started_at = None;
        self.dhcp_ready = false;
    }

    fn reset_link_state(&mut self) {
        self.listening_logged = false;
        self.waiting_dhcp_logged = false;
        self.dhcp_ready = false;
    }
}

pub(super) async fn run_http_server(stack: Stack<'static>) {
    static RX_BUFFER: StaticCell<[u8; HTTP_RW_BUF_FALLBACK]> = StaticCell::new();
    static TX_BUFFER: StaticCell<[u8; HTTP_RW_BUF_FALLBACK]> = StaticCell::new();
    static HEADER_BUFFER: StaticCell<[u8; HTTP_HEADER_MAX]> = StaticCell::new();
    static CHUNK_BUFFER: StaticCell<[u8; HTTP_CHUNK_BUF_FALLBACK]> = StaticCell::new();

    let mut rx_buffer: Option<HttpBuffer<HTTP_RW_BUF_FALLBACK>> = None;
    let mut tx_buffer: Option<HttpBuffer<HTTP_RW_BUF_FALLBACK>> = None;
    let mut header_buffer: Option<HttpBuffer<HTTP_HEADER_MAX>> = None;
    let mut chunk_buffer: Option<HttpBuffer<HTTP_CHUNK_BUF_FALLBACK>> = None;
    let mut state = HttpServerLoopState::new();
    telemetry::set_upload_http_listener(false, None);

    loop {
        if !service_mode_ready(&mut state).await {
            continue;
        }

        let Some(local_ipv4) = gate_dhcp_ipv4(&stack, &mut state).await else {
            continue;
        };
        log_listener_start(local_ipv4, &mut state);
        ensure_http_buffers(
            &mut rx_buffer,
            &mut tx_buffer,
            &mut header_buffer,
            &mut chunk_buffer,
            &RX_BUFFER,
            &TX_BUFFER,
            &HEADER_BUFFER,
            &CHUNK_BUFFER,
        );

        let (Some(rx_buffer), Some(tx_buffer), Some(header_buffer), Some(chunk_buffer)) = (
            rx_buffer.as_mut(),
            tx_buffer.as_mut(),
            header_buffer.as_mut(),
            chunk_buffer.as_mut(),
        ) else {
            telemetry::set_upload_http_listener(false, Some(local_ipv4));
            Timer::after(Duration::from_millis(250)).await;
            continue;
        };

        serve_connection_cycle(
            stack,
            &mut state,
            local_ipv4,
            rx_buffer,
            tx_buffer,
            header_buffer,
            chunk_buffer,
        )
        .await;
    }
}

async fn service_mode_ready(state: &mut HttpServerLoopState) -> bool {
    if !service_mode::upload_transfers_enabled() {
        state.reset_all();
        telemetry::set_upload_http_listener(false, None);
        Timer::after(Duration::from_millis(500)).await;
        return false;
    }

    if !service_mode::upload_http_listener_enabled() {
        state.reset_all();
        telemetry::set_upload_http_listener(false, None);
        log_http_mem_diag("listener_disabled_pause");
        Timer::after(Duration::from_millis(500)).await;
        return false;
    }

    true
}

async fn gate_dhcp_ipv4(
    stack: &Stack<'static>,
    state: &mut HttpServerLoopState,
) -> Option<[u8; 4]> {
    // Gate HTTP on active link + DHCP lease to avoid advertising an unusable listener.
    let local_ipv4 = match dhcp_ipv4_status(stack) {
        Ok(ipv4) => ipv4,
        Err(gate_reason) => {
            telemetry::record_net_pipeline_gate(gate_reason);
            state.dhcp_ready = false;
            state.listening_logged = false;
            telemetry::set_upload_http_listener(false, None);

            if !state.waiting_dhcp_logged {
                if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_NET) {
                    esp_println::println!("upload_http: waiting for dhcp ipv4 lease");
                }
                state.waiting_dhcp_logged = true;
            }

            if state.dhcp_wait_started_at.is_none() {
                state.dhcp_wait_started_at = Some(Instant::now());
            }

            Timer::after(Duration::from_millis(DHCP_POLL_MS)).await;
            return None;
        }
    };

    if let Some(started_at) = state.dhcp_wait_started_at.take() {
        telemetry::record_net_pipeline_dhcp_wait(elapsed_ms_u32(started_at));
    }
    if !state.dhcp_ready {
        telemetry::record_net_pipeline_dhcp_ready();
        state.dhcp_ready = true;
    }
    if state.waiting_dhcp_logged {
        if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_NET) {
            esp_println::println!(
                "upload_http: dhcp ipv4 ready {}.{}.{}.{}",
                local_ipv4[0],
                local_ipv4[1],
                local_ipv4[2],
                local_ipv4[3]
            );
        }
        state.waiting_dhcp_logged = false;
    }

    Some(local_ipv4)
}

fn log_listener_start(local_ipv4: [u8; 4], state: &mut HttpServerLoopState) {
    if state.listening_logged {
        return;
    }

    if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_NET) {
        esp_println::println!(
            "upload_http: listening on {}.{}.{}.{}:{}",
            local_ipv4[0],
            local_ipv4[1],
            local_ipv4[2],
            local_ipv4[3],
            UPLOAD_HTTP_PORT
        );
    }
    state.listening_logged = true;
}

fn ensure_http_buffers(
    rx_buffer: &mut Option<HttpBuffer<HTTP_RW_BUF_FALLBACK>>,
    tx_buffer: &mut Option<HttpBuffer<HTTP_RW_BUF_FALLBACK>>,
    header_buffer: &mut Option<HttpBuffer<HTTP_HEADER_MAX>>,
    chunk_buffer: &mut Option<HttpBuffer<HTTP_CHUNK_BUF_FALLBACK>>,
    rx_cell: &'static StaticCell<[u8; HTTP_RW_BUF_FALLBACK]>,
    tx_cell: &'static StaticCell<[u8; HTTP_RW_BUF_FALLBACK]>,
    header_cell: &'static StaticCell<[u8; HTTP_HEADER_MAX]>,
    chunk_cell: &'static StaticCell<[u8; HTTP_CHUNK_BUF_FALLBACK]>,
) {
    if rx_buffer.is_some() {
        return;
    }

    *rx_buffer = Some(init_http_buffer(rx_cell, HTTP_RX_BUF_TARGET, "http_rx"));
    *tx_buffer = Some(init_http_buffer(tx_cell, HTTP_TX_BUF_TARGET, "http_tx"));
    *header_buffer = Some(init_http_buffer(
        header_cell,
        HTTP_HEADER_MAX,
        "http_header",
    ));
    *chunk_buffer = Some(init_http_buffer(
        chunk_cell,
        HTTP_CHUNK_BUF_TARGET,
        "http_chunk",
    ));
    log_http_mem_diag("buffers_init");
}

async fn serve_connection_cycle(
    stack: Stack<'static>,
    state: &mut HttpServerLoopState,
    local_ipv4: [u8; 4],
    rx_buffer: &mut HttpBuffer<HTTP_RW_BUF_FALLBACK>,
    tx_buffer: &mut HttpBuffer<HTTP_RW_BUF_FALLBACK>,
    header_buffer: &mut HttpBuffer<HTTP_HEADER_MAX>,
    chunk_buffer: &mut HttpBuffer<HTTP_CHUNK_BUF_FALLBACK>,
) {
    let mut socket = TcpSocket::new(stack, rx_buffer.as_mut_slice(), tx_buffer.as_mut_slice());
    socket.set_timeout(Some(Duration::from_secs(HTTP_SOCKET_TIMEOUT_SECS)));
    telemetry::set_upload_http_listener(true, Some(local_ipv4));

    if !accept_connection(&mut socket, &stack, state).await {
        return;
    }

    handle_connection_request(
        &mut socket,
        chunk_buffer.as_mut_slice(),
        header_buffer.as_mut_slice(),
    )
    .await;

    let _ = with_timeout(Duration::from_millis(250), socket.flush()).await;
    socket.close();
    log_http_mem_diag("request_close");
}

async fn accept_connection(
    socket: &mut TcpSocket<'_>,
    stack: &Stack<'static>,
    state: &mut HttpServerLoopState,
) -> bool {
    log_http_mem_diag("accept_before");
    let accept_started_at = Instant::now();
    let accepted = socket
        .accept(IpListenEndpoint {
            addr: None,
            port: UPLOAD_HTTP_PORT,
        })
        .await;
    telemetry::record_net_pipeline_accept_wait(elapsed_ms_u32(accept_started_at));

    if let Err(err) = accepted {
        telemetry::record_upload_http_accept_error();
        if dhcp_ipv4_status(stack).is_err() {
            telemetry::record_upload_http_accept_link_reset();
            state.reset_link_state();
        }
        telemetry::set_upload_http_listener(false, None);
        if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_NET) {
            esp_println::println!("upload_http: accept err={:?}", err);
        }
        log_http_mem_diag("accept_err");
        let _ = with_timeout(Duration::from_millis(250), socket.flush()).await;
        socket.abort();
        return false;
    }

    telemetry::record_upload_http_accept();
    log_http_mem_diag("accept_ok");
    if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_HTTP) {
        esp_println::println!("upload_http: accepted connection");
    }
    true
}

async fn handle_connection_request(
    socket: &mut TcpSocket<'_>,
    chunk_buf: &mut [u8],
    header_buf: &mut [u8],
) {
    log_http_mem_diag("request_begin");
    if let Err(err) = connection::handle_connection(socket, chunk_buf, header_buf).await {
        telemetry::record_upload_http_request_error();
        telemetry::record_upload_http_request_bucket(err);
        log_http_mem_diag("request_err");
        if telemetry::diag_enabled(telemetry::DIAG_DOMAIN_HTTP) {
            esp_println::println!(
                "upload_http: request err={} recv_queue={} send_queue={} state={:?} remote={:?}",
                err,
                socket.recv_queue(),
                socket.send_queue(),
                socket.state(),
                socket.remote_endpoint(),
            );
        }
    } else {
        log_http_mem_diag("request_ok");
    }
}

fn dhcp_ipv4_status(stack: &Stack<'static>) -> Result<[u8; 4], telemetry::NetPipelineGate> {
    if !telemetry::wifi_link_connected() || !stack.is_link_up() {
        if !telemetry::wifi_link_connected() {
            return Err(telemetry::NetPipelineGate::WifiDown);
        }
        return Err(telemetry::NetPipelineGate::LinkDown);
    }
    stack
        .config_v4()
        .map(|cfg| cfg.address.address().octets())
        .filter(|ip| *ip != [0, 0, 0, 0])
        .ok_or(telemetry::NetPipelineGate::NoIpv4)
}

fn elapsed_ms_u32(started_at: Instant) -> u32 {
    let elapsed = started_at.elapsed().as_millis();
    if elapsed > u32::MAX as u64 {
        u32::MAX
    } else {
        elapsed as u32
    }
}

fn log_http_mem_diag(stage: &str) {
    if !telemetry::diag_enabled(telemetry::DIAG_DOMAIN_HTTP)
        && !telemetry::diag_enabled(telemetry::DIAG_DOMAIN_NET)
    {
        return;
    }
    let snapshot = psram::allocator_memory_snapshot();
    esp_println::println!(
        "upload_http: upload_mem stage={} feature={} state={:?} total={} used={} free={} peak={} internal_free={} external_free={} min_free={} min_internal_free={} min_external_free={} large_alloc_external_ok={} large_alloc_internal_ok={} large_alloc_fail={}",
        stage,
        snapshot.feature_enabled,
        snapshot.state,
        snapshot.total_bytes,
        snapshot.used_bytes,
        snapshot.free_bytes,
        snapshot.peak_used_bytes,
        snapshot.free_internal_bytes,
        snapshot.free_external_bytes,
        snapshot.min_free_bytes,
        snapshot.min_free_internal_bytes,
        snapshot.min_free_external_bytes,
        snapshot.large_alloc_external_ok,
        snapshot.large_alloc_internal_ok,
        snapshot.large_alloc_fail
    );
}
