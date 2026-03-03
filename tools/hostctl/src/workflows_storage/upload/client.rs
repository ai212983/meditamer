use std::{
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use reqwest::{blocking::Client, Method};

use super::UploadRetryPolicy;
use crate::env_utils;

#[derive(Clone, Copy)]
pub(super) struct RequestContext<'a> {
    pub(super) host: &'a str,
    pub(super) port: u16,
    pub(super) timeout_sec: f64,
    pub(super) token: Option<&'a str>,
    pub(super) retry_policy: UploadRetryPolicy,
}

pub(super) fn make_client(timeout_sec: f64) -> Result<Client> {
    let connect_timeout_s = env_utils::parse_env_f64("HOSTCTL_UPLOAD_CONNECT_TIMEOUT_SEC", 4.0)?;
    let timeout = Duration::from_secs_f64(timeout_sec.max(0.1));
    let connect_timeout = Duration::from_secs_f64(connect_timeout_s.max(0.1));
    Ok(Client::builder()
        .no_proxy()
        .tcp_nodelay(true)
        .timeout(timeout)
        .connect_timeout(connect_timeout)
        .build()?)
}

pub(super) fn request_raw(
    client: &Client,
    method: Method,
    url: &str,
    body: Option<Vec<u8>>,
    token: Option<&str>,
    timeout_s: f64,
) -> Result<Vec<u8>> {
    let mut req = client.request(method.clone(), url);
    req = req.timeout(Duration::from_secs_f64(timeout_s.max(0.1)));
    if let Some(token) = token {
        req = req.header("x-upload-token", token);
    }
    if let Some(body) = body {
        req = req.body(body);
    }

    let resp = req
        .send()
        .with_context(|| format!("{method} {url} send failed"))?;
    let status = resp.status();
    let bytes = resp
        .bytes()
        .context("failed reading response body")?
        .to_vec();
    if !status.is_success() {
        return Err(anyhow!(
            "{method} {url} failed: {} {}",
            status,
            String::from_utf8_lossy(&bytes)
        ));
    }

    Ok(bytes)
}

pub(super) fn health_timeout_s(timeout_sec: f64) -> f64 {
    timeout_sec.clamp(0.5, 5.0)
}

fn wait_network_recovery(client: &Client, ctx: RequestContext<'_>) -> bool {
    let poll_sec = ctx.retry_policy.net_recovery_poll_sec.max(0.05);
    let deadline = Instant::now()
        + Duration::from_secs_f64(
            ctx.retry_policy
                .net_recovery_timeout_sec
                .min(ctx.timeout_sec.max(0.5)),
        );
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
            return true;
        }
        thread::sleep(Duration::from_secs_f64(poll_sec));
    }
    false
}

pub(super) fn request_sd_busy_aware(
    client: &Client,
    method: Method,
    url: &str,
    body: Option<Vec<u8>>,
    ctx: RequestContext<'_>,
) -> Result<Vec<u8>> {
    let deadline =
        Instant::now() + Duration::from_secs_f64(ctx.retry_policy.sd_busy_total_retry_sec.max(1.0));

    let mut attempt = 0usize;
    loop {
        attempt += 1;
        match request_raw(
            client,
            method.clone(),
            url,
            body.clone(),
            ctx.token,
            ctx.timeout_sec,
        ) {
            Ok(data) => return Ok(data),
            Err(err) => {
                let msg = err.to_string();
                let msg_lower = msg.to_lowercase();
                let can_retry = Instant::now() < deadline;
                let is_sd_busy = msg.contains("409") && msg_lower.contains("sd busy");
                let is_timeout = msg.contains("408") || msg_lower.contains("timed out");
                let is_transient = msg_lower.contains("connection")
                    || msg_lower.contains("connect")
                    || msg_lower.contains("timeout")
                    || msg_lower.contains("send failed")
                    || msg_lower.contains("error sending request");

                if !(can_retry && (is_sd_busy || is_timeout || is_transient)) {
                    return Err(err);
                }

                if is_sd_busy {
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

                let _ = wait_network_recovery(client, ctx);
                thread::sleep(Duration::from_millis((attempt as u64 * 250).min(3000)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::health_timeout_s;

    #[test]
    fn health_timeout_is_clamped() {
        assert_eq!(health_timeout_s(0.01), 0.5);
        assert_eq!(health_timeout_s(1.25), 1.25);
        assert_eq!(health_timeout_s(999.0), 5.0);
    }
}
