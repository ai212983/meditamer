use std::{
    fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Context, Result};
use reqwest::{blocking::Client, Method};
use urlencoding::encode;

use crate::{env_utils, logging::Logger};

#[derive(Clone, Debug)]
pub struct UploadOptions {
    pub host: String,
    pub port: u16,
    pub src: Option<PathBuf>,
    pub dst: String,
    pub timeout_sec: f64,
    pub rm: Vec<String>,
    pub token: Option<String>,
}

fn make_client(timeout_sec: f64) -> Result<Client> {
    let connect_timeout_s = env_utils::parse_env_f64("HOSTCTL_UPLOAD_CONNECT_TIMEOUT_SEC", 4.0)?;
    let timeout = Duration::from_secs_f64(timeout_sec.max(0.1));
    let connect_timeout = Duration::from_secs_f64(connect_timeout_s.max(0.1));
    Ok(Client::builder()
        .no_proxy()
        .timeout(timeout)
        .connect_timeout(connect_timeout)
        .build()?)
}

fn remote_join(root: &str, rel: &Path) -> String {
    let mut root = root.to_string();
    if !root.starts_with('/') {
        root.insert(0, '/');
    }
    root = root.trim_end_matches('/').to_string();
    let mut path = root;
    for part in rel
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .filter(|p| !p.is_empty() && p != ".")
    {
        path.push('/');
        path.push_str(&part);
    }
    if path.is_empty() {
        "/".to_string()
    } else {
        path
    }
}

fn request_raw(
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

fn health_timeout_s(timeout_sec: f64) -> f64 {
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
fn request_sd_busy_aware(
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

fn mkdir_p(
    client: &Client,
    host: &str,
    port: u16,
    timeout_sec: f64,
    path: &str,
    token: Option<&str>,
) -> Result<()> {
    let mut current = String::new();
    for part in path.split('/').filter(|part| !part.is_empty()) {
        current.push('/');
        current.push_str(part);
        let url = format!(
            "http://{host}:{port}/mkdir?path={}",
            encode(&current).replace("%2F", "/")
        );
        let _ = request_sd_busy_aware(
            client,
            Method::POST,
            &url,
            Some(Vec::new()),
            token,
            host,
            port,
            timeout_sec,
        )?;
    }
    Ok(())
}

fn rm_path(
    client: &Client,
    host: &str,
    port: u16,
    timeout_sec: f64,
    path: &str,
    token: Option<&str>,
) -> Result<()> {
    let url = format!(
        "http://{host}:{port}/rm?path={}",
        encode(path).replace("%2F", "/")
    );
    let _ = request_sd_busy_aware(
        client,
        Method::DELETE,
        &url,
        Some(Vec::new()),
        token,
        host,
        port,
        timeout_sec,
    )?;
    Ok(())
}

fn upload_file(
    client: &Client,
    host: &str,
    port: u16,
    timeout_sec: f64,
    local_path: &Path,
    remote_path: &str,
    token: Option<&str>,
) -> Result<()> {
    let data = fs::read(local_path)
        .with_context(|| format!("failed reading upload file {}", local_path.display()))?;

    let upload_url = format!(
        "http://{host}:{port}/upload?path={}",
        encode(remote_path).replace("%2F", "/")
    );
    let put_result = request_sd_busy_aware(
        client,
        Method::PUT,
        &upload_url,
        Some(data.clone()),
        token,
        host,
        port,
        timeout_sec,
    );
    if put_result.is_ok() {
        return Ok(());
    }

    let abort_url = format!("http://{host}:{port}/upload_abort");
    let _ = request_raw(
        client,
        Method::POST,
        &abort_url,
        Some(Vec::new()),
        token,
        timeout_sec,
    );

    let begin_url = format!(
        "http://{host}:{port}/upload_begin?path={}&size={}",
        encode(remote_path).replace("%2F", "/"),
        data.len()
    );
    let _ = request_sd_busy_aware(
        client,
        Method::POST,
        &begin_url,
        Some(Vec::new()),
        token,
        host,
        port,
        timeout_sec,
    )?;

    let chunk_size = env_utils::parse_env_u64("HOSTCTL_UPLOAD_CHUNK_SIZE", 8192)? as usize;
    for chunk in data.chunks(chunk_size.max(1)) {
        let chunk_url = format!("http://{host}:{port}/upload_chunk");
        let _ = request_sd_busy_aware(
            client,
            Method::PUT,
            &chunk_url,
            Some(chunk.to_vec()),
            token,
            host,
            port,
            timeout_sec,
        )?;
    }

    let commit_url = format!("http://{host}:{port}/upload_commit");
    if let Err(err) = request_sd_busy_aware(
        client,
        Method::POST,
        &commit_url,
        Some(Vec::new()),
        token,
        host,
        port,
        timeout_sec,
    ) {
        let _ = request_raw(
            client,
            Method::POST,
            &abort_url,
            Some(Vec::new()),
            token,
            timeout_sec,
        );
        return Err(err);
    }

    Ok(())
}

pub fn run_upload(logger: &mut Logger, opts: UploadOptions) -> Result<()> {
    let client = make_client(opts.timeout_sec)?;

    if opts.src.is_none() && opts.rm.is_empty() {
        return Err(anyhow!("Nothing to do: provide --src and/or --rm"));
    }

    let health_url = format!("http://{}:{}/health", opts.host, opts.port);
    let mut health_ok = false;
    for _ in 0..20 {
        if request_raw(
            &client,
            Method::GET,
            &health_url,
            None,
            None,
            health_timeout_s(opts.timeout_sec),
        )
        .is_ok()
        {
            health_ok = true;
            break;
        }
        thread::sleep(Duration::from_millis(300));
    }
    if !health_ok {
        return Err(anyhow!("health check failed"));
    }

    for rm in &opts.rm {
        let remote = if rm.starts_with('/') {
            rm.clone()
        } else {
            remote_join(&opts.dst, Path::new(rm))
        };
        logger.info(format!("[delete] {remote}"));
        rm_path(
            &client,
            &opts.host,
            opts.port,
            opts.timeout_sec,
            &remote,
            opts.token.as_deref(),
        )?;
    }

    let Some(src) = opts.src else {
        logger.info("Delete complete.");
        return Ok(());
    };

    if !src.exists() {
        return Err(anyhow!("Source path does not exist: {}", src.display()));
    }

    let skip_mkdir = env_utils::parse_env_bool01("HOSTCTL_UPLOAD_SKIP_MKDIR", false)?;

    if src.is_file() {
        let remote_file = remote_join(&opts.dst, Path::new(src.file_name().unwrap_or_default()));
        let remote_dir = Path::new(&remote_file)
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "/".to_string());

        if skip_mkdir {
            logger.info(format!("[mkdir -p] skipped ({remote_dir})"));
        } else {
            logger.info(format!("[mkdir -p] {remote_dir}"));
            mkdir_p(
                &client,
                &opts.host,
                opts.port,
                opts.timeout_sec,
                &remote_dir,
                opts.token.as_deref(),
            )?;
        }

        logger.info(format!("[upload] {} -> {remote_file}", src.display()));
        upload_file(
            &client,
            &opts.host,
            opts.port,
            opts.timeout_sec,
            &src,
            &remote_file,
            opts.token.as_deref(),
        )?;

        logger.info("Upload complete.");
        return Ok(());
    }

    let mut dirs = vec![PathBuf::from(".")];
    let mut files = Vec::new();
    for entry in walkdir_sorted(&src)? {
        if entry.is_dir() {
            let rel = entry
                .strip_prefix(&src)
                .unwrap_or(entry.as_path())
                .to_path_buf();
            dirs.push(rel);
        } else if entry.is_file() {
            let rel = entry
                .strip_prefix(&src)
                .unwrap_or(entry.as_path())
                .to_path_buf();
            files.push((rel, entry.to_path_buf()));
        }
    }

    dirs.sort();
    dirs.dedup();

    for rel_dir in dirs {
        let remote_dir = remote_join(&opts.dst, &rel_dir);
        if skip_mkdir {
            logger.info(format!("[mkdir -p] skipped ({remote_dir})"));
            continue;
        }
        logger.info(format!("[mkdir -p] {remote_dir}"));
        mkdir_p(
            &client,
            &opts.host,
            opts.port,
            opts.timeout_sec,
            &remote_dir,
            opts.token.as_deref(),
        )?;
    }

    for (rel_file, local_file) in files {
        let remote_file = remote_join(&opts.dst, &rel_file);
        logger.info(format!(
            "[upload] {} -> {remote_file}",
            local_file.display()
        ));
        upload_file(
            &client,
            &opts.host,
            opts.port,
            opts.timeout_sec,
            &local_file,
            &remote_file,
            opts.token.as_deref(),
        )?;
    }

    logger.info("Upload complete.");
    Ok(())
}

fn walkdir_sorted(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    fn walk(path: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
        out.push(path.to_path_buf());
        if path.is_dir() {
            let mut entries = fs::read_dir(path)?
                .flatten()
                .map(|e| e.path())
                .collect::<Vec<PathBuf>>();
            entries.sort();
            for entry in entries {
                walk(&entry, out)?;
            }
        }
        Ok(())
    }
    walk(root, &mut out)?;
    Ok(out)
}

pub fn upload_file_direct(
    logger: &mut Logger,
    host: &str,
    port: u16,
    timeout_sec: f64,
    src: &Path,
    dst_root: &str,
    token: Option<&str>,
) -> Result<()> {
    let opts = UploadOptions {
        host: host.to_string(),
        port,
        src: Some(src.to_path_buf()),
        dst: dst_root.to_string(),
        timeout_sec,
        rm: vec![],
        token: token.map(|s| s.to_string()),
    };
    run_upload(logger, opts)
}
