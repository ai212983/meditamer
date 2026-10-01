use std::{path::PathBuf, thread, time::Duration};

use crate::{
    env_utils,
    logging::Logger,
    serial_console::{AckStatus, SerialConsole},
};
use anyhow::{anyhow, Result};
use regex::Regex;
use wall_clock::message::{SyncReply, SyncStatus, PROTOCOL_VERSION};

pub struct RepaintOptions {
    pub command: Option<String>,
}

pub struct TimeSetOptions {}

pub struct TimeStatusOptions {}

/// A completed session: the host's `TIME_REPLY` sample (`requested_*`) and
/// the device's verified `TIME_SYNC OK` result (`verified_*`) -- these can
/// legitimately differ by the second or two the device's own delayed
/// verification takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeSyncOutcome {
    pub requested_utc_epoch_seconds: u32,
    pub requested_offset_minutes: i16,
    pub verified_utc_epoch_seconds: u32,
    pub verified_offset_minutes: i16,
}

/// How long the interactive `hostctl timeset` CLI waits, after sending
/// `TIMESYNC`, for the device to emit its `TIME_REQUEST`.
const TIMESYNC_REQUEST_WAIT_MS: u64 = 5_000;

/// How long `sync_time` waits for a live `TIME_REQUEST` when no
/// already-captured one was supplied (the flash-capture workflow's
/// `capture_mode: "none"` path, or any other caller with nothing captured).
/// Matches the device side's generous cold-boot session deadlines --
/// nothing is watching, so waiting costs nothing but wall time.
pub const NO_CAPTURED_REQUEST_WAIT_MS: u64 = 15_000;

/// Margin added on top of the shared `wall_clock::rtc_backend::VERIFY_DELAY_MS`
/// when bounding one attempt's wait for the device's `TIME_SYNC OK|ERR`.
/// Once a reply is accepted, the device's own delayed verification always
/// takes `VERIFY_DELAY_MS` plus a bit, independent of the session's own
/// (often much larger) `deadline_ms` -- bounding each attempt this way,
/// rather than by `deadline_ms`, is what makes a short bounded retry loop
/// possible at all.
const RESULT_WAIT_MARGIN_MS: u64 = 2_000;

/// One attempt's wait for `TIME_SYNC OK|ERR` after sending `TIME_REPLY`.
const RESULT_WAIT_MS: u64 = wall_clock::rtc_backend::VERIFY_DELAY_MS as u64 + RESULT_WAIT_MARGIN_MS;

/// How many times `complete_sync` resends `TIME_REPLY` if no `TIME_SYNC`
/// response arrives. Safe to retry: a duplicate reply for a session the
/// device already accepted is silently ignored on the device side
/// (`AlreadyClaimed`), so a retry can only recover a reply the device never
/// actually saw -- never double-apply one it did.
const REPLY_ATTEMPTS: u32 = 3;
const REPLY_RETRY_DELAY_MS: u64 = 500;

/// The host resamples its own clock after the device's `TIME_SYNC OK`
/// verified result and checks the final result against its then-current
/// time -- this is that tolerance ("the host samples its clock after
/// receiving the request and checks the final result against its
/// then-current time", architecture doc).
const TIME_SYNC_TOLERANCE_SECS: i64 = 2;

/// Acknowledgement timeout for the plain, session-less `TIMEGET` read.
const TIMEGET_ACK_TIMEOUT_MS: u64 = 1_200;

/// Console settle time after opening the port. Shared by timeset/timestatus/
/// repaint/flash-capture post-command/time-sync; formerly five separate
/// `HOSTCTL_*_SETTLE_MS` env knobs, all default 200 (hostctl-env-audit.md).
pub(crate) const CONSOLE_SETTLE_MS: u64 = 200;

const REPAINT_RETRIES: u32 = 2;
const REPAINT_RETRY_DELAY_MS: u64 = 500;
const REPAINT_ACK_TIMEOUT_MS: u64 = 15_000;

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
        .map_err(|_| anyhow!("host clock is outside the TIME_REPLY u32 epoch range"))?;
    let offset_minutes = i16::try_from(local_now.offset().local_minus_utc() / 60)
        .map_err(|_| anyhow!("host UTC offset does not fit in the TIME_REPLY offset range"))?;
    Ok((utc_epoch_seconds, offset_minutes))
}

/// Waits (up to `wait_ms`) for a live `TIME_REQUEST` line on `console` and
/// decodes it.
fn discover_request(
    console: &mut SerialConsole,
    wait_ms: u64,
) -> Result<wall_clock::message::SyncRequest> {
    let mark = console.mark();
    let regex = Regex::new(r"^TIME_REQUEST\b")?;
    let line = console
        .wait_for_regex_since(mark, &regex, Duration::from_millis(wait_ms))?
        .ok_or_else(|| anyhow!("no TIME_REQUEST seen within {wait_ms}ms"))?;
    wall_clock::codec::decode_request(&line)
        .ok_or_else(|| anyhow!("malformed TIME_REQUEST: {line}"))
}

/// One `TIME_REPLY` attempt's outcome. `Final` means the device answered
/// conclusively (success, a `TIME_SYNC ERR`, or an out-of-tolerance
/// verified drift) -- retrying would only hit `WrongState` against an
/// already-closed session, so `complete_sync` returns it immediately.
/// `Retry` means no conclusive answer arrived at all (a transport failure,
/// a timeout, or a line that didn't parse) -- the device may simply never
/// have seen this attempt's `TIME_REPLY`, so trying again can help.
enum AttemptResult {
    Final(Result<TimeSyncOutcome>),
    Retry(anyhow::Error),
}

/// Replies to an already-known, still-open `request` and waits for the
/// device's verified result, retrying the reply itself (never the already-
/// closed session) up to [`REPLY_ATTEMPTS`] times against transport-level
/// failures.
fn complete_sync(
    console: &mut SerialConsole,
    request: &wall_clock::message::SyncRequest,
) -> Result<TimeSyncOutcome> {
    let mut last_error = anyhow!("no reply attempts were made");
    for attempt in 1..=REPLY_ATTEMPTS {
        match attempt_reply(console, request) {
            AttemptResult::Final(result) => return result,
            AttemptResult::Retry(error) => last_error = error,
        }
        if attempt < REPLY_ATTEMPTS {
            thread::sleep(Duration::from_millis(REPLY_RETRY_DELAY_MS));
        }
    }
    Err(last_error)
}

/// One `TIME_REPLY` send plus a bounded wait for `TIME_SYNC OK|ERR`.
fn attempt_reply(
    console: &mut SerialConsole,
    request: &wall_clock::message::SyncRequest,
) -> AttemptResult {
    let (requested_utc_epoch_seconds, requested_offset_minutes) = match sample_host_utc_and_offset()
    {
        Ok(pair) => pair,
        Err(error) => return AttemptResult::Final(Err(error)),
    };
    let reply = SyncReply {
        version: PROTOCOL_VERSION,
        session: request.session,
        nonce: request.nonce,
        utc_epoch_seconds: requested_utc_epoch_seconds,
        offset_minutes: requested_offset_minutes,
    };
    let mut reply_line = String::new();
    if let Err(error) = wall_clock::codec::encode_reply(&reply, &mut reply_line) {
        return AttemptResult::Final(Err(error.into()));
    }
    let reply_mark = console.mark();
    if let Err(error) = console.send_line(&reply_line) {
        return AttemptResult::Retry(error);
    }

    let result_regex = match Regex::new(r"^TIME_SYNC (OK|ERR)") {
        Ok(regex) => regex,
        Err(error) => return AttemptResult::Final(Err(error.into())),
    };
    let result_line = match console.wait_for_regex_since(
        reply_mark,
        &result_regex,
        Duration::from_millis(RESULT_WAIT_MS),
    ) {
        Ok(Some(line)) => line,
        Ok(None) => {
            return AttemptResult::Retry(anyhow!("no TIME_SYNC response within {RESULT_WAIT_MS}ms"))
        }
        Err(error) => return AttemptResult::Retry(error),
    };
    let Some(result) = wall_clock::codec::decode_result(&result_line) else {
        return AttemptResult::Retry(anyhow!("malformed TIME_SYNC response: {result_line}"));
    };

    match result.status {
        SyncStatus::Ok => {
            let Some((verified_utc_epoch_seconds, verified_offset_minutes)) = result.verified
            else {
                return AttemptResult::Final(Err(anyhow!(
                    "TIME_SYNC OK carried no verified value"
                )));
            };
            let host_utc_now = match sample_host_utc_and_offset() {
                Ok((utc, _)) => utc,
                Err(error) => return AttemptResult::Final(Err(error)),
            };
            let drift = (i64::from(verified_utc_epoch_seconds) - i64::from(host_utc_now)).abs();
            if drift > TIME_SYNC_TOLERANCE_SECS {
                return AttemptResult::Final(Err(anyhow!(
                    "verified drift {drift}s exceeds {TIME_SYNC_TOLERANCE_SECS}s tolerance"
                )));
            }
            AttemptResult::Final(Ok(TimeSyncOutcome {
                requested_utc_epoch_seconds,
                requested_offset_minutes,
                verified_utc_epoch_seconds,
                verified_offset_minutes,
            }))
        }
        SyncStatus::Err => {
            let reason = result
                .reason
                .map(|reason| reason.label())
                .unwrap_or("unknown");
            AttemptResult::Final(Err(anyhow!("TIME_SYNC ERR reason={reason}")))
        }
    }
}

/// Completes one wall-clock synchronization session against an already open
/// console: either replies to `known_request` (a `TIME_REQUEST` some
/// earlier capture already observed -- see
/// `workflows::flash_capture::capture::find_captured_request`), or, absent
/// one, waits up to `discover_wait_ms` for a live `TIME_REQUEST` first.
///
/// Shared by the `hostctl timeset` CLI command and the flash-capture
/// workflow's `time_sync` action.
pub fn sync_time(
    console: &mut SerialConsole,
    known_request: Option<wall_clock::message::SyncRequest>,
    discover_wait_ms: u64,
) -> Result<TimeSyncOutcome> {
    let request = match known_request {
        Some(request) => request,
        None => discover_request(console, discover_wait_ms)?,
    };
    complete_sync(console, &request)
}

pub fn run_timeset(logger: &mut Logger, _opts: TimeSetOptions) -> Result<()> {
    let settle_ms = CONSOLE_SETTLE_MS;
    let (mut console, port, baud) = open_console(settle_ms, None)?;
    console.send_line("TIMESYNC")?;
    let outcome = sync_time(&mut console, None, TIMESYNC_REQUEST_WAIT_MS)?;
    logger.info(format!(
        "TIMESET OK requested_utc={} requested_offset_min={} verified_utc={} verified_offset_min={} -> {port} @ {baud}",
        outcome.requested_utc_epoch_seconds,
        outcome.requested_offset_minutes,
        outcome.verified_utc_epoch_seconds,
        outcome.verified_offset_minutes,
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
            Duration::from_millis(TIMEGET_ACK_TIMEOUT_MS),
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

    /// A minimal fake device: on `TIME_REQUEST` (either printed unprompted
    /// when `send_time_request` is set, matching cold boot, or in reply to a
    /// `TIMESYNC` command otherwise) waits for the matching `TIME_REPLY` and
    /// answers with `TIME_SYNC OK` echoing the host's real current time
    /// (advanced by one second, so the drift/advance checks pass without the
    /// test needing to predict clock skew) unless configured to answer
    /// something else.
    fn spawn_fake_device(
        master: TTYPort,
        session: u32,
        nonce: u32,
        send_time_request: bool,
        fixed_result: Option<String>,
    ) -> std::thread::JoinHandle<Result<()>> {
        std::thread::spawn(move || -> Result<()> {
            let mut master = master;
            if send_time_request {
                let line = format!(
                    "TIME_REQUEST version=1 session={session} nonce={nonce} reason=invalid_cold_boot deadline_ms=5000\r\n"
                );
                master.write_all(line.as_bytes())?;
                master.flush()?;
            }

            let mut rx = Vec::<u8>::new();
            let mut chunk = [0u8; 256];
            let idle_deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                if std::time::Instant::now() > idle_deadline {
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
                    if command == "TIMESYNC" {
                        let line = format!(
                            "TIME_REQUEST version=1 session={session} nonce={nonce} reason=manual_demand deadline_ms=5000\r\n"
                        );
                        master.write_all(line.as_bytes())?;
                        master.flush()?;
                    } else if command.starts_with("TIME_REPLY") {
                        let response = match &fixed_result {
                            Some(fixed) => fixed.clone(),
                            None => {
                                let now = chrono::Local::now().timestamp() + 1;
                                let offset = chrono::Local::now().offset().local_minus_utc() / 60;
                                format!(
                                    "TIME_SYNC OK version=1 session={session} utc={now} offset_min={offset}"
                                )
                            }
                        };
                        master.write_all(response.as_bytes())?;
                        master.write_all(b"\r\n")?;
                        master.flush()?;
                        // Keep `master` open past the response: dropping it
                        // here would close the PTY out from under the
                        // console's still-in-flight read loop on the other
                        // end (it may poll once more even after this reply
                        // already satisfied its match), turning a harmless
                        // "nothing more to read" into a hard broken-pipe
                        // error. Let the idle deadline (or the test's own
                        // `console` dropping first) end this thread instead.
                    }
                }
            }
            Ok(())
        })
    }

    #[test]
    fn sync_time_succeeds_against_a_captured_request() -> Result<()> {
        let (master, slave) = open_pty_pair()?;
        let responder = spawn_fake_device(master, 7, 42, false, None);
        let mut console = SerialConsole::from_port_for_tests(Box::new(slave), None)?;

        let request = wall_clock::message::SyncRequest {
            version: PROTOCOL_VERSION,
            session: 7,
            nonce: 42,
            trigger: wall_clock::message::TriggerReason::InvalidColdBoot,
            deadline_ms: 5_000,
        };
        let outcome = sync_time(&mut console, Some(request), 0)?;

        let (host_utc, host_offset) = sample_host_utc_and_offset()?;
        assert_eq!(outcome.requested_offset_minutes, host_offset);
        assert!((i64::from(outcome.requested_utc_epoch_seconds) - i64::from(host_utc)).abs() <= 2);
        assert!(outcome.verified_utc_epoch_seconds > outcome.requested_utc_epoch_seconds);
        responder
            .join()
            .map_err(|_| anyhow!("fake device thread panicked"))??;
        Ok(())
    }

    #[test]
    fn sync_time_discovers_a_live_time_request_when_none_is_known() -> Result<()> {
        let (master, slave) = open_pty_pair()?;
        let responder = spawn_fake_device(master, 3, 9, true, None);
        let mut console = SerialConsole::from_port_for_tests(Box::new(slave), None)?;

        let outcome = sync_time(&mut console, None, 2_000)?;
        assert!(outcome.verified_utc_epoch_seconds > outcome.requested_utc_epoch_seconds);
        responder
            .join()
            .map_err(|_| anyhow!("fake device thread panicked"))??;
        Ok(())
    }

    #[test]
    fn sync_time_fails_on_a_time_sync_err() -> Result<()> {
        let (master, slave) = open_pty_pair()?;
        let responder = spawn_fake_device(
            master,
            1,
            1,
            false,
            Some("TIME_SYNC ERR version=1 session=1 reason=not_advanced".to_string()),
        );
        let mut console = SerialConsole::from_port_for_tests(Box::new(slave), None)?;

        let request = wall_clock::message::SyncRequest {
            version: PROTOCOL_VERSION,
            session: 1,
            nonce: 1,
            trigger: wall_clock::message::TriggerReason::ManualDemand,
            deadline_ms: 5_000,
        };
        let result = sync_time(&mut console, Some(request), 0);
        let Err(error) = result else {
            panic!("expected a TIME_SYNC ERR failure");
        };
        assert!(
            error.to_string().contains("not_advanced"),
            "error was: {error}"
        );
        responder
            .join()
            .map_err(|_| anyhow!("fake device thread panicked"))??;
        Ok(())
    }

    #[test]
    fn sync_time_rejects_a_verified_result_that_drifts_too_far() -> Result<()> {
        let (master, slave) = open_pty_pair()?;
        let responder = spawn_fake_device(
            master,
            2,
            2,
            false,
            Some("TIME_SYNC OK version=1 session=2 utc=1000 offset_min=0".to_string()),
        );
        let mut console = SerialConsole::from_port_for_tests(Box::new(slave), None)?;

        let request = wall_clock::message::SyncRequest {
            version: PROTOCOL_VERSION,
            session: 2,
            nonce: 2,
            trigger: wall_clock::message::TriggerReason::ManualDemand,
            deadline_ms: 5_000,
        };
        let result = sync_time(&mut console, Some(request), 0);
        let Err(error) = result else {
            panic!("expected a drift-tolerance failure");
        };
        assert!(error.to_string().contains("drift"), "error was: {error}");
        responder
            .join()
            .map_err(|_| anyhow!("fake device thread panicked"))??;
        Ok(())
    }
}
