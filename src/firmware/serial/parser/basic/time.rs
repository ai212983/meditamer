use super::super::util::{parse_i64_ascii, parse_u64_ascii, trim_ascii_whitespace};

/// `TIMESET <utc_epoch_seconds> <offset_minutes>`. Only syntactic/range
/// bounds a `u32`/`i16` pair can hold are checked here; domain validation
/// (representable calendar window, offset granularity) happens in the RTC
/// driver so the wire protocol's `TIMESET ERR reason=<range|offset>` stays
/// authoritative in one place.
pub(in super::super) fn parse_timeset_command(line: &[u8]) -> Option<(u32, i16)> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"TIMESET";
    if !trimmed.starts_with(cmd) {
        return None;
    }

    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let (utc_epoch_seconds, next_i) = parse_u64_ascii(trimmed, i)?;
    if utc_epoch_seconds > u32::MAX as u64 {
        return None;
    }
    i = next_i;

    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let (offset_minutes, next_i) = parse_i64_ascii(trimmed, i)?;
    if offset_minutes < i16::MIN as i64 || offset_minutes > i16::MAX as i64 {
        return None;
    }
    i = next_i;

    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i != trimmed.len() {
        return None;
    }

    Some((utc_epoch_seconds as u32, offset_minutes as i16))
}

pub(in super::super) fn parse_timeget_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line) == b"TIMEGET"
}
