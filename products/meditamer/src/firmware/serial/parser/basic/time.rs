use super::super::util::trim_ascii_whitespace;

/// `TIME_REPLY ...` -- decoded through the shared `wall_clock` codec
/// (SER-01) rather than a bespoke parser, so the wire grammar has exactly
/// one implementation across every board and hostctl.
/// `docs/references/runtime/serial-control.md#time-synchronization`.
pub(in super::super) fn parse_time_reply_command(
    line: &[u8],
) -> Option<wall_clock::message::SyncReply> {
    let text = core::str::from_utf8(line).ok()?;
    wall_clock::codec::decode_reply(text)
}

/// `TIMESYNC` -- asks the device to open a fresh manual-demand
/// synchronization session.
pub(in super::super) fn parse_timesync_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line) == b"TIMESYNC"
}

pub(in super::super) fn parse_timeget_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line) == b"TIMEGET"
}
