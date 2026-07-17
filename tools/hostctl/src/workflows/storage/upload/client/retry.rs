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
        is_transport_reset_error,
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

pub(crate) fn request_sd_busy_aware_timed(
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
    let transport_reset_fast_retry_streak_limit = upload_transport_reset_fast_retry_streak_limit()?;
    let transport_reset_chunk_fallback = upload_transport_reset_chunk_fallback_enabled()?;
    let transport_reset_chunk_fallback_streak_limit =
        upload_transport_reset_chunk_fallback_streak_limit()?;

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
                let trigger_transport_reset_chunk_fallback = is_direct_upload_put
                    && transport_reset_chunk_fallback
                    && is_transport_reset
                    && transport_reset_streak > transport_reset_chunk_fallback_streak_limit;

                if emit_send_diag {
                    let compact_msg = msg.replace(['\n', '\r'], " ");
                    let compact_chain = format_error_chain(&err, 6);
                    let line = format!(
                        "host_upload_retry_diag: attempt={} pre_put_delay_ms={} sd_busy={} timeout={} transport_reset={} transport_reset_streak={} skip_transport_reset_health_recovery={} transport_reset_chunk_fallback={} transient={} reqwest_seen={} reqwest_timeout={} reqwest_connect={} reqwest_request={} reqwest_body={} io_conn_reset={} io_broken_pipe={} io_conn_aborted={} io_timed_out={} io_conn_refused={} io_not_connected={} err={} err_chain={}",
                        attempt,
                        pre_put_delay_ms,
                        if is_sd_busy { 1 } else { 0 },
                        if is_timeout { 1 } else { 0 },
                        if is_transport_reset { 1 } else { 0 },
                        transport_reset_streak,
                        if skip_transport_reset_health_recovery { 1 } else { 0 },
                        if trigger_transport_reset_chunk_fallback {
                            1
                        } else {
                            0
                        },
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

                if trigger_transport_reset_chunk_fallback {
                    return Err(err.context(format!(
                        "{TRANSPORT_RESET_CHUNK_FALLBACK_MARKER}: streak={} limit={} attempt={}",
                        transport_reset_streak,
                        transport_reset_chunk_fallback_streak_limit,
                        attempt
                    )));
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
