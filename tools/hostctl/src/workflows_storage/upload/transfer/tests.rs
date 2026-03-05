use std::{
    fs,
    io::{Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use super::{make_client, upload_file, RequestContext};
use crate::workflows_storage::upload::UploadRetryPolicy;

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvVarRestore {
    key: &'static str,
    old: Option<String>,
}

impl EnvVarRestore {
    fn set(key: &'static str, value: &str) -> Self {
        let old = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self { key, old }
    }
}

impl Drop for EnvVarRestore {
    fn drop(&mut self) {
        if let Some(old) = self.old.as_deref() {
            std::env::set_var(self.key, old);
        } else {
            std::env::remove_var(self.key);
        }
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn read_http_request(
    stream: &mut TcpStream,
) -> std::io::Result<(String, String, usize, usize)> {
    const HEADER_END: &[u8] = b"\r\n\r\n";
    let mut raw = Vec::<u8>::with_capacity(4096);
    let mut scratch = [0u8; 2048];
    let header_end = loop {
        let n = stream.read(&mut scratch)?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "eof before request header",
            ));
        }
        raw.extend_from_slice(&scratch[..n]);
        if let Some(end) = find_bytes(&raw, HEADER_END) {
            break end;
        }
    };
    let header_text = std::str::from_utf8(&raw[..header_end])
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "header not utf8"))?;
    let mut lines = header_text.lines();
    let request_line = lines
        .next()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "missing request line"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();
    let mut content_length = 0usize;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse().unwrap_or(0);
            break;
        }
    }
    let body_prefetched = raw.len().saturating_sub(header_end + HEADER_END.len());
    Ok((method, target, content_length, body_prefetched))
}

fn drain_body(
    stream: &mut TcpStream,
    content_length: usize,
    prefetched: usize,
) -> std::io::Result<()> {
    let mut remaining = content_length.saturating_sub(prefetched);
    let mut scratch = [0u8; 2048];
    while remaining > 0 {
        let n = stream.read(&mut scratch)?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "eof while draining request body",
            ));
        }
        remaining = remaining.saturating_sub(n);
    }
    Ok(())
}

fn write_response(stream: &mut TcpStream, status: &str, body: &str) -> std::io::Result<()> {
    let resp = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(resp.as_bytes())?;
    stream.flush()?;
    Ok(())
}

#[test]
fn direct_mode_repeated_transport_reset_falls_back_to_chunked_upload() {
    let _guard = env_lock().lock().expect("env lock");

    let _mode = EnvVarRestore::set("HOSTCTL_UPLOAD_MODE", "direct");
    let _force_close = EnvVarRestore::set("HOSTCTL_UPLOAD_FORCE_CONN_CLOSE", "1");
    let _send_diag = EnvVarRestore::set("HOSTCTL_UPLOAD_SEND_DIAG", "0");
    let _burst_sender = EnvVarRestore::set("HOSTCTL_UPLOAD_DIRECT_BURST_SENDER", "0");
    let _fast_retry = EnvVarRestore::set("HOSTCTL_UPLOAD_TRANSPORT_RESET_FAST_RETRY", "1");
    let _fast_retry_streak =
        EnvVarRestore::set("HOSTCTL_UPLOAD_TRANSPORT_RESET_FAST_RETRY_STREAK", "2");
    let _chunk_fallback =
        EnvVarRestore::set("HOSTCTL_UPLOAD_TRANSPORT_RESET_CHUNK_FALLBACK", "1");
    let _chunk_fallback_streak =
        EnvVarRestore::set("HOSTCTL_UPLOAD_TRANSPORT_RESET_CHUNK_FALLBACK_STREAK", "1");

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock upload server");
    listener
        .set_nonblocking(true)
        .expect("set listener nonblocking");
    let port = listener.local_addr().expect("server addr").port();

    let direct_put_seen = Arc::new(AtomicUsize::new(0));
    let begin_seen = Arc::new(AtomicUsize::new(0));
    let chunk_seen = Arc::new(AtomicUsize::new(0));
    let commit_seen = Arc::new(AtomicUsize::new(0));

    let direct_put_seen_server = Arc::clone(&direct_put_seen);
    let begin_seen_server = Arc::clone(&begin_seen);
    let chunk_seen_server = Arc::clone(&chunk_seen);
    let commit_seen_server = Arc::clone(&commit_seen);
    let server_thread = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            let Ok((mut stream, _)) = listener.accept() else {
                thread::sleep(Duration::from_millis(10));
                if commit_seen_server.load(Ordering::Relaxed) > 0 {
                    break;
                }
                continue;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
            let Ok((method, target, content_length, prefetched)) = read_http_request(&mut stream) else {
                continue;
            };

            if method == "PUT" && target.starts_with("/upload?") {
                let seen = direct_put_seen_server.fetch_add(1, Ordering::Relaxed) + 1;
                if seen <= 2 {
                    let _ = stream.shutdown(Shutdown::Both);
                    continue;
                }
                let _ = write_response(&mut stream, "500 Internal Server Error", "direct should have fallen back");
                continue;
            }

            if method == "GET" && target == "/health" {
                let _ = write_response(&mut stream, "200 OK", "ok");
                continue;
            }
            if method == "POST" && target.starts_with("/upload_abort") {
                let _ = drain_body(&mut stream, content_length, prefetched);
                let _ = write_response(&mut stream, "200 OK", "abort ok");
                continue;
            }
            if method == "POST" && target.starts_with("/upload_begin") {
                begin_seen_server.fetch_add(1, Ordering::Relaxed);
                let _ = drain_body(&mut stream, content_length, prefetched);
                let _ = write_response(&mut stream, "200 OK", "begin ok");
                continue;
            }
            if method == "PUT" && target == "/upload_chunk" {
                chunk_seen_server.fetch_add(1, Ordering::Relaxed);
                let _ = drain_body(&mut stream, content_length, prefetched);
                let _ = write_response(&mut stream, "200 OK", "chunk ok");
                continue;
            }
            if method == "POST" && target == "/upload_commit" {
                commit_seen_server.fetch_add(1, Ordering::Relaxed);
                let _ = drain_body(&mut stream, content_length, prefetched);
                let _ = write_response(&mut stream, "200 OK", "commit ok");
                continue;
            }

            let _ = write_response(&mut stream, "404 Not Found", "not found");
        }
    });

    let mut temp_path = std::env::temp_dir();
    let unique_suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before epoch")
        .as_nanos();
    temp_path.push(format!(
        "hostctl_upload_fallback_test_{}_{}.bin",
        std::process::id(),
        unique_suffix
    ));
    fs::write(&temp_path, vec![0xAB; 32 * 1024]).expect("write temp payload");

    let client = make_client(1.2).expect("client");
    let request_ctx = RequestContext {
        host: "127.0.0.1",
        port,
        timeout_sec: 1.2,
        token: None,
        retry_policy: UploadRetryPolicy {
            sd_busy_total_retry_sec: 3.0,
            net_recovery_timeout_sec: 0.4,
            net_recovery_poll_sec: 0.05,
            net_recovery_consecutive_health_successes: 1,
        },
    };
    let upload_result = upload_file(&client, request_ctx, &temp_path, "/assets/fallback.bin");
    let _ = fs::remove_file(&temp_path);
    upload_result.expect("upload with fallback");

    assert_eq!(
        direct_put_seen.load(Ordering::Relaxed),
        2,
        "expected two direct PUT failures before fallback"
    );
    assert!(
        begin_seen.load(Ordering::Relaxed) >= 1,
        "expected chunked begin after fallback"
    );
    assert!(
        chunk_seen.load(Ordering::Relaxed) >= 1,
        "expected at least one chunk request after fallback"
    );
    assert_eq!(
        commit_seen.load(Ordering::Relaxed),
        1,
        "expected single commit after chunk fallback"
    );

    server_thread.join().expect("join mock server");
}
