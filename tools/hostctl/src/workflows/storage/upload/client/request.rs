use std::{
    io::{Cursor, Read},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use reqwest::{blocking::Client, Method};

use super::{
    config::{
        should_use_direct_burst_sender, upload_direct_burst_bytes,
        upload_direct_burst_mode_active, upload_force_connection_close,
        upload_send_diag_deep_enabled, upload_send_diag_enabled, upload_tcp_nodelay_enabled,
    },
    direct_stream::request_raw_timed_direct_stream,
    errors::elapsed_ms_u32,
    RequestBodyCadence,
    RequestTiming,
    TimedResponse,
};

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
            if gap_ms >= super::BODY_READ_GAP_OVER_10MS {
                cadence.read_gap_over_10ms = cadence.read_gap_over_10ms.saturating_add(1);
            }
            if gap_ms >= super::BODY_READ_GAP_OVER_50MS {
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

pub(crate) fn make_client(timeout_sec: f64) -> Result<Client> {
    let connect_timeout_s =
        crate::env_utils::parse_env_f64("HOSTCTL_UPLOAD_CONNECT_TIMEOUT_SEC", 4.0)?;
    let disable_pool = crate::env_utils::parse_env_bool01("HOSTCTL_UPLOAD_DISABLE_POOL", false)?
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

pub(crate) fn request_raw(
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

pub(crate) fn request_raw_timed(
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
        let use_reader_body =
            burst_read_cap.is_some() || (upload_send_diag_enabled() && upload_send_diag_deep_enabled());
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

pub(crate) fn health_timeout_s(timeout_sec: f64) -> f64 {
    timeout_sec.clamp(0.5, 5.0)
}
