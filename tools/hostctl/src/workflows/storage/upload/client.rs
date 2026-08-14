mod config;
mod errors;
mod request;
mod retry;
#[cfg(test)]
mod tests;

use super::{direct_stream, UploadRetryPolicy};

#[derive(Clone, Copy)]
pub(super) struct RequestContext<'a> {
    pub(super) host: &'a str,
    pub(super) port: u16,
    pub(super) timeout_sec: f64,
    pub(super) token: Option<&'a str>,
    pub(super) retry_policy: UploadRetryPolicy,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct RequestBodyCadence {
    pub(super) read_calls: u32,
    pub(super) short_reads: u32,
    pub(super) read_bytes: usize,
    pub(super) read_gap_ms_total: u32,
    pub(super) read_gap_ms_max: u32,
    pub(super) read_gap_over_10ms: u32,
    pub(super) read_gap_over_50ms: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct RequestTiming {
    pub(super) body_bytes: usize,
    pub(super) send_ms: u32,
    pub(super) response_read_ms: u32,
    pub(super) total_ms: u32,
    pub(super) body_cadence: RequestBodyCadence,
}

#[derive(Debug)]
pub(super) struct TimedResponse {
    pub(super) body: Vec<u8>,
    pub(super) timing: RequestTiming,
    pub(super) attempts: usize,
}

const BODY_READ_GAP_OVER_10MS: u32 = 10;
const BODY_READ_GAP_OVER_50MS: u32 = 50;
const DIRECT_BURST_BYTES_DEFAULT: usize = 64 * 1024;
const DIRECT_BURST_BYTES_MIN: usize = 4 * 1024;
const DIRECT_BURST_BYTES_MAX: usize = 256 * 1024;
const DIRECT_BURST_PRE_PUT_DELAY_MS_DEFAULT: u64 = 120;
const TRANSPORT_RESET_RECOVERY_POLL_SEC: f64 = 0.2;
const TRANSPORT_RESET_BACKOFF_MS_STEP: u64 = 75;
const TRANSPORT_RESET_BACKOFF_MS_MAX: u64 = 600;
const TRANSPORT_RESET_FAST_RETRY_STREAK_DEFAULT: u32 = 2;
const TRANSPORT_RESET_CHUNK_FALLBACK_STREAK_DEFAULT: u32 = 2;
const TRANSPORT_RESET_CHUNK_FALLBACK_MARKER: &str = "transport_reset_chunk_fallback_trigger";
const RETRY_BACKOFF_MS_STEP: u64 = 250;
const RETRY_BACKOFF_MS_MAX: u64 = 3000;

pub(crate) use request::{health_timeout_s, make_client, request_raw};
pub(crate) use retry::{
    is_transport_reset_chunk_fallback_error, request_sd_busy_aware, request_sd_busy_aware_timed,
};

#[allow(unused_imports)]
use config::{
    append_host_diag_line, should_use_direct_burst_sender, upload_direct_burst_bytes,
    upload_direct_burst_mode_active, upload_direct_burst_sender_enabled,
    upload_force_connection_close, upload_pre_put_delay_ms, upload_send_diag_deep_enabled,
    upload_send_diag_enabled, upload_tcp_nodelay_enabled,
    upload_transport_reset_chunk_fallback_enabled,
    upload_transport_reset_chunk_fallback_streak_limit, upload_transport_reset_fast_retry_enabled,
    upload_transport_reset_fast_retry_streak_limit,
};
#[allow(unused_imports)]
use direct_stream::request_raw_timed_direct_stream;
#[allow(unused_imports)]
use errors::{
    elapsed_ms_u32, format_error_chain, inspect_io_error_flags, inspect_reqwest_error_flags,
    is_transport_reset_error,
};
