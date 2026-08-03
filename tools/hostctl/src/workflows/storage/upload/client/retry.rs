use std::{
    thread,
    time::{Duration, Instant},
};

use anyhow::Result;
use reqwest::{blocking::Client, Method};

use super::{
    config::{
        append_host_diag_line, upload_pre_put_delay_ms, upload_send_diag_enabled,
        upload_transport_reset_chunk_fallback_enabled,
        upload_transport_reset_chunk_fallback_streak_limit,
        upload_transport_reset_fast_retry_enabled, upload_transport_reset_fast_retry_streak_limit,
    },
    errors::{
        format_error_chain, inspect_io_error_flags, inspect_reqwest_error_flags,
        is_transport_reset_error, IoErrorFlags, ReqwestErrorFlags,
    },
    request::{health_timeout_s, make_client, request_raw, request_raw_timed},
    RequestContext, TimedResponse, RETRY_BACKOFF_MS_MAX, RETRY_BACKOFF_MS_STEP,
    TRANSPORT_RESET_BACKOFF_MS_MAX, TRANSPORT_RESET_BACKOFF_MS_STEP,
    TRANSPORT_RESET_CHUNK_FALLBACK_MARKER, TRANSPORT_RESET_RECOVERY_POLL_SEC,
};

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

pub(crate) fn is_transport_reset_chunk_fallback_error(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .to_string()
            .contains(TRANSPORT_RESET_CHUNK_FALLBACK_MARKER)
    })
}

pub(crate) fn request_sd_busy_aware(
    client: &Client,
    method: Method,
    url: &str,
    body: Option<Vec<u8>>,
    ctx: RequestContext<'_>,
) -> Result<Vec<u8>> {
    let timed = request_sd_busy_aware_timed(client, method, url, body, ctx)?;
    Ok(timed.body)
}

struct RetrySettings {
    deadline: Instant,
    is_direct_upload_put: bool,
    emit_send_diag: bool,
    pre_put_delay_ms: u64,
    transport_reset_fast_retry: bool,
    transport_reset_fast_retry_streak_limit: u32,
    transport_reset_chunk_fallback: bool,
    transport_reset_chunk_fallback_streak_limit: u32,
}

impl RetrySettings {
    fn load(method: &Method, url: &str, ctx: RequestContext<'_>) -> Result<Self> {
        let is_direct_upload_put = method == Method::PUT && url.contains("/upload?");
        Ok(Self {
            deadline: Instant::now()
                + Duration::from_secs_f64(ctx.retry_policy.sd_busy_total_retry_sec.max(1.0)),
            is_direct_upload_put,
            emit_send_diag: upload_send_diag_enabled() && is_direct_upload_put,
            pre_put_delay_ms: if is_direct_upload_put {
                upload_pre_put_delay_ms()?
            } else {
                0
            },
            transport_reset_fast_retry: upload_transport_reset_fast_retry_enabled()?,
            transport_reset_fast_retry_streak_limit:
                upload_transport_reset_fast_retry_streak_limit()?,
            transport_reset_chunk_fallback: upload_transport_reset_chunk_fallback_enabled()?,
            transport_reset_chunk_fallback_streak_limit:
                upload_transport_reset_chunk_fallback_streak_limit()?,
        })
    }
}

struct RequestFailure {
    message: String,
    reqwest: ReqwestErrorFlags,
    io: IoErrorFlags,
    is_sd_busy: bool,
    is_timeout: bool,
    is_transport_reset: bool,
    is_transient: bool,
}

impl RequestFailure {
    fn classify(err: &anyhow::Error, deadline: Instant) -> (Self, bool) {
        let message = err.to_string();
        let message_lower = message.to_lowercase();
        let reqwest = inspect_reqwest_error_flags(err);
        let io = inspect_io_error_flags(err);
        let can_retry = Instant::now() < deadline;
        let is_transport_reset = is_transport_reset_error(&message_lower)
            || io.connection_reset
            || io.broken_pipe
            || io.connection_aborted;
        (
            Self {
                is_sd_busy: message.contains("409") && message_lower.contains("sd busy"),
                is_timeout: message.contains("408")
                    || message_lower.contains("timed out")
                    || reqwest.timeout,
                is_transport_reset,
                is_transient: message_lower.contains("connection")
                    || message_lower.contains("connect")
                    || message_lower.contains("timeout")
                    || message_lower.contains("send failed")
                    || message_lower.contains("error sending request")
                    || reqwest.transient()
                    || io.transient(),
                message,
                reqwest,
                io,
            },
            can_retry,
        )
    }

    fn is_retryable(&self) -> bool {
        self.is_sd_busy || self.is_timeout || self.is_transient
    }
}

struct RetryDecision {
    skip_health_recovery: bool,
    trigger_chunk_fallback: bool,
}

impl RetryDecision {
    fn for_failure(
        settings: &RetrySettings,
        failure: &RequestFailure,
        transport_reset_streak: u32,
    ) -> Self {
        Self {
            skip_health_recovery: failure.is_transport_reset
                && settings.transport_reset_fast_retry
                && transport_reset_streak <= settings.transport_reset_fast_retry_streak_limit,
            trigger_chunk_fallback: settings.is_direct_upload_put
                && settings.transport_reset_chunk_fallback
                && failure.is_transport_reset
                && transport_reset_streak > settings.transport_reset_chunk_fallback_streak_limit,
        }
    }
}

struct RetryState {
    attempt: usize,
    transport_reset_streak: u32,
    retry_client: Option<Client>,
}

impl RetryState {
    fn new() -> Self {
        Self {
            attempt: 0,
            transport_reset_streak: 0,
            retry_client: None,
        }
    }

    fn begin_attempt(&mut self, pre_put_delay_ms: u64) {
        self.attempt += 1;
        if pre_put_delay_ms > 0 {
            thread::sleep(Duration::from_millis(pre_put_delay_ms));
        }
    }

    fn active_client<'a>(&'a self, default_client: &'a Client) -> &'a Client {
        self.retry_client.as_ref().unwrap_or(default_client)
    }

    fn record_failure(&mut self, failure: &RequestFailure) {
        self.transport_reset_streak = if failure.is_transport_reset {
            self.transport_reset_streak.saturating_add(1)
        } else {
            0
        };
    }
}

fn emit_retry_diagnostic(
    err: &anyhow::Error,
    settings: &RetrySettings,
    state: &RetryState,
    failure: &RequestFailure,
    decision: &RetryDecision,
) {
    let compact_msg = failure.message.replace(['\n', '\r'], " ");
    let compact_chain = format_error_chain(err, 6);
    let line = format!(
        "host_upload_retry_diag: attempt={} pre_put_delay_ms={} sd_busy={} timeout={} transport_reset={} transport_reset_streak={} skip_transport_reset_health_recovery={} transport_reset_chunk_fallback={} transient={} reqwest_seen={} reqwest_timeout={} reqwest_connect={} reqwest_request={} reqwest_body={} io_conn_reset={} io_broken_pipe={} io_conn_aborted={} io_timed_out={} io_conn_refused={} io_not_connected={} err={} err_chain={}",
        state.attempt,
        settings.pre_put_delay_ms,
        failure.is_sd_busy as u8,
        failure.is_timeout as u8,
        failure.is_transport_reset as u8,
        state.transport_reset_streak,
        decision.skip_health_recovery as u8,
        decision.trigger_chunk_fallback as u8,
        failure.is_transient as u8,
        failure.reqwest.seen as u8,
        failure.reqwest.timeout as u8,
        failure.reqwest.connect as u8,
        failure.reqwest.request as u8,
        failure.reqwest.body as u8,
        failure.io.connection_reset as u8,
        failure.io.broken_pipe as u8,
        failure.io.connection_aborted as u8,
        failure.io.timed_out as u8,
        failure.io.connection_refused as u8,
        failure.io.not_connected as u8,
        compact_msg,
        compact_chain,
    );
    println!("{line}");
    append_host_diag_line(&line);
}

fn abort_busy_upload(client: &Client, ctx: RequestContext<'_>) {
    let abort_url = format!("http://{}:{}/upload_abort", ctx.host, ctx.port);
    let _ = request_raw(
        client,
        Method::POST,
        &abort_url,
        Some(Vec::new()),
        ctx.token,
        ctx.timeout_sec,
    );
}

fn recover_before_retry(
    client: &Client,
    ctx: RequestContext<'_>,
    state: &mut RetryState,
    failure: &RequestFailure,
    decision: &RetryDecision,
) -> Result<()> {
    let default_backoff_ms =
        (state.attempt as u64 * RETRY_BACKOFF_MS_STEP).min(RETRY_BACKOFF_MS_MAX);
    let mut retry_backoff_ms = default_backoff_ms;
    let mut health_successes = ctx
        .retry_policy
        .net_recovery_consecutive_health_successes
        .max(1);
    let mut recovery_poll_override = None;

    if failure.is_transport_reset {
        state.retry_client = Some(make_client(ctx.timeout_sec)?);
        health_successes = 1;
        recovery_poll_override = Some(TRANSPORT_RESET_RECOVERY_POLL_SEC);
        retry_backoff_ms = (state.attempt as u64 * TRANSPORT_RESET_BACKOFF_MS_STEP)
            .min(TRANSPORT_RESET_BACKOFF_MS_MAX);
    }

    if !decision.skip_health_recovery {
        let recovered = wait_network_recovery(
            state.active_client(client),
            ctx,
            health_successes,
            recovery_poll_override,
        );
        if !recovered {
            retry_backoff_ms = default_backoff_ms;
        }
    }
    thread::sleep(Duration::from_millis(retry_backoff_ms));
    Ok(())
}

pub(crate) fn request_sd_busy_aware_timed(
    client: &Client,
    method: Method,
    url: &str,
    body: Option<Vec<u8>>,
    ctx: RequestContext<'_>,
) -> Result<TimedResponse> {
    let settings = RetrySettings::load(&method, url, ctx)?;
    let mut state = RetryState::new();
    loop {
        state.begin_attempt(settings.pre_put_delay_ms);
        let request_result = request_raw_timed(
            state.active_client(client),
            method.clone(),
            url,
            body.clone(),
            ctx.token,
            ctx.timeout_sec,
        );
        match request_result {
            Ok(mut response) => {
                response.attempts = state.attempt;
                return Ok(response);
            }
            Err(err) => {
                let (failure, can_retry) = RequestFailure::classify(&err, settings.deadline);
                state.record_failure(&failure);
                let decision =
                    RetryDecision::for_failure(&settings, &failure, state.transport_reset_streak);

                if settings.emit_send_diag {
                    emit_retry_diagnostic(&err, &settings, &state, &failure, &decision);
                }

                if decision.trigger_chunk_fallback {
                    return Err(err.context(format!(
                        "{TRANSPORT_RESET_CHUNK_FALLBACK_MARKER}: streak={} limit={} attempt={}",
                        state.transport_reset_streak,
                        settings.transport_reset_chunk_fallback_streak_limit,
                        state.attempt
                    )));
                }

                if !can_retry || !failure.is_retryable() {
                    return Err(err);
                }

                if failure.is_sd_busy {
                    abort_busy_upload(state.active_client(client), ctx);
                }

                recover_before_retry(client, ctx, &mut state, &failure, &decision)?;
            }
        }
    }
}
