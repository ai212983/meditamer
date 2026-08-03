use embassy_net::{tcp::TcpSocket, IpListenEndpoint, Stack};
use embassy_time::{Duration, Instant, Timer};

mod connection;
mod helpers;

use super::super::super::types::{HTTP_RX_BUF_TARGET_BYTES, SD_UPLOAD_CHUNK_MAX};
use crate::firmware::psram;
use crate::firmware::runtime::service_mode;
use crate::firmware::telemetry;

const UPLOAD_HTTP_PORT: u16 = 8080;
const UPLOAD_HTTP_ROOT: &str = "/assets";
const UPLOAD_HTTP_TOKEN_HEADER: &str = "x-upload-token";
const HTTP_HEADER_MAX: usize = 1024;
const HTTP_RW_BUF_FALLBACK: usize = 512;
// Keep a large RX window in PSRAM so /upload can continue receiving while the
// SD writer is busy; this trims body-read stalls between chunk roundtrips.
const HTTP_RX_BUF_TARGET: usize = HTTP_RX_BUF_TARGET_BYTES;
// TX path is response-only for this listener, so a smaller buffer is enough.
const HTTP_TX_BUF_TARGET: usize = 4_096;
const HTTP_CHUNK_BUF_FALLBACK: usize = 1024;
const HTTP_CHUNK_BUF_TARGET: usize = SD_UPLOAD_CHUNK_MAX;
const HTTP_SOCKET_TIMEOUT_SECS: u64 = 60;
const DHCP_POLL_MS: u64 = 250;

enum HttpBuffer<const N: usize> {
    Psram(psram::LargeByteBuffer),
}

impl<const N: usize> HttpBuffer<N> {
    fn as_mut_slice(&mut self) -> &mut [u8] {
        match self {
            Self::Psram(buffer) => buffer.as_mut_slice(),
        }
    }
}

fn init_http_buffer<const N: usize>(alloc_bytes: usize, tag: &'static str) -> Option<HttpBuffer<N>> {
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
            Some(HttpBuffer::Psram(buffer))
        }
        Err(err) => {
            esp_println::println!(
                "upload_http: buffer_alloc_failed tag={} placement=psram err={:?}",
                tag,
                err
            );
            None
        }
    }
}

struct HttpServerLoopState {
    listening_logged: bool,
    waiting_dhcp_logged: bool,
    dhcp_wait_started_at: Option<Instant>,
    dhcp_gate_started_at: Option<Instant>,
    dhcp_gate_last_reason: Option<telemetry::NetPipelineGate>,
    dhcp_ready: bool,
    transfers_pause_started_at: Option<Instant>,
    listener_gate_last_enabled: bool,
    listener_gate_last_seq: u32,
    listener_gate_disabled_logged: bool,
    last_request_closed_at: Option<Instant>,
    last_request_route: Option<connection::RequestRouteKind>,
}

impl HttpServerLoopState {
    fn new() -> Self {
        Self {
            listening_logged: false,
            waiting_dhcp_logged: false,
            dhcp_wait_started_at: None,
            dhcp_gate_started_at: None,
            dhcp_gate_last_reason: None,
            dhcp_ready: false,
            transfers_pause_started_at: None,
            listener_gate_last_enabled: service_mode::upload_http_listener_enabled(),
            listener_gate_last_seq: service_mode::upload_http_listener_set_seq(),
            listener_gate_disabled_logged: false,
            last_request_closed_at: None,
            last_request_route: None,
        }
    }

    fn reset_all(&mut self) {
        self.listening_logged = false;
        self.waiting_dhcp_logged = false;
        self.dhcp_wait_started_at = None;
        self.dhcp_gate_started_at = None;
        self.dhcp_gate_last_reason = None;
        self.dhcp_ready = false;
        self.last_request_closed_at = None;
        self.last_request_route = None;
    }

    fn reset_link_state(&mut self) {
        self.listening_logged = false;
        self.waiting_dhcp_logged = false;
        self.dhcp_gate_started_at = None;
        self.dhcp_gate_last_reason = None;
        self.dhcp_ready = false;
    }
}

struct HttpServerBuffers {
    rx: Option<HttpBuffer<HTTP_RW_BUF_FALLBACK>>,
    tx: Option<HttpBuffer<HTTP_RW_BUF_FALLBACK>>,
    header: Option<HttpBuffer<HTTP_HEADER_MAX>>,
    chunk: Option<HttpBuffer<HTTP_CHUNK_BUF_FALLBACK>>,
}

impl HttpServerBuffers {
    fn new() -> Self {
        Self {
            rx: None,
            tx: None,
            header: None,
            chunk: None,
        }
    }

    fn ensure_initialized(&mut self) {
        if self.rx.is_some()
            && self.tx.is_some()
            && self.header.is_some()
            && self.chunk.is_some()
        {
            return;
        }

        if self.rx.is_none() {
            self.rx = init_http_buffer(
                HTTP_RX_BUF_TARGET,
                "http_rx",
            );
        }
        if self.tx.is_none() {
            self.tx = init_http_buffer(
                HTTP_TX_BUF_TARGET,
                "http_tx",
            );
        }
        if self.header.is_none() {
            self.header = init_http_buffer(
                HTTP_HEADER_MAX,
                "http_header",
            );
        }
        if self.chunk.is_none() {
            self.chunk = init_http_buffer(
                HTTP_CHUNK_BUF_TARGET,
                "http_chunk",
            );
        }
        log_http_mem_diag("buffers_init");
    }

    fn borrow_mut(&mut self) -> Option<HttpServerBuffersMut<'_>> {
        Some(HttpServerBuffersMut {
            rx: self.rx.as_mut()?,
            tx: self.tx.as_mut()?,
            header: self.header.as_mut()?,
            chunk: self.chunk.as_mut()?,
        })
    }
}

struct HttpServerBuffersMut<'a> {
    rx: &'a mut HttpBuffer<HTTP_RW_BUF_FALLBACK>,
    tx: &'a mut HttpBuffer<HTTP_RW_BUF_FALLBACK>,
    header: &'a mut HttpBuffer<HTTP_HEADER_MAX>,
    chunk: &'a mut HttpBuffer<HTTP_CHUNK_BUF_FALLBACK>,
}
