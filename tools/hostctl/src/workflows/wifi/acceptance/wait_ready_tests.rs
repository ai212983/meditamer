use std::{
    io::{Read, Write},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use serialport::{SerialPort, TTYPort};
use tempfile::tempdir;

use super::{wait_ready, wait_state_progress, WaitReadyState};
use crate::{
    serial_console::SerialConsole,
    workflows::wifi::common::{NetPolicy, NetStatus},
};

fn open_pty_pair() -> Result<(TTYPort, TTYPort)> {
    TTYPort::pair().map_err(|err| anyhow!("TTYPort::pair failed: {err}"))
}

fn spawn_status_panic_responder(mut master: TTYPort) -> thread::JoinHandle<()> {
    let _ = master.set_timeout(Duration::from_millis(80));
    thread::spawn(move || {
        let mut rx = Vec::<u8>::new();
        let mut chunk = [0u8; 512];
        let mut emitted_panic = false;
        let mut last_activity = Instant::now();

        loop {
            let n = match master.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => {
                    last_activity = Instant::now();
                    n
                }
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(err) if err.kind() == std::io::ErrorKind::TimedOut => {
                    if last_activity.elapsed() > Duration::from_secs(2) {
                        break;
                    }
                    continue;
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if last_activity.elapsed() > Duration::from_secs(2) {
                        break;
                    }
                    continue;
                }
                Err(_) => break,
            };
            rx.extend_from_slice(&chunk[..n]);

            while let Some(pos) = rx.iter().position(|byte| *byte == b'\n') {
                let mut line = rx.drain(..=pos).collect::<Vec<u8>>();
                while matches!(line.last(), Some(b'\r' | b'\n')) {
                    line.pop();
                }
                if line.is_empty() {
                    continue;
                }
                let command = String::from_utf8_lossy(&line).trim().to_string();
                if command != "NET STATUS" {
                    continue;
                }

                let _ = master.write_all(
                    b"NET_STATUS {\"state\":\"Idle\",\"link\":false,\"ipv4\":\"0.0.0.0\",\"listener\":false,\"listener_enabled\":false,\"failure_class\":\"none\"}\r\n",
                );
                if !emitted_panic {
                    let _ = master.write_all(
                        b"Guru Meditation Error: Core 0 panic'ed (LoadProhibited)\r\n",
                    );
                    emitted_panic = true;
                }
                let _ = master.flush();
            }
        }
    })
}

fn test_console(slave: TTYPort) -> Result<SerialConsole> {
    let temp = tempdir()?;
    let log_path = PathBuf::from(temp.path()).join("wait_ready_panic_test.log");
    SerialConsole::from_port_for_tests(Box::new(slave), Some(&log_path))
}

#[test]
fn wait_state_progress_fails_fast_on_panic_marker() -> Result<()> {
    let (master, slave) = open_pty_pair()?;
    let responder = spawn_status_panic_responder(master);
    let mut console = test_console(slave)?;

    let err = wait_state_progress(&mut console, 2_500).expect_err("must fail");
    assert!(err.to_string().contains("panic_detected"));
    assert!(err.to_string().contains("runtime_panic_guru"));

    responder
        .join()
        .map_err(|_| anyhow!("status panic responder thread panicked"))?;
    Ok(())
}

#[test]
fn wait_ready_fails_fast_on_panic_marker() -> Result<()> {
    let (master, slave) = open_pty_pair()?;
    let responder = spawn_status_panic_responder(master);
    let mut console = test_console(slave)?;

    let err = wait_ready(&mut console, NetPolicy::default()).expect_err("must fail");
    assert!(err.to_string().contains("panic_detected"));
    assert!(err.to_string().contains("runtime_panic_guru"));

    responder
        .join()
        .map_err(|_| anyhow!("status panic responder thread panicked"))?;
    Ok(())
}

#[test]
fn wait_ready_resets_post_connect_window_when_attempt_advances() {
    let mut state = WaitReadyState::new(Instant::now(), NetPolicy::default());
    state.update_connect_timing(&NetStatus {
        state: Some("ListenerWait".to_string()),
        link: Some(true),
        ipv4: Some("0.0.0.0".to_string()),
        listener: Some(false),
        listener_enabled: Some(true),
        failure_class: Some("none".to_string()),
        failure_code: Some(0),
        ladder_step: Some("retry_same".to_string()),
        attempt: Some(1),
        uptime_ms: Some(1_000),
    });
    let first_deadline = state.post_connect_deadline.expect("first deadline");
    thread::sleep(Duration::from_millis(10));
    state.update_connect_timing(&NetStatus {
        attempt: Some(2),
        ..NetStatus {
            state: Some("DhcpWait".to_string()),
            link: Some(true),
            ipv4: Some("0.0.0.0".to_string()),
            listener: Some(false),
            listener_enabled: Some(true),
            failure_class: Some("none".to_string()),
            failure_code: Some(0),
            ladder_step: Some("retry_same".to_string()),
            attempt: Some(1),
            uptime_ms: Some(2_000),
        }
    });
    let reset_deadline = state.post_connect_deadline.expect("reset deadline");
    assert!(reset_deadline > first_deadline);
}
