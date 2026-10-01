use super::command_relay::{
    handle_idle_timeout, handle_progress_timeout, spawn_log_tee, wait_for_log_threads,
};
use super::{CommandMonitorState, CommandProgressMode, CommandSpec};
use std::{
    fs::OpenOptions,
    io::Write,
    path::Path,
    process::{Command, ExitStatus, Stdio},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

use anyhow::Result;

use crate::logging::ensure_parent_dir;

#[derive(Clone, Copy, Debug)]
pub struct CommandRunOptions<'a> {
    pub(super) timeout: Option<Duration>,
    pub(super) progress_interval: Option<Duration>,
    pub(super) progress_label: Option<&'a str>,
    pub(super) idle_timeout: Option<Duration>,
    pub(super) progress_stall_timeout: Option<Duration>,
    pub(super) progress_mode: Option<CommandProgressMode>,
    pub(super) log_drain_timeout: Duration,
}

impl CommandRunOptions<'_> {
    pub(super) const fn new(log_drain_timeout: Duration) -> Self {
        Self {
            timeout: None,
            progress_interval: None,
            progress_label: None,
            idle_timeout: None,
            progress_stall_timeout: None,
            progress_mode: None,
            log_drain_timeout,
        }
    }
}

pub fn run_command_logged(
    spec: &CommandSpec,
    log_path: &Path,
    opts: CommandRunOptions<'_>,
) -> Result<ExitStatus> {
    append_log_line(log_path, &format!("$ {}\n", format_command(spec)))?;
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(current_dir) = &spec.current_dir {
        command.current_dir(current_dir);
    }
    for name in &spec.env_remove {
        command.env_remove(name);
    }
    let mut child = command.spawn()?;
    let monitor_state = Arc::new(Mutex::new(CommandMonitorState::new()));
    let (relay_done_tx, relay_done_rx) = mpsc::channel();
    let mut log_threads = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        log_threads.push(spawn_log_tee(
            log_threads.len(),
            stdout,
            log_path.to_path_buf(),
            monitor_state.clone(),
            opts.progress_mode,
            relay_done_tx.clone(),
        ));
    }
    if let Some(stderr) = child.stderr.take() {
        log_threads.push(spawn_log_tee(
            log_threads.len(),
            stderr,
            log_path.to_path_buf(),
            monitor_state.clone(),
            opts.progress_mode,
            relay_done_tx.clone(),
        ));
    }
    drop(relay_done_tx);

    if opts.timeout.is_none()
        && opts.progress_interval.is_none()
        && opts.idle_timeout.is_none()
        && opts.progress_stall_timeout.is_none()
    {
        let status = child.wait()?;
        wait_for_log_threads(
            &mut log_threads,
            &relay_done_rx,
            log_path,
            opts.log_drain_timeout,
        )?;
        return Ok(status);
    }

    let started = Instant::now();
    let heartbeat_label = opts.progress_label.unwrap_or("command");
    let mut next_progress = opts
        .progress_interval
        .map(|interval| started.checked_add(interval).unwrap_or(started));

    loop {
        if let Some(status) = child.try_wait()? {
            wait_for_log_threads(
                &mut log_threads,
                &relay_done_rx,
                log_path,
                opts.log_drain_timeout,
            )?;
            return Ok(status);
        }
        emit_heartbeat(
            log_path,
            opts.progress_interval,
            &mut next_progress,
            started,
            heartbeat_label,
        )?;
        if let Some(status) = handle_command_timeout(&mut child, log_path, opts.timeout, started)? {
            wait_for_log_threads(
                &mut log_threads,
                &relay_done_rx,
                log_path,
                opts.log_drain_timeout,
            )?;
            return Ok(status);
        }
        if let Some(status) =
            handle_idle_timeout(&mut child, log_path, opts.idle_timeout, &monitor_state)?
        {
            wait_for_log_threads(
                &mut log_threads,
                &relay_done_rx,
                log_path,
                opts.log_drain_timeout,
            )?;
            return Ok(status);
        }
        if let Some(status) = handle_progress_timeout(
            &mut child,
            log_path,
            opts.progress_stall_timeout,
            &monitor_state,
        )? {
            wait_for_log_threads(
                &mut log_threads,
                &relay_done_rx,
                log_path,
                opts.log_drain_timeout,
            )?;
            return Ok(status);
        }
        thread::sleep(Duration::from_millis(250));
    }
}

pub(super) fn append_log_line(path: &Path, line: &str) -> Result<()> {
    ensure_parent_dir(path)?;
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())?;
    Ok(())
}

fn format_command(spec: &CommandSpec) -> String {
    std::iter::once(spec.program.to_string_lossy().into_owned())
        .chain(
            spec.args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned()),
        )
        .collect::<Vec<_>>()
        .join(" ")
}

fn emit_heartbeat(
    log_path: &Path,
    progress_interval: Option<Duration>,
    next_progress: &mut Option<Instant>,
    started: Instant,
    heartbeat_label: &str,
) -> Result<()> {
    let now = Instant::now();
    if let (Some(interval), Some(deadline)) = (progress_interval, *next_progress) {
        if now >= deadline {
            let elapsed = started.elapsed().as_secs();
            let line = format!("{heartbeat_label} in progress... elapsed {elapsed}s");
            println!("{line}");
            append_log_line(log_path, &format!("{line}\n"))?;
            *next_progress = deadline
                .checked_add(interval)
                .or_else(|| now.checked_add(interval));
        }
    }
    Ok(())
}

fn handle_command_timeout(
    child: &mut std::process::Child,
    log_path: &Path,
    timeout: Option<Duration>,
    started: Instant,
) -> Result<Option<ExitStatus>> {
    let Some(timeout) = timeout else {
        return Ok(None);
    };
    if started.elapsed() < timeout {
        return Ok(None);
    }
    child.kill().ok();
    let status = child.wait()?;
    append_log_line(
        log_path,
        &format!("command timed out after {}s\n", timeout.as_secs()),
    )?;
    Ok(Some(status))
}
