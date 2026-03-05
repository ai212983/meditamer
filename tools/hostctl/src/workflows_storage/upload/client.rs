use std::{
    fs,
    io::{Cursor, ErrorKind, Read, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use reqwest::{blocking::Client, Method};

use super::{direct_stream::request_raw_timed_direct_stream, UploadRetryPolicy};
use crate::env_utils;

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
const RETRY_BACKOFF_MS_STEP: u64 = 250;
const RETRY_BACKOFF_MS_MAX: u64 = 3000;

fn upload_send_diag_enabled() -> bool {
    env_utils::parse_env_bool01("HOSTCTL_UPLOAD_SEND_DIAG", false).unwrap_or(false)
}

fn upload_send_diag_deep_enabled() -> bool {
    env_utils::parse_env_bool01("HOSTCTL_UPLOAD_SEND_DIAG_DEEP", false).unwrap_or(false)
}

fn upload_force_connection_close() -> bool {
    env_utils::parse_env_bool01("HOSTCTL_UPLOAD_FORCE_CONN_CLOSE", false).unwrap_or(false)
}

fn upload_direct_burst_sender_enabled() -> Result<bool> {
    env_utils::parse_env_bool01("HOSTCTL_UPLOAD_DIRECT_BURST_SENDER", false)
}

fn upload_direct_burst_mode_active() -> bool {
    upload_direct_burst_sender_enabled().unwrap_or(false)
}

fn upload_direct_burst_bytes() -> Result<usize> {
    let configured = env_utils::parse_env_u64(
        "HOSTCTL_UPLOAD_DIRECT_BURST_BYTES",
        DIRECT_BURST_BYTES_DEFAULT as u64,
    )? as usize;
    Ok(configured.clamp(DIRECT_BURST_BYTES_MIN, DIRECT_BURST_BYTES_MAX))
}

fn upload_tcp_nodelay_enabled() -> Result<bool> {
    env_utils::parse_env_bool01("HOSTCTL_UPLOAD_TCP_NODELAY", true)
}

fn upload_pre_put_delay_ms() -> Result<u64> {
    let default = if upload_direct_burst_mode_active() {
        DIRECT_BURST_PRE_PUT_DELAY_MS_DEFAULT
    } else {
        0
    };
    env_utils::parse_env_u64("HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS", default)
}

fn upload_transport_reset_fast_retry_enabled() -> Result<bool> {
    env_utils::parse_env_bool01("HOSTCTL_UPLOAD_TRANSPORT_RESET_FAST_RETRY", true)
}

fn upload_transport_reset_fast_retry_streak_limit() -> Result<u32> {
    Ok(env_utils::parse_env_u32(
        "HOSTCTL_UPLOAD_TRANSPORT_RESET_FAST_RETRY_STREAK",
        TRANSPORT_RESET_FAST_RETRY_STREAK_DEFAULT,
    )?
    .max(1))
}

fn host_diag_log_path() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("HOSTCTL_UPLOAD_SEND_DIAG_PATH") {
        if !path.trim().is_empty() {
            return Some(PathBuf::from(path));
        }
    }
    std::env::var("HOSTCTL_NET_LOG_PATH")
        .ok()
        .filter(|path| !path.trim().is_empty())
        .map(|path| PathBuf::from(format!("{path}.hostdiag")))
}

fn append_host_diag_line(line: &str) {
    if let Some(path) = host_diag_log_path() {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(file, "{line}");
            let _ = file.flush();
        }
    }
}

fn elapsed_ms_u32(started_at: Instant) -> u32 {
    let elapsed = started_at.elapsed().as_millis();
    if elapsed > u32::MAX as u128 {
        u32::MAX
    } else {
        elapsed as u32
    }
}

fn compact_diag_text(value: &str) -> String {
    value.replace(['\n', '\r'], " ")
}

fn should_use_direct_burst_sender(method: &Method, url: &str) -> Result<bool> {
    if !upload_direct_burst_sender_enabled()? {
        return Ok(false);
    }
    Ok(*method == Method::PUT && url.contains("/upload?"))
}

#[derive(Clone, Copy, Debug, Default)]
struct ReqwestErrorFlags {
    seen: bool,
    timeout: bool,
    connect: bool,
    request: bool,
    body: bool,
}

impl ReqwestErrorFlags {
    fn transient(self) -> bool {
        self.timeout || self.connect || self.request || self.body
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct IoErrorFlags {
    connection_reset: bool,
    broken_pipe: bool,
    connection_aborted: bool,
    timed_out: bool,
    connection_refused: bool,
    not_connected: bool,
}

impl IoErrorFlags {
    fn transient(self) -> bool {
        self.connection_reset
            || self.broken_pipe
            || self.connection_aborted
            || self.timed_out
            || self.connection_refused
            || self.not_connected
    }
}

fn inspect_reqwest_error_flags(err: &anyhow::Error) -> ReqwestErrorFlags {
    let mut flags = ReqwestErrorFlags::default();
    for cause in err.chain() {
        if let Some(req_err) = cause.downcast_ref::<reqwest::Error>() {
            flags.seen = true;
            flags.timeout |= req_err.is_timeout();
            flags.connect |= req_err.is_connect();
            flags.request |= req_err.is_request();
            flags.body |= req_err.is_body();
        }
    }
    flags
}

fn inspect_io_error_flags(err: &anyhow::Error) -> IoErrorFlags {
    let mut flags = IoErrorFlags::default();
    for cause in err.chain() {
        if let Some(io_err) = cause.downcast_ref::<std::io::Error>() {
            match io_err.kind() {
                ErrorKind::ConnectionReset => flags.connection_reset = true,
                ErrorKind::BrokenPipe => flags.broken_pipe = true,
                ErrorKind::ConnectionAborted => flags.connection_aborted = true,
                ErrorKind::TimedOut => flags.timed_out = true,
                ErrorKind::ConnectionRefused => flags.connection_refused = true,
                ErrorKind::NotConnected => flags.not_connected = true,
                _ => {}
            }
        }
    }
    flags
}

fn format_error_chain(err: &anyhow::Error, max_causes: usize) -> String {
    let limit = max_causes.max(1);
    let mut out = String::new();
    for (idx, cause) in err.chain().enumerate() {
        if idx >= limit {
            out.push_str(" <- ...");
            break;
        }
        if idx > 0 {
            out.push_str(" <- ");
        }
        out.push_str(&compact_diag_text(&cause.to_string()));
    }
    out
}

struct InstrumentedBodyReader {
    cursor: Cursor<Vec<u8>>,
    cadence: Arc<Mutex<RequestBodyCadence>>,
    max_read_bytes: Option<usize>,
    last_read_completed_at: Option<Instant>,
}

impl InstrumentedBodyReader {
    fn new_with_max_read(
        body: Vec<u8>,
        cadence: Arc<Mutex<RequestBodyCadence>>,
        max_read_bytes: impl Into<Option<usize>>,
    ) -> Self {
        let max_read_bytes = max_read_bytes.into().map(|bytes| bytes.max(1));
        Self {
            cursor: Cursor::new(body),
            cadence,
            max_read_bytes,
            last_read_completed_at: None,
        }
    }
}

impl Read for InstrumentedBodyReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut cadence = self
            .cadence
            .lock()
            .map_err(|_| std::io::Error::other("body cadence lock poisoned"))?;
        if let Some(previous_completed_at) = self.last_read_completed_at {
            let gap_ms = elapsed_ms_u32(previous_completed_at);
            cadence.read_gap_ms_total = cadence.read_gap_ms_total.saturating_add(gap_ms);
            cadence.read_gap_ms_max = cadence.read_gap_ms_max.max(gap_ms);
            if gap_ms >= BODY_READ_GAP_OVER_10MS {
                cadence.read_gap_over_10ms = cadence.read_gap_over_10ms.saturating_add(1);
            }
            if gap_ms >= BODY_READ_GAP_OVER_50MS {
                cadence.read_gap_over_50ms = cadence.read_gap_over_50ms.saturating_add(1);
            }
        }
        let read_len = self
            .max_read_bytes
            .map(|max| max.min(buf.len()))
            .unwrap_or(buf.len());
        let n = self.cursor.read(&mut buf[..read_len])?;
        if n > 0 {
            cadence.read_calls = cadence.read_calls.saturating_add(1);
            cadence.read_bytes = cadence.read_bytes.saturating_add(n);
            if n < read_len {
                cadence.short_reads = cadence.short_reads.saturating_add(1);
            }
        }
        drop(cadence);
        self.last_read_completed_at = Some(Instant::now());
        Ok(n)
    }
}

pub(super) fn make_client(timeout_sec: f64) -> Result<Client> {
    let connect_timeout_s = env_utils::parse_env_f64("HOSTCTL_UPLOAD_CONNECT_TIMEOUT_SEC", 4.0)?;
    let disable_pool = env_utils::parse_env_bool01("HOSTCTL_UPLOAD_DISABLE_POOL", false)?
        || upload_direct_burst_mode_active();
    let tcp_nodelay = upload_tcp_nodelay_enabled()?;
    let timeout = Duration::from_secs_f64(timeout_sec.max(0.1));
    let connect_timeout = Duration::from_secs_f64(connect_timeout_s.max(0.1));
    let mut builder = Client::builder()
        .no_proxy()
        .tcp_nodelay(tcp_nodelay)
        .timeout(timeout)
        .connect_timeout(connect_timeout);
    if disable_pool {
        builder = builder.pool_max_idle_per_host(0);
    }
    Ok(builder.build()?)
}

pub(super) fn request_raw(
    client: &Client,
    method: Method,
    url: &str,
    body: Option<Vec<u8>>,
    token: Option<&str>,
    timeout_s: f64,
) -> Result<Vec<u8>> {
    let timed = request_raw_timed(client, method, url, body, token, timeout_s)?;
    Ok(timed.body)
}

pub(super) fn request_raw_timed(
    client: &Client,
    method: Method,
    url: &str,
    body: Option<Vec<u8>>,
    token: Option<&str>,
    timeout_s: f64,
) -> Result<TimedResponse> {
    let burst_read_cap = if body.is_some() && should_use_direct_burst_sender(&method, url)? {
        Some(upload_direct_burst_bytes()?)
    } else {
        None
    };
    if let Some(burst_bytes) = burst_read_cap {
        let payload = body.unwrap_or_default();
        let tcp_nodelay = upload_tcp_nodelay_enabled()?;
        let timed = request_raw_timed_direct_stream(
            &method,
            url,
            &payload,
            token,
            timeout_s,
            burst_bytes,
            tcp_nodelay,
        )?;
        return Ok(TimedResponse {
            body: timed.body,
            timing: RequestTiming {
                body_bytes: payload.len(),
                send_ms: timed.send_ms,
                response_read_ms: timed.response_read_ms,
                total_ms: timed.total_ms,
                body_cadence: RequestBodyCadence {
                    read_calls: timed.body_cadence.read_calls,
                    short_reads: timed.body_cadence.short_reads,
                    read_bytes: timed.body_cadence.read_bytes,
                    read_gap_ms_total: timed.body_cadence.read_gap_ms_total,
                    read_gap_ms_max: timed.body_cadence.read_gap_ms_max,
                    read_gap_over_10ms: timed.body_cadence.read_gap_over_10ms,
                    read_gap_over_50ms: timed.body_cadence.read_gap_over_50ms,
                },
            },
            attempts: 1,
        });
    }

    let mut req = client.request(method.clone(), url);
    req = req.timeout(Duration::from_secs_f64(timeout_s.max(0.1)));
    if upload_force_connection_close() || upload_direct_burst_mode_active() {
        req = req.header("Connection", "close");
    }
    if let Some(token) = token {
        req = req.header("x-upload-token", token);
    }
    let body_bytes = body.as_ref().map_or(0usize, |payload| payload.len());
    let cadence = Arc::new(Mutex::new(RequestBodyCadence::default()));
    if let Some(body) = body {
        let use_reader_body = burst_read_cap.is_some()
            || (upload_send_diag_enabled() && upload_send_diag_deep_enabled());
        if use_reader_body {
            let reader = InstrumentedBodyReader::new_with_max_read(
                body,
                Arc::clone(&cadence),
                burst_read_cap,
            );
            req = req
                .header("Content-Length", body_bytes.to_string())
                .body(reqwest::blocking::Body::new(reader));
        } else {
            req = req.body(body);
        }
    }

    let request_started_at = Instant::now();
    let send_started_at = request_started_at;
    let resp = req
        .send()
        .with_context(|| format!("{method} {url} send failed"))?;
    let send_ms = elapsed_ms_u32(send_started_at);
    let status = resp.status();
    let response_started_at = Instant::now();
    let bytes = resp
        .bytes()
        .context("failed reading response body")?
        .to_vec();
    let response_read_ms = elapsed_ms_u32(response_started_at);
    if !status.is_success() {
        return Err(anyhow!(
            "{method} {url} failed: {} {}",
            status,
            String::from_utf8_lossy(&bytes)
        ));
    }

    let body_cadence = cadence.lock().map(|locked| *locked).unwrap_or_default();
    Ok(TimedResponse {
        body: bytes,
        timing: RequestTiming {
            body_bytes,
            send_ms,
            response_read_ms,
            total_ms: elapsed_ms_u32(request_started_at),
            body_cadence,
        },
        attempts: 1,
    })
}

pub(super) fn health_timeout_s(timeout_sec: f64) -> f64 {
    timeout_sec.clamp(0.5, 5.0)
}

fn wait_network_recovery(
    client: &Client,
    ctx: RequestContext<'_>,
    consecutive_health_successes: u32,
    poll_sec_override: Option<f64>,
) -> bool {
    let poll_sec = poll_sec_override
        .unwrap_or(ctx.retry_policy.net_recovery_poll_sec)
        .max(0.05);
    let target_successes = consecutive_health_successes.max(1);
    let deadline = Instant::now()
        + Duration::from_secs_f64(
            ctx.retry_policy
                .net_recovery_timeout_sec
                .min(ctx.timeout_sec.max(0.5)),
        );
    let mut success_streak = 0u32;
    while Instant::now() < deadline {
        let url = format!("http://{}:{}/health", ctx.host, ctx.port);
        if request_raw(
            client,
            Method::GET,
            &url,
            None,
            None,
            health_timeout_s(ctx.timeout_sec),
        )
        .is_ok()
        {
            success_streak = success_streak.saturating_add(1);
            if success_streak >= target_successes {
                return true;
            }
        } else {
            success_streak = 0;
        }
        thread::sleep(Duration::from_secs_f64(poll_sec));
    }
    false
}

fn is_transport_reset_error(msg_lower: &str) -> bool {
    msg_lower.contains("connection reset")
        || msg_lower.contains("send failed")
        || msg_lower.contains("error sending request")
        || msg_lower.contains("connection aborted")
        || msg_lower.contains("broken pipe")
}

pub(super) fn request_sd_busy_aware(
    client: &Client,
    method: Method,
    url: &str,
    body: Option<Vec<u8>>,
    ctx: RequestContext<'_>,
) -> Result<Vec<u8>> {
    let timed = request_sd_busy_aware_timed(client, method, url, body, ctx)?;
    Ok(timed.body)
}

pub(super) fn request_sd_busy_aware_timed(
    client: &Client,
    method: Method,
    url: &str,
    body: Option<Vec<u8>>,
    ctx: RequestContext<'_>,
) -> Result<TimedResponse> {
    let deadline =
        Instant::now() + Duration::from_secs_f64(ctx.retry_policy.sd_busy_total_retry_sec.max(1.0));
    let is_direct_upload_put = method == Method::PUT && url.contains("/upload?");
    let emit_send_diag = upload_send_diag_enabled() && is_direct_upload_put;
    let pre_put_delay_ms = if is_direct_upload_put {
        upload_pre_put_delay_ms()?
    } else {
        0
    };
    let transport_reset_fast_retry = upload_transport_reset_fast_retry_enabled()?;
    let transport_reset_fast_retry_streak_limit =
        upload_transport_reset_fast_retry_streak_limit()?;

    let mut attempt = 0usize;
    let mut transport_reset_streak = 0u32;
    let mut retry_client: Option<Client> = None;
    loop {
        attempt += 1;
        if pre_put_delay_ms > 0 {
            thread::sleep(Duration::from_millis(pre_put_delay_ms));
        }
        let active_client = retry_client.as_ref().unwrap_or(client);
        match request_raw_timed(
            active_client,
            method.clone(),
            url,
            body.clone(),
            ctx.token,
            ctx.timeout_sec,
        ) {
            Ok(mut response) => {
                response.attempts = attempt;
                return Ok(response);
            }
            Err(err) => {
                let msg = err.to_string();
                let msg_lower = msg.to_lowercase();
                let reqwest_flags = inspect_reqwest_error_flags(&err);
                let io_flags = inspect_io_error_flags(&err);
                let can_retry = Instant::now() < deadline;
                let is_sd_busy = msg.contains("409") && msg_lower.contains("sd busy");
                let is_timeout =
                    msg.contains("408") || msg_lower.contains("timed out") || reqwest_flags.timeout;
                let is_transport_reset = is_transport_reset_error(&msg_lower)
                    || io_flags.connection_reset
                    || io_flags.broken_pipe
                    || io_flags.connection_aborted;
                let is_transient = msg_lower.contains("connection")
                    || msg_lower.contains("connect")
                    || msg_lower.contains("timeout")
                    || msg_lower.contains("send failed")
                    || msg_lower.contains("error sending request")
                    || reqwest_flags.transient()
                    || io_flags.transient();
                if is_transport_reset {
                    transport_reset_streak = transport_reset_streak.saturating_add(1);
                } else {
                    transport_reset_streak = 0;
                }
                let skip_transport_reset_health_recovery = is_transport_reset
                    && transport_reset_fast_retry
                    && transport_reset_streak <= transport_reset_fast_retry_streak_limit;

                if emit_send_diag {
                    let compact_msg = compact_diag_text(&msg);
                    let compact_chain = format_error_chain(&err, 6);
                    let line = format!(
                        "host_upload_retry_diag: attempt={} pre_put_delay_ms={} sd_busy={} timeout={} transport_reset={} transport_reset_streak={} skip_transport_reset_health_recovery={} transient={} reqwest_seen={} reqwest_timeout={} reqwest_connect={} reqwest_request={} reqwest_body={} io_conn_reset={} io_broken_pipe={} io_conn_aborted={} io_timed_out={} io_conn_refused={} io_not_connected={} err={} err_chain={}",
                        attempt,
                        pre_put_delay_ms,
                        if is_sd_busy { 1 } else { 0 },
                        if is_timeout { 1 } else { 0 },
                        if is_transport_reset { 1 } else { 0 },
                        transport_reset_streak,
                        if skip_transport_reset_health_recovery { 1 } else { 0 },
                        if is_transient { 1 } else { 0 },
                        if reqwest_flags.seen { 1 } else { 0 },
                        if reqwest_flags.timeout { 1 } else { 0 },
                        if reqwest_flags.connect { 1 } else { 0 },
                        if reqwest_flags.request { 1 } else { 0 },
                        if reqwest_flags.body { 1 } else { 0 },
                        if io_flags.connection_reset { 1 } else { 0 },
                        if io_flags.broken_pipe { 1 } else { 0 },
                        if io_flags.connection_aborted { 1 } else { 0 },
                        if io_flags.timed_out { 1 } else { 0 },
                        if io_flags.connection_refused { 1 } else { 0 },
                        if io_flags.not_connected { 1 } else { 0 },
                        compact_msg,
                        compact_chain,
                    );
                    println!("{line}");
                    append_host_diag_line(&line);
                }

                if !(can_retry && (is_sd_busy || is_timeout || is_transient)) {
                    return Err(err);
                }

                if is_sd_busy {
                    let abort_url = format!("http://{}:{}/upload_abort", ctx.host, ctx.port);
                    let _ = request_raw(
                        active_client,
                        Method::POST,
                        &abort_url,
                        Some(Vec::new()),
                        ctx.token,
                        ctx.timeout_sec,
                    );
                }

                let mut health_successes = ctx
                    .retry_policy
                    .net_recovery_consecutive_health_successes
                    .max(1);
                let mut recovery_poll_override = None;
                let mut skip_recovery_wait = false;
                let default_backoff_ms =
                    (attempt as u64 * RETRY_BACKOFF_MS_STEP).min(RETRY_BACKOFF_MS_MAX);
                let mut retry_backoff_ms = default_backoff_ms;
                if is_transport_reset {
                    retry_client = Some(make_client(ctx.timeout_sec)?);
                    // Under AP contention, transport resets are often brief. Probe with a faster
                    // poll and a shorter backoff to limit single-cycle tail latency inflation.
                    health_successes = 1;
                    recovery_poll_override = Some(TRANSPORT_RESET_RECOVERY_POLL_SEC);
                    retry_backoff_ms = (attempt as u64 * TRANSPORT_RESET_BACKOFF_MS_STEP)
                        .min(TRANSPORT_RESET_BACKOFF_MS_MAX);
                    skip_recovery_wait = skip_transport_reset_health_recovery;
                }

                let recovery_client = retry_client.as_ref().unwrap_or(client);
                if !skip_recovery_wait {
                    let recovered = wait_network_recovery(
                        recovery_client,
                        ctx,
                        health_successes,
                        recovery_poll_override,
                    );
                    if !recovered {
                        retry_backoff_ms = default_backoff_ms;
                    }
                }
                thread::sleep(Duration::from_millis(retry_backoff_ms));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{io::ErrorKind, time::Duration};

    use super::{
        format_error_chain, health_timeout_s, inspect_io_error_flags, inspect_reqwest_error_flags,
        is_transport_reset_error,
    };
    use anyhow::{anyhow, Context};
    use reqwest::blocking::Client;

    #[test]
    fn health_timeout_is_clamped() {
        assert_eq!(health_timeout_s(0.01), 0.5);
        assert_eq!(health_timeout_s(1.25), 1.25);
        assert_eq!(health_timeout_s(999.0), 5.0);
    }

    #[test]
    fn transport_reset_error_detection_matches_send_and_reset_signatures() {
        assert!(is_transport_reset_error(
            "put http://10.0.0.8:8080/upload send failed"
        ));
        assert!(is_transport_reset_error("connection reset by peer"));
        assert!(!is_transport_reset_error("409 sd busy"));
    }

    #[test]
    fn reqwest_and_io_flag_extractors_capture_nested_error_types() {
        let io_err = std::io::Error::new(ErrorKind::ConnectionReset, "reset");
        let err =
            anyhow::Error::from(io_err).context("PUT http://10.0.0.8:8080/upload send failed");
        let io_flags = inspect_io_error_flags(&err);
        assert!(io_flags.connection_reset);
        assert!(io_flags.transient());

        let client = Client::builder()
            .timeout(Duration::from_millis(100))
            .build()
            .expect("client");
        let reqwest_err = client
            .get("http://127.0.0.1:1/health")
            .send()
            .expect_err("request must fail");
        let wrapped = anyhow::Error::from(reqwest_err).context("GET /health send failed");
        let req_flags = inspect_reqwest_error_flags(&wrapped);
        assert!(req_flags.seen);
        assert!(req_flags.transient());
    }

    #[test]
    fn error_chain_formatter_keeps_context_order() {
        let err: anyhow::Error = Err::<(), _>(anyhow!("leaf network error"))
            .context("mid request layer")
            .context("top upload wrapper")
            .expect_err("must fail");
        let chain = format_error_chain(&err, 8);
        assert!(chain.contains("top upload wrapper"));
        assert!(chain.contains("mid request layer"));
        assert!(chain.contains("leaf network error"));
    }
}
