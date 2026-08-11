use super::command_run::append_log_line;
use super::{CommandMonitorState, CommandProgressMode, LogRelayThread};
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::ExitStatus,
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Result};

use crate::logging::ensure_parent_dir;

pub(super) fn handle_idle_timeout(
    child: &mut std::process::Child,
    log_path: &Path,
    idle_timeout: Option<Duration>,
    monitor_state: &Arc<Mutex<CommandMonitorState>>,
) -> Result<Option<ExitStatus>> {
    let Some(idle_timeout) = idle_timeout else {
        return Ok(None);
    };
    let idle_elapsed = monitor_state
        .lock()
        .map_err(|_| anyhow!("command monitor mutex poisoned"))?
        .last_output_at
        .elapsed();
    if idle_elapsed < idle_timeout {
        return Ok(None);
    }
    child.kill().ok();
    let status = child.wait()?;
    append_log_line(
        log_path,
        &format!(
            "command output stalled after {}s without new child output\n",
            idle_timeout.as_secs()
        ),
    )?;
    Ok(Some(status))
}

pub(super) fn handle_progress_timeout(
    child: &mut std::process::Child,
    log_path: &Path,
    progress_stall_timeout: Option<Duration>,
    monitor_state: &Arc<Mutex<CommandMonitorState>>,
) -> Result<Option<ExitStatus>> {
    let Some(progress_stall_timeout) = progress_stall_timeout else {
        return Ok(None);
    };
    let (progress_elapsed, last_progress_marker) = {
        let state = monitor_state
            .lock()
            .map_err(|_| anyhow!("command monitor mutex poisoned"))?;
        (
            state.last_progress_at.map(|instant| instant.elapsed()),
            state.last_progress_marker.clone(),
        )
    };
    let Some(progress_elapsed) = progress_elapsed else {
        return Ok(None);
    };
    if progress_elapsed < progress_stall_timeout {
        return Ok(None);
    }
    child.kill().ok();
    let status = child.wait()?;
    let detail = last_progress_marker
        .as_deref()
        .map(|marker| format!(" (last progress: {marker})"))
        .unwrap_or_default();
    append_log_line(
        log_path,
        &format!(
            "command progress stalled after {}s without esptool write advancement{}\n",
            progress_stall_timeout.as_secs(),
            detail
        ),
    )?;
    Ok(Some(status))
}

pub(super) fn spawn_log_tee<R>(
    id: usize,
    mut reader: R,
    log_path: PathBuf,
    monitor_state: Arc<Mutex<CommandMonitorState>>,
    progress_mode: Option<CommandProgressMode>,
    done_tx: mpsc::Sender<usize>,
) -> LogRelayThread
where
    R: Read + Send + 'static,
{
    let handle = thread::spawn(move || {
        ensure_parent_dir(&log_path)?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;
        let mut buffer = [0_u8; 4096];
        let mut pending = String::new();
        loop {
            let bytes_read = reader.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            file.write_all(&buffer[..bytes_read])?;
            monitor_state
                .lock()
                .map_err(|_| anyhow!("command monitor mutex poisoned"))?
                .note_output();
            if let Some(progress_mode) = progress_mode {
                let chunk = String::from_utf8_lossy(&buffer[..bytes_read]);
                pending.push_str(&chunk);
                drain_progress_lines(&monitor_state, progress_mode, &mut pending)?;
            }
        }
        if let Some(progress_mode) = progress_mode {
            let trailing = pending.trim();
            if let Some(marker) = progress_marker(progress_mode, trailing) {
                monitor_state
                    .lock()
                    .map_err(|_| anyhow!("command monitor mutex poisoned"))?
                    .note_progress(marker);
            }
        }
        let _ = done_tx.send(id);
        Ok(())
    });
    LogRelayThread { id, handle }
}

pub(super) fn wait_for_log_threads(
    handles: &mut Vec<LogRelayThread>,
    done_rx: &mpsc::Receiver<usize>,
    log_path: &Path,
    drain_timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now()
        .checked_add(drain_timeout)
        .unwrap_or_else(Instant::now);
    let mut completed_ids = Vec::new();
    while completed_ids.len() < handles.len() {
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        match done_rx.recv_timeout(deadline.saturating_duration_since(now)) {
            Ok(id) => completed_ids.push(id),
            Err(mpsc::RecvTimeoutError::Timeout) => break,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let mut timed_out = false;
    for relay in handles.drain(..) {
        if completed_ids.contains(&relay.id) {
            match relay.handle.join() {
                Ok(result) => result?,
                Err(_) => bail!("command log relay thread panicked"),
            }
        } else {
            timed_out = true;
        }
    }
    if timed_out {
        append_log_line(
            log_path,
            &format!(
                "command log drain exceeded {}ms; detaching pending relay thread(s)\n",
                drain_timeout.as_millis()
            ),
        )?;
    }
    Ok(())
}

fn drain_progress_lines(
    monitor_state: &Arc<Mutex<CommandMonitorState>>,
    progress_mode: CommandProgressMode,
    pending: &mut String,
) -> Result<()> {
    while let Some(line) = take_next_line(pending) {
        if let Some(marker) = progress_marker(progress_mode, &line) {
            monitor_state
                .lock()
                .map_err(|_| anyhow!("command monitor mutex poisoned"))?
                .note_progress(marker);
        }
    }
    Ok(())
}

fn take_next_line(pending: &mut String) -> Option<String> {
    let newline_idx = pending.find(['\n', '\r'])?;
    let line = pending[..newline_idx].to_string();
    let mut consume_len = newline_idx + 1;
    while consume_len < pending.len() {
        let next = pending.as_bytes()[consume_len];
        if next != b'\n' && next != b'\r' {
            break;
        }
        consume_len += 1;
    }
    pending.drain(..consume_len);
    Some(line)
}

fn progress_marker(progress_mode: CommandProgressMode, line: &str) -> Option<String> {
    let trimmed = line.trim();
    match progress_mode {
        CommandProgressMode::EsptoolWriteFlash => trimmed
            .starts_with("Writing at 0x")
            .then(|| trimmed.to_string()),
    }
}
