use std::{
    io::{Read, Write},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use anyhow::{anyhow, Result};
use serde_json::json;
use serialport::{SerialPort, TTYPort};
use tempfile::tempdir;

use super::super::{profile::DiscoveryProfile, WifiDiscoveryRuntime};
use crate::{
    logging::Logger,
    serial_console::SerialConsole,
    workflows::wifi::common::{MemDiagSummary, NetPolicy},
};

fn open_pty_pair() -> Result<(TTYPort, TTYPort)> {
    TTYPort::pair().map_err(|err| anyhow!("TTYPort::pair failed: {err}"))
}

fn spawn_discovery_panic_responder(mut master: TTYPort) -> thread::JoinHandle<()> {
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
                if command == "NET START" {
                    let _ = master.write_all(b"NET OK op=start\r\n");
                    let _ = master.flush();
                } else if command == "NET STATUS" {
                    let _ = master.write_all(
                        b"NET_STATUS {\"state\":\"Idle\",\"link\":false,\"ipv4\":\"0.0.0.0\",\"listener\":false,\"listener_enabled\":false}\r\n",
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
        }
    })
}

fn test_console(slave: TTYPort) -> Result<SerialConsole> {
    let temp = tempdir()?;
    let log_path = PathBuf::from(temp.path()).join("discovery_runtime_panic_test.log");
    SerialConsole::from_port_for_tests(Box::new(slave), Some(&log_path))
}

#[test]
fn probe_round_fails_fast_when_panic_marker_is_seen() -> Result<()> {
    let (master, slave) = open_pty_pair()?;
    let responder = spawn_discovery_panic_responder(master);
    let mut logger = Logger::new(None)?;
    let mut runtime = WifiDiscoveryRuntime {
        logger: &mut logger,
        console: test_console(slave)?,
        ssid: "test-ap".to_string(),
        password: "test-pass".to_string(),
        policy: NetPolicy::default(),
        profile: DiscoveryProfile {
            rounds: 1,
            round_timeout_ms: 2_000,
            poll_interval_ms: 30,
            status_poll_ms: 120,
            force_stop_before_round: false,
            recover_before_round: false,
            recover_after_round: false,
            recover_settle_ms: 6_000,
            disable_listener_during_probe_rounds: false,
            max_zero_discovery_rounds: 0,
            min_ready_rounds: 1,
            min_ssid_seen_rounds: 1,
            require_scan_evidence_each_round: false,
        },
        samples: Vec::new(),
        ready_rounds: 0,
        zero_discovery_rounds: 0,
        ssid_seen_rounds: 0,
        total_scan_zero_events: 0,
        total_scan_nonzero_events: 0,
        total_scan_runs_delta: 0,
        total_no_ap_found_events: 0,
        last_wifi_metrics_scan_counters: None,
        mem_diag: MemDiagSummary::default(),
        panic_first: None,
    };

    let mut context = json!({"round_index": 0});
    let err = runtime
        .handle_probe_round(&mut context)
        .expect_err("panic marker should fail round immediately");
    assert!(err.to_string().contains("panic_detected"));
    assert!(err.to_string().contains("runtime_panic_guru"));
    assert!(runtime.panic_first.is_some());

    drop(runtime);
    responder
        .join()
        .map_err(|_| anyhow!("discovery panic responder thread panicked"))?;
    Ok(())
}
