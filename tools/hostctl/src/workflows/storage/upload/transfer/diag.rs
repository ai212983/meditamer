use std::{
    fs,
    io::Write,
    path::PathBuf,
};

use super::super::client::{RequestContext, TimedResponse};

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

pub(super) fn append_host_diag_line(line: &str) {
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

pub(super) fn log_direct_upload_diag(
    timed: &TimedResponse,
    remote_path: &str,
    request_ctx: RequestContext<'_>,
) {
    let cadence = timed.timing.body_cadence;
    let bytes_per_read = if cadence.read_calls == 0 {
        0
    } else {
        cadence.read_bytes / cadence.read_calls as usize
    };
    let line = format!(
        "host_upload_send_diag: mode=direct path={} target={}:{} attempts={} body_bytes={} send_ms={} resp_read_ms={} total_ms={} body_read_calls={} body_short_reads={} body_bytes_per_read={} body_gap_ms_total={} body_gap_ms_max={} body_gap_over_10ms={} body_gap_over_50ms={}",
        remote_path,
        request_ctx.host,
        request_ctx.port,
        timed.attempts,
        timed.timing.body_bytes,
        timed.timing.send_ms,
        timed.timing.response_read_ms,
        timed.timing.total_ms,
        cadence.read_calls,
        cadence.short_reads,
        bytes_per_read,
        cadence.read_gap_ms_total,
        cadence.read_gap_ms_max,
        cadence.read_gap_over_10ms,
        cadence.read_gap_over_50ms,
    );
    println!("{line}");
    append_host_diag_line(&line);
}
