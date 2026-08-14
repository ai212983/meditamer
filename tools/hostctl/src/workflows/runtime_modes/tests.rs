use std::{
    io::{Read, Write},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use serde_json::json;
use serialport::TTYPort;
use tempfile::tempdir;

use super::runtime::RuntimeModesScenarioRuntime;
use crate::{
    logging::Logger,
    scenarios::{execute_workflow, load_workflow},
    serial_console::SerialConsole,
};

fn open_pty_pair() -> Result<(TTYPort, TTYPort)> {
    TTYPort::pair().map_err(|err| anyhow!("TTYPort::pair failed: {err}"))
}

#[test]
fn runtime_modes_smoke_runs_against_fake_uart() -> Result<()> {
    let (mut master, slave) = open_pty_pair()?;

    let responder = thread::spawn(move || {
        let mut rx = Vec::<u8>::new();
        let mut chunk = [0u8; 512];
        let mut upload = "off".to_string();
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

            while let Some(pos) = rx.iter().position(|b| *b == b'\n') {
                let mut line = rx.drain(..=pos).collect::<Vec<u8>>();
                while matches!(line.last(), Some(b'\r' | b'\n')) {
                    line.pop();
                }
                if line.is_empty() {
                    continue;
                }
                let command = String::from_utf8_lossy(&line).trim().to_string();
                if command.is_empty() {
                    continue;
                }

                let response = if command == "STATE GET" {
                    format!(
                        "STATE phase=idle upload={upload} diag_kind=none targets=NONE ready=true"
                    )
                } else if command == "STATE SET upload=on" {
                    upload = "on".to_string();
                    "STATE OK".to_string()
                } else if command == "STATE SET upload=off" {
                    upload = "off".to_string();
                    "STATE OK".to_string()
                } else if command == "PING" {
                    "PONG".to_string()
                } else if command == "PSRAM" {
                    "PSRAM feature_enabled=true state=ready total_bytes=1 used_bytes=1 free_bytes=0 peak_used_bytes=1"
                        .to_string()
                } else {
                    String::new()
                };

                if !response.is_empty() {
                    let _ = master.write_all(response.as_bytes());
                    let _ = master.write_all(b"\r\n");
                    let _ = master.flush();
                }
            }
        }
    });

    let temp = tempdir()?;
    let log_path = PathBuf::from(temp.path()).join("runtime_modes_fake_uart.log");
    let scenario_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/runtime-modes-smoke.sw.yaml");

    let mut logger = Logger::new(None)?;
    let workflow = load_workflow(&scenario_path)?;
    let console = SerialConsole::from_port_for_tests(Box::new(slave), Some(&log_path))?;
    let mut runtime = RuntimeModesScenarioRuntime::new(&mut logger, console, 0, 1, 1);
    let _ = execute_workflow(&workflow, &mut runtime, &json!({ "suite": "full" }))?;
    responder
        .join()
        .map_err(|_| anyhow!("fake UART responder thread panicked"))?;

    let raw = std::fs::read_to_string(log_path)?;
    if !raw.contains("STATE phase=idle") {
        return Err(anyhow!("runtime smoke capture missing STATE responses"));
    }
    if !raw.contains("PSRAM feature_enabled=true") {
        return Err(anyhow!("runtime smoke capture missing PSRAM responses"));
    }
    if !raw.contains("PONG") {
        return Err(anyhow!("runtime smoke capture missing PONG response"));
    }
    Ok(())
}
