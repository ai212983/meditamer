use crate::firmware::types::TimeSyncCommand;

use super::super::util::{find_subslice, parse_i32_ascii, parse_u64_ascii, trim_ascii_whitespace};

pub(in super::super) fn parse_timeset_command(line: &[u8]) -> Option<TimeSyncCommand> {
    let cmd_idx = find_subslice(line, b"TIMESET")?;
    let mut i = cmd_idx + b"TIMESET".len();
    let len = line.len();

    while i < len && line[i].is_ascii_whitespace() {
        i += 1;
    }
    let (unix_epoch_utc_seconds, next_i) = parse_u64_ascii(line, i)?;
    i = next_i;
    while i < len && line[i].is_ascii_whitespace() {
        i += 1;
    }
    let (tz_offset_minutes, next_i) = parse_i32_ascii(line, i)?;
    i = next_i;
    while i < len && line[i].is_ascii_whitespace() {
        i += 1;
    }
    if i != len {
        return None;
    }
    if !(-720..=840).contains(&tz_offset_minutes) {
        return None;
    }

    Some(TimeSyncCommand {
        unix_epoch_utc_seconds,
        tz_offset_minutes,
    })
}

pub(in super::super) fn parse_repaint_command(line: &[u8]) -> bool {
    let cmd = trim_ascii_whitespace(line);
    cmd == b"REPAINT" || cmd == b"REFRESH"
}

pub(in super::super) fn parse_repaint_marble_command(line: &[u8]) -> bool {
    let cmd = trim_ascii_whitespace(line);
    cmd == b"REPAINT_MARBLE" || cmd == b"MARBLE"
}

pub(in super::super) fn parse_metrics_command(line: &[u8]) -> bool {
    let cmd = trim_ascii_whitespace(line);
    cmd == b"METRICS" || cmd == b"PERF"
}

pub(in super::super) fn parse_metrics_net_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line) == b"METRICSNET"
}

pub(in super::super) fn parse_ping_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line) == b"PING"
}

pub(in super::super) fn parse_allocator_status_command(line: &[u8]) -> bool {
    let cmd = trim_ascii_whitespace(line);
    cmd == b"PSRAM" || cmd == b"ALLOCATOR" || cmd == b"HEAP"
}

pub(in super::super) fn parse_allocator_alloc_probe_command(line: &[u8]) -> Option<u32> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = if trimmed.starts_with(b"PSRAMALLOC") {
        b"PSRAMALLOC".as_slice()
    } else if trimmed.starts_with(b"HEAPALLOC") {
        b"HEAPALLOC".as_slice()
    } else {
        return None;
    };

    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i == trimmed.len() {
        return None;
    }
    let (bytes, next_i) = parse_u64_ascii(trimmed, i)?;
    if bytes > u32::MAX as u64 {
        return None;
    }
    i = next_i;
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i != trimmed.len() {
        return None;
    }
    Some(bytes as u32)
}

pub(in super::super) fn parse_touch_wizard_command(line: &[u8]) -> bool {
    let cmd = trim_ascii_whitespace(line);
    cmd == b"TOUCH_WIZARD" || cmd == b"TOUCH_CAL" || cmd == b"CAL_TOUCH"
}

pub(in super::super) fn parse_touch_wizard_dump_command(line: &[u8]) -> bool {
    let cmd = trim_ascii_whitespace(line);
    cmd == b"TOUCH_WIZARD_DUMP" || cmd == b"TOUCH_DUMP" || cmd == b"WIZARD_DUMP"
}

pub(in super::super) fn parse_sdprobe_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line) == b"SDPROBE"
}
