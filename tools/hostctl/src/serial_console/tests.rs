use std::time::{Duration, Instant};

use anyhow::Result;

use super::{mock::MockPort, sdreq_regex, SerialConsole};

#[test]
fn inkplate_metrics_waits_for_the_final_record_without_reissuing() -> Result<()> {
    let port = MockPort::new().pushed_reads(&[
        b"METRICS TOUCH_SCHED active_n=7\r\n",
        b"METRICS UPLOAD req_read_body_reset=0\r\n",
        b"METRICS NET_ACCEPT arm_gap_n=3\r\n",
    ]);
    let state = port.state.clone();
    let mut console = SerialConsole::from_port_for_tests(Box::new(port), None)?;
    let line = console.command_wait_inkplate_metrics(
        &regex::Regex::new(r"^METRICS TOUCH_SCHED ")?,
        Duration::from_secs(1),
    )?;
    assert_eq!(line.as_deref(), Some("METRICS TOUCH_SCHED active_n=7"));
    assert_eq!(console.mark(), 3);
    assert_eq!(state.lock().unwrap().writes.concat(), b"METRICS\r\n");
    Ok(())
}

#[test]
fn busy_metrics_is_not_a_successful_snapshot() -> Result<()> {
    let port = MockPort::new().pushed_reads(&[b"METRICS BUSY\r\n"]);
    let mut console = SerialConsole::from_port_for_tests(Box::new(port), None)?;
    assert!(console
        .command_wait_inkplate_metrics(&regex::Regex::new(r"^METRICS ")?, Duration::from_secs(1))
        .is_err());
    Ok(())
}

#[test]
fn continuous_logs_do_not_prevent_acknowledgement_timeout() -> Result<()> {
    let port = MockPort::new();
    port.state.lock().expect("lock").repeat_read = true;
    let mut console = SerialConsole::from_port_for_tests(Box::new(port), None)?;
    let started = std::time::Instant::now();
    let result = console.command_wait_regex(
        "PING",
        &regex::Regex::new(r"^PONG$")?,
        Duration::from_millis(50),
    )?;
    assert!(result.is_none());
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(console.mark() > 0, "continuous logs were consumed");
    Ok(())
}

#[test]
fn serial_write_errors_do_not_disclose_command_payloads() -> Result<()> {
    let port = MockPort::new().failing_writes();
    let mut console = SerialConsole::from_port_for_tests(Box::new(port), None)?;
    let secret = "NETCFG SET {\"password\":\"do-not-log\"}";
    let error = console.send_line(secret).expect_err("write must fail");
    let detail = error.to_string();
    assert!(detail.contains("failed to write serial line"));
    assert!(!detail.contains("do-not-log"));
    assert!(!detail.contains("NETCFG SET"));
    Ok(())
}

#[test]
fn paced_serial_line_stays_below_the_device_fifo_and_preserves_bytes() -> Result<()> {
    let port = MockPort::new();
    let state = port.state.clone();
    let mut console = SerialConsole::from_port_for_tests(Box::new(port), None)?;
    let command = format!("NETCFG SET {{\"ssid\":\"{}\"}}", "x".repeat(300));
    console.send_line_paced(&command, 32, Duration::ZERO)?;

    let state = state.lock().expect("lock");
    assert!(state.writes.iter().all(|chunk| chunk.len() <= 32));
    let written = state.writes.concat();
    assert_eq!(written, format!("{command}\r\n").as_bytes());
    Ok(())
}

#[test]
fn paced_serial_line_rejects_a_zero_chunk_size() -> Result<()> {
    let port = MockPort::new();
    let mut console = SerialConsole::from_port_for_tests(Box::new(port), None)?;
    let error = console
        .send_line_paced("PING", 0, Duration::ZERO)
        .expect_err("zero chunk size must fail");
    assert_eq!(error.to_string(), "paced serial chunk size must be nonzero");
    Ok(())
}

#[test]
fn paced_serial_write_errors_do_not_disclose_command_payloads() -> Result<()> {
    let port = MockPort::new().failing_writes();
    let mut console = SerialConsole::from_port_for_tests(Box::new(port), None)?;
    let secret = "NETCFG SET {\"password\":\"do-not-log\"}";
    let error = console
        .send_line_paced(secret, 64, Duration::ZERO)
        .expect_err("write must fail");
    let detail = error.to_string();
    assert!(detail.contains("failed to write paced serial line"));
    assert!(!detail.contains("do-not-log"));
    assert!(!detail.contains("NETCFG SET"));
    Ok(())
}

#[test]
fn sdreq_regex_matches_exact_op_token() {
    let fat_stat = sdreq_regex(Some("fat_stat")).expect("regex compiles");
    assert!(fat_stat.is_match("SDREQ id=7 op=fat_stat"));
    assert!(fat_stat.is_match("SDREQ id=7 op=fat_stat path=/foo"));
    assert!(!fat_stat.is_match("SDREQ id=7 op=fat_stat_extra"));
}

#[test]
fn capture_raw_for_reads_lines_from_same_descriptor() -> Result<()> {
    let port = MockPort::new().pushed_reads(&[b"BOOT_RESET reason=poweron\r\n"]);
    let mut console = SerialConsole::from_port_for_tests(Box::new(port), None)?;
    let bytes = console.capture_raw_for(Duration::from_millis(150))?;
    assert!(String::from_utf8_lossy(&bytes).contains("BOOT_RESET reason=poweron"));
    let lines = console.read_recent_lines(0);
    assert!(lines
        .iter()
        .any(|line| line.contains("BOOT_RESET reason=poweron")));
    Ok(())
}

#[test]
fn ack_detection_tolerates_a_concurrent_writer_prefix() -> Result<()> {
    let port =
        MockPort::new().pushed_reads(&[b"tap_trace,123,0x00UIFIXTURE OKtap_trace,124,0x00\r\n"]);
    let mut console = SerialConsole::from_port_for_tests(Box::new(port), None)?;
    let (status, line) = console.wait_ack_since(0, "UIFIXTURE", Duration::from_millis(150))?;
    assert_eq!(status, super::AckStatus::Ok);
    assert_eq!(
        line.as_deref(),
        Some("tap_trace,123,0x00UIFIXTURE OKtap_trace,124,0x00")
    );
    Ok(())
}

#[test]
fn silence_clock_tracks_received_bytes() -> Result<()> {
    let port = MockPort::new().pushed_reads(&[b"tap_trace,1\r\n"]);
    let mut console = SerialConsole::from_port_for_tests(Box::new(port), None)?;
    assert!(console.silence_duration() < Duration::from_secs(5));
    console.poll_once()?;
    assert_eq!(console.mark(), 1, "pushed bytes were consumed");
    assert!(console.silence_duration() < Duration::from_secs(5));
    std::thread::sleep(Duration::from_millis(120));
    assert!(
        console.silence_duration() >= Duration::from_millis(100),
        "silence grows without traffic"
    );
    Ok(())
}

#[test]
fn silent_stream_aborts_marker_wait_early() -> Result<()> {
    let port = MockPort::new();
    let mut console = SerialConsole::from_port_for_tests(Box::new(port), None)?;
    let started = Instant::now();
    let found = console.wait_for_regex_since_abort_on_silence(
        0,
        &regex::Regex::new(r"^AMBIENT_ASSETS ")?,
        Duration::from_secs(30),
        Duration::from_millis(100),
    )?;
    assert!(found.is_none());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "abort precedes the full timeout"
    );
    assert!(console.silence_duration() >= Duration::from_millis(100));
    Ok(())
}

#[test]
fn live_stream_still_matches_under_silence_limit() -> Result<()> {
    let port = MockPort::new().pushed_reads(&[b"AMBIENT_ASSETS status=adopted\r\n"]);
    let mut console = SerialConsole::from_port_for_tests(Box::new(port), None)?;
    let found = console.wait_for_regex_since_abort_on_silence(
        0,
        &regex::Regex::new(r"^AMBIENT_ASSETS ")?,
        Duration::from_secs(5),
        Duration::from_secs(60),
    )?;
    assert_eq!(found.as_deref(), Some("AMBIENT_ASSETS status=adopted"));
    Ok(())
}

#[test]
fn pulse_en_reset_toggles_only_rts_with_dtr_held_low() -> Result<()> {
    let port = MockPort::new();
    let state = port.state.clone();
    let mut console = SerialConsole::from_port_for_tests(Box::new(port), None)?;
    console.pulse_en_reset(1, 0)?;
    let state = state.lock().expect("lock");
    assert_eq!(state.dtr, vec![false]);
    assert_eq!(state.rts, vec![false, true, false]);
    Ok(())
}
