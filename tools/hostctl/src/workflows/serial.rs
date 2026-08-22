use std::{path::PathBuf, thread, time::Duration};

use crate::{
    env_utils,
    logging::Logger,
    serial_console::{AckStatus, SerialConsole},
};
use anyhow::{anyhow, Result};
use regex::Regex;

pub struct RepaintOptions {
    pub command: Option<String>,
}

pub struct TimeSetOptions {}

pub struct TimeStatusOptions {}

/// A successful `TIMESET` + verified `TIMEGET` round trip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeSyncOutcome {
    pub utc_epoch_seconds: u32,
    pub offset_minutes: i16,
}

const TIME_SYNC_MAX_ATTEMPTS: u32 = 8;
const TIME_SYNC_RETRY_DELAY_MS: u64 = 700;
const TIME_SYNC_ACK_TIMEOUT_MS: u64 = 1_200;
/// "Slightly longer than one second" between the immediate `TIMESET`
/// readback and the delayed `TIMEGET` advancement check.
const TIME_SYNC_ADVANCE_DELAY_MS: u64 = 1_100;
const TIME_SYNC_TOLERANCE_SECS: i64 = 2;

/// Console settle time after opening the port. Shared by timeset/timestatus/
/// repaint/flash-capture post-command/time-sync; formerly five separate
/// `HOSTCTL_*_SETTLE_MS` env knobs, all default 200 (hostctl-env-audit.md).
pub(crate) const CONSOLE_SETTLE_MS: u64 = 200;

const REPAINT_RETRIES: u32 = 2;
const REPAINT_RETRY_DELAY_MS: u64 = 500;
const REPAINT_ACK_TIMEOUT_MS: u64 = 15_000;

enum TimeSetResponse {
    Ok {
        utc_epoch_seconds: u32,
        offset_minutes: i16,
    },
    Err {
        reason: String,
    },
}

enum TimeGetResponse {
    ValidOn {
        utc_epoch_seconds: u32,
        offset_minutes: i16,
    },
    ValidOff {
        reason: String,
    },
    Err {
        reason: String,
    },
}

fn parse_timeset_line(line: &str) -> Result<TimeSetResponse> {
    if let Some(caps) = Regex::new(r"^TIMESET OK utc=(\d+) offset_min=(-?\d+)")?.captures(line) {
        return Ok(TimeSetResponse::Ok {
            utc_epoch_seconds: caps[1].parse()?,
            offset_minutes: caps[2].parse()?,
        });
    }
    if let Some(caps) = Regex::new(r"^TIMESET ERR reason=(\S+)")?.captures(line) {
        return Ok(TimeSetResponse::Err {
            reason: caps[1].to_string(),
        });
    }
    Err(anyhow!("unrecognized TIMESET response: {line}"))
}

fn parse_timeget_line(line: &str) -> Result<TimeGetResponse> {
    if let Some(caps) =
        Regex::new(r"^TIMEGET OK valid=on utc=(\d+) local=\d+ offset_min=(-?\d+) os=clear")?
            .captures(line)
    {
        return Ok(TimeGetResponse::ValidOn {
            utc_epoch_seconds: caps[1].parse()?,
            offset_minutes: caps[2].parse()?,
        });
    }
    if let Some(caps) = Regex::new(r"^TIMEGET OK valid=off reason=(\S+)")?.captures(line) {
        return Ok(TimeGetResponse::ValidOff {
            reason: caps[1].to_string(),
        });
    }
    if let Some(caps) = Regex::new(r"^TIMEGET ERR reason=(\S+)")?.captures(line) {
        return Ok(TimeGetResponse::Err {
            reason: caps[1].to_string(),
        });
    }
    Err(anyhow!("unrecognized TIMEGET response: {line}"))
}

/// Samples the host wall clock right now: UTC epoch seconds and the host's
/// current fixed UTC offset in minutes, from a single clock read so the two
/// values are consistent with each other.
fn sample_host_utc_and_offset() -> Result<(u32, i16)> {
    let local_now = chrono::Local::now();
    let utc_epoch_seconds = u32::try_from(local_now.timestamp())
        .map_err(|_| anyhow!("host clock is outside the TIMESET u32 epoch range"))?;
    let offset_minutes = i16::try_from(local_now.offset().local_minus_utc() / 60)
        .map_err(|_| anyhow!("host UTC offset does not fit in the TIMESET offset range"))?;
    Ok((utc_epoch_seconds, offset_minutes))
}

/// Runs the bounded `TIMESET`/`TIMEGET` readiness policy against an already
/// open console: at most [`TIME_SYNC_MAX_ATTEMPTS`] attempts, resampling the
/// host clock immediately before every attempt (never reusing a timestamp
/// captured before a readiness delay), each gated by a
/// [`TIME_SYNC_ACK_TIMEOUT_MS`] serial acknowledgement timeout and a
/// [`TIME_SYNC_RETRY_DELAY_MS`] delay between attempts.
///
/// Shared by the `hostctl timeset` CLI command and the flash-capture
/// workflow's `time_sync` action, so both apply the identical policy.
pub fn sync_time(console: &mut SerialConsole) -> Result<TimeSyncOutcome> {
    let ack_timeout = Duration::from_millis(TIME_SYNC_ACK_TIMEOUT_MS);
    let timeset_regex = Regex::new(r"^TIMESET (OK|ERR)")?;
    let timeget_regex = Regex::new(r"^TIMEGET (OK|ERR)")?;

    let mut last_failure = String::from("no attempts were made");
    for attempt in 1..=TIME_SYNC_MAX_ATTEMPTS {
        match attempt_time_sync(console, &timeset_regex, &timeget_regex, ack_timeout) {
            Ok(outcome) => return Ok(outcome),
            Err(reason) => last_failure = reason,
        }
        if attempt < TIME_SYNC_MAX_ATTEMPTS {
            thread::sleep(Duration::from_millis(TIME_SYNC_RETRY_DELAY_MS));
        }
    }
    Err(anyhow!(
        "time sync did not verify after {TIME_SYNC_MAX_ATTEMPTS} attempts: {last_failure}"
    ))
}

/// Stringifies a transport-level failure so a single bad attempt can be
/// folded into a plain `String` reason instead of aborting the whole
/// readiness loop with an `anyhow::Error`.
fn to_reason(error: anyhow::Error) -> String {
    error.to_string()
}

/// One `TIMESET` attempt: sends the sampled host clock and parses the
/// immediate readback.
fn send_timeset(
    console: &mut SerialConsole,
    timeset_regex: &Regex,
    ack_timeout: Duration,
    utc_epoch_seconds: u32,
    offset_minutes: i16,
) -> Result<TimeSyncOutcome, String> {
    let command = format!("TIMESET {utc_epoch_seconds} {offset_minutes}");
    let line = console
        .command_wait_regex(&command, timeset_regex, ack_timeout)
        .map_err(to_reason)?
        .ok_or_else(|| "no TIMESET response".to_string())?;
    match parse_timeset_line(&line).map_err(to_reason)? {
        TimeSetResponse::Ok {
            utc_epoch_seconds,
            offset_minutes,
        } => Ok(TimeSyncOutcome {
            utc_epoch_seconds,
            offset_minutes,
        }),
        TimeSetResponse::Err { reason } => Err(format!("TIMESET ERR reason={reason}")),
    }
}

/// The delayed, verified `TIMEGET` half of one attempt: the readback must
/// show UTC has advanced past `readback`, the offset must match exactly,
/// and the readback must land within [`TIME_SYNC_TOLERANCE_SECS`] of
/// `host_utc_now`.
fn verify_timeget(
    console: &mut SerialConsole,
    timeget_regex: &Regex,
    ack_timeout: Duration,
    readback: TimeSyncOutcome,
    host_utc_now: u32,
) -> Result<TimeSyncOutcome, String> {
    let line = console
        .command_wait_regex("TIMEGET", timeget_regex, ack_timeout)
        .map_err(to_reason)?
        .ok_or_else(|| "no TIMEGET response".to_string())?;
    match parse_timeget_line(&line).map_err(to_reason)? {
        TimeGetResponse::ValidOn {
            utc_epoch_seconds,
            offset_minutes,
        } => {
            if utc_epoch_seconds <= readback.utc_epoch_seconds {
                return Err(format!(
                    "UTC did not advance: readback={} delayed={}",
                    readback.utc_epoch_seconds, utc_epoch_seconds
                ));
            }
            if offset_minutes != readback.offset_minutes {
                return Err(format!(
                    "offset mismatch: readback={} delayed={}",
                    readback.offset_minutes, offset_minutes
                ));
            }
            let drift = (i64::from(utc_epoch_seconds) - i64::from(host_utc_now)).abs();
            if drift > TIME_SYNC_TOLERANCE_SECS {
                return Err(format!(
                    "delayed readback drift {drift}s exceeds {TIME_SYNC_TOLERANCE_SECS}s tolerance"
                ));
            }
            Ok(readback)
        }
        TimeGetResponse::ValidOff { reason } => {
            Err(format!("TIMEGET OK valid=off reason={reason}"))
        }
        TimeGetResponse::Err { reason } => Err(format!("TIMEGET ERR reason={reason}")),
    }
}

/// One `TIMESET` + delayed, verified `TIMEGET` attempt. Returns the failure
/// reason as `Err(String)` (never a transport `anyhow::Error`, so a single
/// bad attempt cannot abort the whole readiness loop) except for outright
/// serial I/O failures, which propagate immediately.
fn attempt_time_sync(
    console: &mut SerialConsole,
    timeset_regex: &Regex,
    timeget_regex: &Regex,
    ack_timeout: Duration,
) -> Result<TimeSyncOutcome, String> {
    let (utc_epoch_seconds, offset_minutes) = sample_host_utc_and_offset().map_err(to_reason)?;
    let readback = send_timeset(
        console,
        timeset_regex,
        ack_timeout,
        utc_epoch_seconds,
        offset_minutes,
    )?;

    thread::sleep(Duration::from_millis(TIME_SYNC_ADVANCE_DELAY_MS));
    let (host_utc_now, _) = sample_host_utc_and_offset().map_err(to_reason)?;

    verify_timeget(console, timeget_regex, ack_timeout, readback, host_utc_now)
}

pub fn run_timeset(logger: &mut Logger, _opts: TimeSetOptions) -> Result<()> {
    let settle_ms = CONSOLE_SETTLE_MS;
    let (mut console, port, baud) = open_console(settle_ms, None)?;
    let outcome = sync_time(&mut console)?;
    logger.info(format!(
        "TIMESET OK utc={} offset_min={} -> {port} @ {baud}",
        outcome.utc_epoch_seconds, outcome.offset_minutes
    ));
    Ok(())
}

pub fn run_timestatus(logger: &mut Logger, _opts: TimeStatusOptions) -> Result<()> {
    let settle_ms = CONSOLE_SETTLE_MS;
    let (mut console, port, baud) = open_console(settle_ms, None)?;
    let regex = Regex::new(r"^TIMEGET (OK|ERR)")?;
    let line = console
        .command_wait_regex(
            "TIMEGET",
            &regex,
            Duration::from_millis(TIME_SYNC_ACK_TIMEOUT_MS),
        )?
        .ok_or_else(|| anyhow!("no TIMEGET response from {port} @ {baud}"))?;
    match parse_timeget_line(&line)? {
        TimeGetResponse::ValidOn {
            utc_epoch_seconds,
            offset_minutes,
        } => {
            logger.info(format!(
                "TIMEGET OK valid=on utc={utc_epoch_seconds} offset_min={offset_minutes}"
            ));
            Ok(())
        }
        TimeGetResponse::ValidOff { reason } => {
            logger.info(format!("TIMEGET OK valid=off reason={reason}"));
            Ok(())
        }
        TimeGetResponse::Err { reason } => Err(anyhow!("TIMEGET ERR reason={reason}")),
    }
}

fn open_console(
    settle_ms: u64,
    output_path: Option<PathBuf>,
) -> Result<(SerialConsole, String, u32)> {
    let port = env_utils::require_port()?;
    let baud = env_utils::baud_from_env(115200)?;
    let mut console = SerialConsole::open(&port, baud, output_path.as_deref())?;
    console.settle(settle_ms)?;
    Ok((console, port, baud))
}

pub fn run_repaint(logger: &mut Logger, opts: RepaintOptions) -> Result<()> {
    let settle_ms = CONSOLE_SETTLE_MS;
    let retries = REPAINT_RETRIES;
    let retry_delay_ms = REPAINT_RETRY_DELAY_MS;
    let wait_ack = true;
    let ack_timeout_ms = REPAINT_ACK_TIMEOUT_MS;
    let command = opts.command.unwrap_or_else(|| "REPAINT".to_string());
    let ack_tag = command
        .split_ascii_whitespace()
        .next()
        .ok_or_else(|| anyhow!("serial command must not be empty"))?;
    let output_path = None;

    if retries == 0 {
        return Err(anyhow!("serial retry count must be >= 1"));
    }

    let (mut console, port, baud) = open_console(settle_ms, output_path)?;
    let ack_ok = format!("{} OK", ack_tag);
    let ack_busy = format!("{} BUSY", ack_tag);

    for attempt in 1..=retries {
        let mark = console.mark();
        console.send_line(&command)?;
        if wait_ack {
            let (status, line) =
                console.wait_ack_since(mark, ack_tag, Duration::from_millis(ack_timeout_ms))?;
            if let Some(line) = line {
                if status == AckStatus::Ok && line.contains(&ack_ok) {
                    logger.info(format!(
                        "Sent ({attempt}x) with ACK: {command} -> {port} @ {baud}"
                    ));
                    return Ok(());
                }
                if status == AckStatus::Busy && line.contains(&ack_busy) {
                    thread::sleep(Duration::from_millis(retry_delay_ms));
                    continue;
                }
                if status == AckStatus::Err {
                    return Err(anyhow!("{command} failed: {line}"));
                }
            }
        }

        if attempt < retries {
            thread::sleep(Duration::from_millis(retry_delay_ms));
        }
    }

    if wait_ack {
        return Err(anyhow!(
            "No {command} ACK after {retries} attempts: {command} -> {port} @ {baud}"
        ));
    }

    logger.info(format!("Sent ({retries}x): {command} -> {port} @ {baud}"));
    Ok(())
}
#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    use anyhow::anyhow;
    use serialport::TTYPort;

    use super::*;

    fn open_pty_pair() -> Result<(TTYPort, TTYPort)> {
        TTYPort::pair().map_err(|err| anyhow!("TTYPort::pair failed: {err}"))
    }

    #[test]
    fn parses_timeset_ok_and_err_lines() {
        match parse_timeset_line("TIMESET OK utc=1762531200 offset_min=-300").unwrap() {
            TimeSetResponse::Ok {
                utc_epoch_seconds,
                offset_minutes,
            } => {
                assert_eq!(utc_epoch_seconds, 1_762_531_200);
                assert_eq!(offset_minutes, -300);
            }
            TimeSetResponse::Err { .. } => panic!("expected Ok"),
        }
        match parse_timeset_line("TIMESET ERR reason=clock_stopped").unwrap() {
            TimeSetResponse::Err { reason } => assert_eq!(reason, "clock_stopped"),
            TimeSetResponse::Ok { .. } => panic!("expected Err"),
        }
        assert!(parse_timeset_line("garbage").is_err());
    }

    #[test]
    fn parses_every_timeget_response_form() {
        match parse_timeget_line("TIMEGET OK valid=on utc=100 local=160 offset_min=60 os=clear")
            .unwrap()
        {
            TimeGetResponse::ValidOn {
                utc_epoch_seconds,
                offset_minutes,
            } => {
                assert_eq!(utc_epoch_seconds, 100);
                assert_eq!(offset_minutes, 60);
            }
            _ => panic!("expected ValidOn"),
        }
        match parse_timeget_line("TIMEGET OK valid=off reason=offset_unset").unwrap() {
            TimeGetResponse::ValidOff { reason } => assert_eq!(reason, "offset_unset"),
            _ => panic!("expected ValidOff"),
        }
        match parse_timeget_line("TIMEGET ERR reason=i2c").unwrap() {
            TimeGetResponse::Err { reason } => assert_eq!(reason, "i2c"),
            _ => panic!("expected Err"),
        }
        assert!(parse_timeget_line("garbage").is_err());
    }

    /// A minimal fake device: echoes `TIMESET <utc> <offset>` back as a
    /// verified readback, then answers `TIMEGET` with the host's real
    /// current time (so the advancement/tolerance checks pass without the
    /// test needing to predict clock skew) unless configured to answer
    /// something else first.
    fn spawn_fake_device(
        master: TTYPort,
        timeset_response: Option<String>,
        timeget_response: Option<String>,
    ) -> std::thread::JoinHandle<Result<()>> {
        std::thread::spawn(move || -> Result<()> {
            let mut master = master;
            let mut rx = Vec::<u8>::new();
            let mut chunk = [0u8; 256];
            let mut offset_reply: i64 = 0;
            let idle_deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                if std::time::Instant::now() > idle_deadline {
                    // The test either finished early (e.g. a fast-fail path
                    // that never sends TIMEGET) or something is wrong; either
                    // way, do not hang the test binary forever.
                    break;
                }
                let n = match master.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(err)
                        if matches!(
                            err.kind(),
                            std::io::ErrorKind::TimedOut
                                | std::io::ErrorKind::WouldBlock
                                | std::io::ErrorKind::Interrupted
                        ) =>
                    {
                        continue
                    }
                    Err(_) => break,
                };
                rx.extend_from_slice(&chunk[..n]);
                while let Some(pos) = rx.iter().position(|byte| *byte == b'\n') {
                    let mut line: Vec<u8> = rx.drain(..=pos).collect();
                    while matches!(line.last(), Some(b'\r' | b'\n')) {
                        line.pop();
                    }
                    let command = String::from_utf8_lossy(&line).trim().to_string();
                    if command.is_empty() {
                        continue;
                    }
                    if let Some(rest) = command.strip_prefix("TIMESET ") {
                        let mut parts = rest.split_ascii_whitespace();
                        let utc: i64 = parts
                            .next()
                            .ok_or_else(|| anyhow!("missing utc"))?
                            .parse()?;
                        let offset: i64 = parts
                            .next()
                            .ok_or_else(|| anyhow!("missing offset"))?
                            .parse()?;
                        offset_reply = offset;
                        let response = timeset_response
                            .clone()
                            .unwrap_or_else(|| format!("TIMESET OK utc={utc} offset_min={offset}"));
                        let rejected = response.contains("ERR");
                        master.write_all(response.as_bytes())?;
                        master.write_all(b"\r\n")?;
                        master.flush()?;
                        if rejected {
                            // The caller bails out on a TIMESET rejection
                            // without ever sending TIMEGET; nothing more to
                            // answer. Give the reader a moment to drain the
                            // response before this end of the PTY closes --
                            // closing immediately can race the still-pending
                            // read on some platforms.
                            std::thread::sleep(Duration::from_millis(200));
                            return Ok(());
                        }
                    } else if command == "TIMEGET" {
                        let response = match &timeget_response {
                            Some(fixed) => fixed.clone(),
                            None => {
                                let now = chrono::Local::now().timestamp();
                                let local = now + offset_reply * 60;
                                format!(
                                    "TIMEGET OK valid=on utc={now} local={local} offset_min={offset_reply} os=clear"
                                )
                            }
                        };
                        master.write_all(response.as_bytes())?;
                        master.write_all(b"\r\n")?;
                        master.flush()?;
                        break;
                    }
                }
            }
            Ok(())
        })
    }

    #[test]
    fn attempt_time_sync_succeeds_when_delayed_readback_advances_and_matches() -> Result<()> {
        let (master, slave) = open_pty_pair()?;
        let responder = spawn_fake_device(master, None, None);
        let mut console = SerialConsole::from_port_for_tests(Box::new(slave), None)?;
        let timeset_regex = Regex::new(r"^TIMESET (OK|ERR)")?;
        let timeget_regex = Regex::new(r"^TIMEGET (OK|ERR)")?;

        let outcome = attempt_time_sync(
            &mut console,
            &timeset_regex,
            &timeget_regex,
            Duration::from_millis(TIME_SYNC_ACK_TIMEOUT_MS),
        )
        .map_err(|reason| anyhow!(reason))?;

        let (host_utc, host_offset) = sample_host_utc_and_offset()?;
        assert_eq!(outcome.offset_minutes, host_offset);
        // The fake device echoed the sampled host UTC straight back.
        assert!((i64::from(outcome.utc_epoch_seconds) - i64::from(host_utc)).abs() <= 2);
        responder
            .join()
            .map_err(|_| anyhow!("fake device thread panicked"))??;
        Ok(())
    }

    #[test]
    fn attempt_time_sync_fails_fast_on_timeset_err_without_sleeping() -> Result<()> {
        let (master, slave) = open_pty_pair()?;
        let responder =
            spawn_fake_device(master, Some("TIMESET ERR reason=range".to_string()), None);
        let mut console = SerialConsole::from_port_for_tests(Box::new(slave), None)?;
        let timeset_regex = Regex::new(r"^TIMESET (OK|ERR)")?;
        let timeget_regex = Regex::new(r"^TIMEGET (OK|ERR)")?;

        let started = std::time::Instant::now();
        let result = attempt_time_sync(
            &mut console,
            &timeset_regex,
            &timeget_regex,
            Duration::from_millis(TIME_SYNC_ACK_TIMEOUT_MS),
        );
        assert_eq!(result, Err("TIMESET ERR reason=range".to_string()));
        // A TIMESET-level rejection must not pay the delayed-advancement sleep.
        assert!(started.elapsed() < Duration::from_millis(TIME_SYNC_ADVANCE_DELAY_MS));
        responder
            .join()
            .map_err(|_| anyhow!("fake device thread panicked"))??;
        Ok(())
    }

    #[test]
    fn attempt_time_sync_rejects_a_readback_that_does_not_advance() -> Result<()> {
        let (master, slave) = open_pty_pair()?;
        let fixed_timeget = "TIMEGET OK valid=on utc=1000 local=1000 offset_min=0 os=clear";
        let responder = spawn_fake_device(
            master,
            Some("TIMESET OK utc=1000 offset_min=0".to_string()),
            Some(fixed_timeget.to_string()),
        );
        let mut console = SerialConsole::from_port_for_tests(Box::new(slave), None)?;
        let timeset_regex = Regex::new(r"^TIMESET (OK|ERR)")?;
        let timeget_regex = Regex::new(r"^TIMEGET (OK|ERR)")?;

        let result = attempt_time_sync(
            &mut console,
            &timeset_regex,
            &timeget_regex,
            Duration::from_millis(TIME_SYNC_ACK_TIMEOUT_MS),
        );
        let Err(reason) = result else {
            panic!("expected a did-not-advance failure");
        };
        assert!(reason.contains("did not advance"), "reason was: {reason}");
        responder
            .join()
            .map_err(|_| anyhow!("fake device thread panicked"))??;
        Ok(())
    }
}
