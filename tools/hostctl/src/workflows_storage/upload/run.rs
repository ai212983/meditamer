use std::{
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use anyhow::{anyhow, Result};
use reqwest::Method;

use crate::{env_utils, logging::Logger};

use super::{
    client::{health_timeout_s, make_client, request_raw, RequestContext},
    pathing::{remote_join, walkdir_sorted},
    transfer::{mkdir_p, rm_path, stat_path, upload_file},
    UploadOptions, UploadRetryPolicy,
};

#[derive(Clone, Copy)]
struct UploadRunTarget<'a> {
    request_ctx: RequestContext<'a>,
    dst_root: &'a str,
    skip_mkdir: bool,
}

#[derive(Clone, Copy)]
pub struct DirectUploadOptions<'a> {
    pub host: &'a str,
    pub port: u16,
    pub timeout_sec: f64,
    pub src: &'a Path,
    pub dst_root: &'a str,
    pub token: Option<&'a str>,
    pub retry_policy: UploadRetryPolicy,
}

pub fn run_upload(logger: &mut Logger, opts: UploadOptions) -> Result<()> {
    let client = make_client(opts.timeout_sec)?;
    let retry_policy = retry_policy_from_general_env()?;
    let token = opts.token.as_deref();
    let request_ctx = RequestContext {
        host: &opts.host,
        port: opts.port,
        timeout_sec: opts.timeout_sec,
        token,
        retry_policy,
    };

    if opts.src.is_none() && opts.rm.is_empty() {
        return Err(anyhow!("Nothing to do: provide --src and/or --rm"));
    }

    require_health_check(&client, &opts.host, opts.port, opts.timeout_sec, 20, 300)?;

    for rm in &opts.rm {
        let remote = if rm.starts_with('/') {
            rm.clone()
        } else {
            remote_join(&opts.dst, Path::new(rm))
        };
        logger.info(format!("[delete] {remote}"));
        rm_path(&client, request_ctx, &remote)?;
    }

    let Some(src) = opts.src else {
        logger.info("Delete complete.");
        return Ok(());
    };

    if !src.exists() {
        return Err(anyhow!("Source path does not exist: {}", src.display()));
    }

    let skip_mkdir = env_utils::parse_env_bool01("HOSTCTL_UPLOAD_SKIP_MKDIR", false)?;
    let upload_target = UploadRunTarget {
        request_ctx,
        dst_root: &opts.dst,
        skip_mkdir,
    };

    if src.is_file() {
        return run_single_file_upload(logger, &client, upload_target, &src);
    }

    run_directory_upload(logger, &client, upload_target, &src)
}

fn run_single_file_upload(
    logger: &mut Logger,
    client: &reqwest::blocking::Client,
    target: UploadRunTarget<'_>,
    src: &Path,
) -> Result<()> {
    let remote_file = remote_join(
        target.dst_root,
        Path::new(src.file_name().unwrap_or_default()),
    );
    let remote_dir = Path::new(&remote_file)
        .parent()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "/".to_string());

    if target.skip_mkdir {
        logger.info(format!("[mkdir -p] skipped ({remote_dir})"));
    } else {
        logger.info(format!("[mkdir -p] {remote_dir}"));
        mkdir_p(client, target.request_ctx, &remote_dir)?;
    }

    logger.info(format!("[upload] {} -> {remote_file}", src.display()));
    upload_file(client, target.request_ctx, src, &remote_file)?;

    logger.info("Upload complete.");
    Ok(())
}

fn run_directory_upload(
    logger: &mut Logger,
    client: &reqwest::blocking::Client,
    target: UploadRunTarget<'_>,
    src: &Path,
) -> Result<()> {
    let mut dirs = vec![PathBuf::from(".")];
    let mut files = Vec::new();
    for entry in walkdir_sorted(src)? {
        if entry.is_dir() {
            let rel = entry
                .strip_prefix(src)
                .unwrap_or(entry.as_path())
                .to_path_buf();
            dirs.push(rel);
        } else if entry.is_file() {
            let rel = entry
                .strip_prefix(src)
                .unwrap_or(entry.as_path())
                .to_path_buf();
            files.push((rel, entry.to_path_buf()));
        }
    }

    dirs.sort();
    dirs.dedup();

    for rel_dir in dirs {
        let remote_dir = remote_join(target.dst_root, &rel_dir);
        if target.skip_mkdir {
            logger.info(format!("[mkdir -p] skipped ({remote_dir})"));
            continue;
        }
        logger.info(format!("[mkdir -p] {remote_dir}"));
        mkdir_p(client, target.request_ctx, &remote_dir)?;
    }

    for (rel_file, local_file) in files {
        let remote_file = remote_join(target.dst_root, &rel_file);
        logger.info(format!(
            "[upload] {} -> {remote_file}",
            local_file.display()
        ));
        upload_file(client, target.request_ctx, &local_file, &remote_file)?;
    }

    logger.info("Upload complete.");
    Ok(())
}

pub fn make_direct_upload_client(timeout_sec: f64) -> Result<reqwest::blocking::Client> {
    make_client(timeout_sec)
}

pub fn upload_file_direct_fast_with_client(
    logger: &mut Logger,
    client: &reqwest::blocking::Client,
    opts: DirectUploadOptions<'_>,
) -> Result<()> {
    if !opts.src.exists() {
        return Err(anyhow!(
            "Source path does not exist: {}",
            opts.src.display()
        ));
    }

    // Keep wifi-acceptance failures bounded when host->device HTTP path is broken.
    require_health_check(client, opts.host, opts.port, opts.timeout_sec, 3, 250)?;
    let skip_mkdir = env_utils::parse_env_bool01("HOSTCTL_UPLOAD_SKIP_MKDIR", false)?;
    let upload_target = UploadRunTarget {
        request_ctx: RequestContext {
            host: opts.host,
            port: opts.port,
            timeout_sec: opts.timeout_sec,
            token: opts.token,
            retry_policy: opts.retry_policy,
        },
        dst_root: opts.dst_root,
        skip_mkdir,
    };

    run_single_file_upload(logger, client, upload_target, opts.src)
}

pub fn stat_remote_file(
    host: &str,
    port: u16,
    timeout_sec: f64,
    remote_path: &str,
    token: Option<&str>,
    retry_policy: UploadRetryPolicy,
) -> Result<bool> {
    let client = make_client(timeout_sec)?;
    let request_ctx = RequestContext {
        host,
        port,
        timeout_sec,
        token,
        retry_policy,
    };
    match stat_path(&client, request_ctx, remote_path) {
        Ok(()) => Ok(true),
        Err(err) => {
            let msg = err.to_string().to_lowercase();
            if msg.contains("404") || msg.contains("not found") {
                Ok(false)
            } else {
                Err(err)
            }
        }
    }
}

fn retry_policy_from_general_env() -> Result<UploadRetryPolicy> {
    Ok(UploadRetryPolicy {
        sd_busy_total_retry_sec: env_utils::parse_env_f64(
            "HOSTCTL_UPLOAD_SD_BUSY_TOTAL_RETRY_SEC",
            180.0,
        )?,
        net_recovery_timeout_sec: env_utils::parse_env_f64(
            "HOSTCTL_UPLOAD_NET_RECOVERY_TIMEOUT_SEC",
            45.0,
        )?,
        net_recovery_poll_sec: env_utils::parse_env_f64(
            "HOSTCTL_UPLOAD_NET_RECOVERY_POLL_SEC",
            0.8,
        )?,
        net_recovery_consecutive_health_successes: env_utils::parse_env_u32(
            "HOSTCTL_UPLOAD_NET_RECOVERY_CONSECUTIVE_HEALTH",
            2,
        )?
        .max(1),
    })
}

fn require_health_check(
    client: &reqwest::blocking::Client,
    host: &str,
    port: u16,
    timeout_sec: f64,
    attempts: u32,
    retry_delay_ms: u64,
) -> Result<()> {
    let attempt_count = attempts.max(1);
    let health_url = format!("http://{host}:{port}/health");
    let mut last_error = String::from("<none>");
    for idx in 0..attempt_count {
        match request_raw(
            client,
            Method::GET,
            &health_url,
            None,
            None,
            health_timeout_s(timeout_sec),
        ) {
            Ok(_) => return Ok(()),
            Err(err) => {
                last_error = err.to_string();
            }
        }
        if idx + 1 < attempt_count {
            thread::sleep(Duration::from_millis(retry_delay_ms));
        }
    }
    Err(anyhow!(
        "health check failed: GET {health_url} (attempts={attempt_count}) last_error={last_error}"
    ))
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        thread,
        time::Duration,
    };

    use super::{make_client, require_health_check};

    fn spawn_health_server(statuses: Vec<u16>) -> (u16, Arc<AtomicUsize>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        listener.set_nonblocking(false).expect("set blocking mode");
        let port = listener.local_addr().expect("local_addr").port();
        let hit_count = Arc::new(AtomicUsize::new(0));
        let hit_count_clone = Arc::clone(&hit_count);
        let handle = thread::spawn(move || {
            for code in statuses {
                let (mut stream, _) = listener.accept().expect("accept");
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                hit_count_clone.fetch_add(1, Ordering::Relaxed);
                let body = if code == 200 { "ok" } else { "err" };
                let response = format!(
                    "HTTP/1.1 {code} TEST\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (port, hit_count, handle)
    }

    #[test]
    fn health_check_retries_and_fails_with_attempt_count() {
        let (port, hit_count, handle) = spawn_health_server(vec![500, 500, 500]);
        let client = make_client(2.0).expect("client");
        let err =
            require_health_check(&client, "127.0.0.1", port, 2.0, 3, 1).expect_err("must fail");
        assert!(err.to_string().contains("attempts=3"));
        assert_eq!(hit_count.load(Ordering::Relaxed), 3);
        handle.join().expect("join");
    }

    #[test]
    fn health_check_succeeds_after_transient_failures() {
        let (port, hit_count, handle) = spawn_health_server(vec![500, 500, 200]);
        let client = make_client(2.0).expect("client");
        require_health_check(&client, "127.0.0.1", port, 2.0, 5, 1).expect("must succeed");
        assert_eq!(hit_count.load(Ordering::Relaxed), 3);
        handle.join().expect("join");
    }
}
