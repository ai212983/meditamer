//! ASCII line codec for the wall-clock session protocol -- independent of
//! any transport (UART, USB-Serial-JTAG, ...). Wire grammar:
//!
//! ```text
//! TIME_REQUEST version=<u8> session=<u32> nonce=<u32> reason=<label> deadline_ms=<u32>
//! TIME_REPLY version=<u8> session=<u32> nonce=<u32> utc=<u32> offset_min=<i16>
//! TIME_SYNC OK version=<u8> session=<u32> utc=<u32> offset_min=<i16>
//! TIME_SYNC ERR version=<u8> session=<u32> reason=<label>
//! ```
//!
//! Encoders write into any `core::fmt::Write` sink with no trailing `\r\n`
//! -- callers append their own line ending, matching the rest of the serial
//! wire protocol (`src/firmware/serial/time_dispatch.rs`). Decoders parse a
//! `&str` and return `None` on anything malformed rather than panicking.

use core::fmt::Write;

use crate::message::{
    SyncReply, SyncRequest, SyncResult, SyncStatus, TriggerReason, VerifyErrorReason,
};

pub fn encode_request(request: &SyncRequest, out: &mut impl Write) -> core::fmt::Result {
    write!(
        out,
        "TIME_REQUEST version={} session={} nonce={} reason={} deadline_ms={}",
        request.version,
        request.session,
        request.nonce,
        request.trigger.label(),
        request.deadline_ms,
    )
}

pub fn encode_reply(reply: &SyncReply, out: &mut impl Write) -> core::fmt::Result {
    write!(
        out,
        "TIME_REPLY version={} session={} nonce={} utc={} offset_min={}",
        reply.version, reply.session, reply.nonce, reply.utc_epoch_seconds, reply.offset_minutes,
    )
}

pub fn encode_result(result: &SyncResult, out: &mut impl Write) -> core::fmt::Result {
    match result.status {
        SyncStatus::Ok => {
            // `verified` is always `Some` when `status` is `Ok` by
            // construction (`Coordinator::finish`'s only `Ok` call site);
            // `unwrap_or` is a defensive fallback, not an expected path --
            // embedded encoders never panic on a shape mismatch.
            let (utc_epoch_seconds, offset_minutes) = result.verified.unwrap_or((0, 0));
            write!(
                out,
                "TIME_SYNC OK version={} session={} utc={} offset_min={}",
                result.version, result.session, utc_epoch_seconds, offset_minutes,
            )
        }
        SyncStatus::Err => {
            let reason = result
                .reason
                .map(|reason| reason.label())
                .unwrap_or("unknown");
            write!(
                out,
                "TIME_SYNC ERR version={} session={} reason={}",
                result.version, result.session, reason,
            )
        }
    }
}

pub fn decode_request(line: &str) -> Option<SyncRequest> {
    let line = line.trim();
    if !line.starts_with("TIME_REQUEST") || token_count(line) != 6 {
        return None;
    }
    Some(SyncRequest {
        version: parse_u8(find_value(line, "version")?)?,
        session: parse_u32(find_value(line, "session")?)?,
        nonce: parse_u32(find_value(line, "nonce")?)?,
        trigger: TriggerReason::from_label(find_value(line, "reason")?)?,
        deadline_ms: parse_u32(find_value(line, "deadline_ms")?)?,
    })
}

pub fn decode_reply(line: &str) -> Option<SyncReply> {
    let line = line.trim();
    if !line.starts_with("TIME_REPLY") || token_count(line) != 6 {
        return None;
    }
    Some(SyncReply {
        version: parse_u8(find_value(line, "version")?)?,
        session: parse_u32(find_value(line, "session")?)?,
        nonce: parse_u32(find_value(line, "nonce")?)?,
        utc_epoch_seconds: parse_u32(find_value(line, "utc")?)?,
        offset_minutes: parse_i16(find_value(line, "offset_min")?)?,
    })
}

pub fn decode_result(line: &str) -> Option<SyncResult> {
    let rest = line.trim().strip_prefix("TIME_SYNC ")?;
    if rest.starts_with("OK") {
        // `rest` no longer includes the `TIME_SYNC` tag (already stripped
        // above): `OK` + version/session/utc/offset_min = 5 tokens.
        if token_count(rest) != 5 {
            return None;
        }
        Some(SyncResult {
            version: parse_u8(find_value(rest, "version")?)?,
            session: parse_u32(find_value(rest, "session")?)?,
            status: SyncStatus::Ok,
            verified: Some((
                parse_u32(find_value(rest, "utc")?)?,
                parse_i16(find_value(rest, "offset_min")?)?,
            )),
            reason: None,
        })
    } else if rest.starts_with("ERR") {
        // `ERR` + version/session/reason = 4 tokens.
        if token_count(rest) != 4 {
            return None;
        }
        Some(SyncResult {
            version: parse_u8(find_value(rest, "version")?)?,
            session: parse_u32(find_value(rest, "session")?)?,
            status: SyncStatus::Err,
            verified: None,
            reason: Some(VerifyErrorReason::from_label(find_value(rest, "reason")?)),
        })
    } else {
        None
    }
}

/// Finds `key=value` among whitespace-separated tokens and returns `value`.
fn find_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split_ascii_whitespace().find_map(|token| {
        let (found_key, value) = token.split_once('=')?;
        (found_key == key).then_some(value)
    })
}

/// Counts whitespace-separated tokens. Every decoder pairs this with an
/// exact expected count, so trailing garbage, a duplicated field, or bytes
/// from a concurrent writer interleaved into the same line (a documented
/// real risk on a shared UART -- see
/// `tools/hostctl/src/serial_console/mod.rs`'s `wait_ack_since`) is rejected
/// as malformed instead of silently accepted alongside the fields
/// `find_value` did manage to find.
fn token_count(line: &str) -> usize {
    line.split_ascii_whitespace().count()
}

fn parse_u8(value: &str) -> Option<u8> {
    value.parse().ok()
}

fn parse_u32(value: &str) -> Option<u32> {
    value.parse().ok()
}

fn parse_i16(value: &str) -> Option<i16> {
    value.parse().ok()
}
