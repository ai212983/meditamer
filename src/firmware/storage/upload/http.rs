//! Upload HTTP server.
//!
//! [`server_loop`] owns the listener lifecycle, [`socket_cycle`] one accepted
//! connection, [`connection`] the request parsing and routing,
//! [`listener_gate`] the DHCP/link gating they share, and [`mem_diag`] the
//! shared memory logging. The buffers and loop state live here.

use core::sync::atomic::{AtomicU16, Ordering};
use embassy_time::Instant;

mod connection;
mod helpers;
mod listener_gate;
mod mem_diag;
mod server_loop;
mod socket_cycle;

use mem_diag::log_http_mem_diag;

pub(super) use server_loop::run_http_server;

use super::super::super::types::{HTTP_RX_BUF_TARGET_BYTES, SD_UPLOAD_CHUNK_MAX};
use crate::firmware::observability;
use crate::firmware::psram;
use crate::firmware::service_mode;

pub(super) const UPLOAD_HTTP_PORT: u16 = 8080;
pub(super) const UPLOAD_HTTP_ROOT: &str = "/assets";
pub(super) const UPLOAD_HTTP_TOKEN_HEADER: &str = "x-upload-token";
pub(super) const HTTP_HEADER_MAX: usize = 1024;
pub(super) const HTTP_RW_BUF_FALLBACK: usize = 512;
// Keep a large RX window in PSRAM so /upload can continue receiving while the
// SD writer is busy; this trims body-read stalls between chunk roundtrips.
pub const HTTP_RX_BUF_TARGET: usize = HTTP_RX_BUF_TARGET_BYTES;
// TX path is response-only for this listener, so a smaller buffer is enough.
const HTTP_TX_BUF_TARGET: usize = 4_096;
pub(super) const HTTP_CHUNK_BUF_FALLBACK: usize = 1024;
const HTTP_CHUNK_BUF_TARGET: usize = SD_UPLOAD_CHUNK_MAX;
pub const HTTP_SOCKET_TIMEOUT_SECS: u64 = 60;
pub(super) const DHCP_POLL_MS: u64 = 250;

static ACTIVE_CONNECTIONS: AtomicU16 = AtomicU16::new(0);

pub(in crate::firmware::storage::upload) fn active_connections() -> u16 {
    ACTIVE_CONNECTIONS.load(Ordering::Acquire)
}

pub(super) struct ActiveConnectionGuard;

impl ActiveConnectionGuard {
    pub(super) fn enter() -> Self {
        ACTIVE_CONNECTIONS.fetch_add(1, Ordering::AcqRel);
        Self
    }
}

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        ACTIVE_CONNECTIONS.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(super) enum HttpBuffer<const N: usize> {
    Psram(psram::LargeByteBuffer),
}

impl<const N: usize> HttpBuffer<N> {
    pub(super) fn as_mut_slice(&mut self) -> &mut [u8] {
        match self {
            Self::Psram(buffer) => buffer.as_mut_slice(),
        }
    }
}

fn init_http_buffer<const N: usize>(
    alloc_bytes: usize,
    tag: &'static str,
) -> Option<HttpBuffer<N>> {
    match psram::alloc_large_byte_buffer(alloc_bytes) {
        Ok(buffer) => {
            if observability::log_filter_enabled(observability::LOG_DOMAIN_HTTP) {
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

pub(super) struct HttpServerLoopState {
    listening_logged: bool,
    waiting_dhcp_logged: bool,
    dhcp_wait_started_at: Option<Instant>,
    dhcp_gate_started_at: Option<Instant>,
    dhcp_gate_last_reason: Option<observability::NetPipelineGate>,
    dhcp_ready: bool,
    transfers_pause_started_at: Option<Instant>,
    listener_gate_last_enabled: bool,
    listener_gate_last_seq: u32,
    listener_gate_disabled_logged: bool,
    last_request_closed_at: Option<Instant>,
    last_request_route: Option<connection::RequestRouteKind>,
}

impl HttpServerLoopState {
    pub(super) fn new() -> Self {
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

    pub(super) fn reset_all(&mut self) {
        self.listening_logged = false;
        self.waiting_dhcp_logged = false;
        self.dhcp_wait_started_at = None;
        self.dhcp_gate_started_at = None;
        self.dhcp_gate_last_reason = None;
        self.dhcp_ready = false;
        self.last_request_closed_at = None;
        self.last_request_route = None;
    }

    pub(super) fn reset_link_state(&mut self) {
        self.listening_logged = false;
        self.waiting_dhcp_logged = false;
        self.dhcp_gate_started_at = None;
        self.dhcp_gate_last_reason = None;
        self.dhcp_ready = false;
    }
}

pub(super) struct HttpServerBuffers {
    rx: Option<HttpBuffer<HTTP_RW_BUF_FALLBACK>>,
    tx: Option<HttpBuffer<HTTP_RW_BUF_FALLBACK>>,
    header: Option<HttpBuffer<HTTP_HEADER_MAX>>,
    chunk: Option<HttpBuffer<HTTP_CHUNK_BUF_FALLBACK>>,
}

impl HttpServerBuffers {
    pub(super) fn new() -> Self {
        Self {
            rx: None,
            tx: None,
            header: None,
            chunk: None,
        }
    }

    pub(super) fn ensure_initialized(&mut self) {
        if self.rx.is_some() && self.tx.is_some() && self.header.is_some() && self.chunk.is_some() {
            return;
        }

        if self.rx.is_none() {
            self.rx = init_http_buffer(HTTP_RX_BUF_TARGET, "http_rx");
        }
        if self.tx.is_none() {
            self.tx = init_http_buffer(HTTP_TX_BUF_TARGET, "http_tx");
        }
        if self.header.is_none() {
            self.header = init_http_buffer(HTTP_HEADER_MAX, "http_header");
        }
        if self.chunk.is_none() {
            self.chunk = init_http_buffer(HTTP_CHUNK_BUF_TARGET, "http_chunk");
        }
        log_http_mem_diag("buffers_init");
    }

    pub(super) fn borrow_mut(&mut self) -> Option<HttpServerBuffersMut<'_>> {
        Some(HttpServerBuffersMut {
            rx: self.rx.as_mut()?,
            tx: self.tx.as_mut()?,
            header: self.header.as_mut()?,
            chunk: self.chunk.as_mut()?,
        })
    }
}

pub(super) struct HttpServerBuffersMut<'a> {
    pub(super) rx: &'a mut HttpBuffer<HTTP_RW_BUF_FALLBACK>,
    pub(super) tx: &'a mut HttpBuffer<HTTP_RW_BUF_FALLBACK>,
    pub(super) header: &'a mut HttpBuffer<HTTP_HEADER_MAX>,
    pub(super) chunk: &'a mut HttpBuffer<HTTP_CHUNK_BUF_FALLBACK>,
}
