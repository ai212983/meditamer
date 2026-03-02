use std::{
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use reqwest::{blocking::Client, Method};

use crate::env_utils;

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

fn wait_network_recovery(client: &Client, host: &str, port: u16, timeout_sec: f64) -> bool {
    let poll_sec =
        env_utils::parse_env_f64("HOSTCTL_UPLOAD_NET_RECOVERY_POLL_SEC", 0.8).unwrap_or(0.8);
    let deadline = Instant::now()
        + Duration::from_secs_f64(
            env_utils::parse_env_f64("HOSTCTL_UPLOAD_NET_RECOVERY_TIMEOUT_SEC", 45.0)
                .unwrap_or(45.0)
                .min(timeout_sec.max(0.5)),
        );
    while Instant::now() < deadline {
        let url = format!("http://{host}:{port}/health");
        if request_raw(
            client,
            Method::GET,
            &url,
            None,
            None,
            health_timeout_s(timeout_sec),
        )
        .is_ok()
        {
            return true;
        }
        thread::sleep(Duration::from_secs_f64(poll_sec.max(0.05)));
    }
    false
}

#[allow(clippy::too_many_arguments)]
pub(super) fn request_sd_busy_aware(
    client: &Client,
    method: Method,
    url: &str,
    body: Option<Vec<u8>>,
    token: Option<&str>,
    host: &str,
    port: u16,
    timeout_sec: f64,
) -> Result<Vec<u8>> {
    let max_busy_s = env_utils::parse_env_f64("HOSTCTL_UPLOAD_SD_BUSY_TOTAL_RETRY_SEC", 180.0)?;
    let deadline = Instant::now() + Duration::from_secs_f64(max_busy_s.max(1.0));

    let mut attempt = 0usize;
    loop {
        attempt += 1;
        match request_raw(
            client,
            method.clone(),
            url,
            body.clone(),
            token,
            timeout_sec,
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
                    let abort_url = format!("http://{host}:{port}/upload_abort");
                    let _ = request_raw(
                        client,
                        Method::POST,
                        &abort_url,
                        Some(Vec::new()),
                        token,
                        timeout_sec,
                    );
                }

                let _ = wait_network_recovery(client, host, port, timeout_sec);
                thread::sleep(Duration::from_millis((attempt as u64 * 250).min(3000)));
            }
        }
    }
}
