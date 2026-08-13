use super::super::util::{parse_u64_ascii, trim_ascii_whitespace};
use crate::firmware::scheduling::SchedulerProfile;
use crate::firmware::serial::commands::SchedulerOperation;

pub(in super::super) fn parse_repaint_command(line: &[u8]) -> bool {
    let cmd = trim_ascii_whitespace(line);
    cmd == b"REPAINT" || cmd == b"REFRESH"
}

pub(in super::super) fn parse_metrics_command(line: &[u8]) -> bool {
    let cmd = trim_ascii_whitespace(line);
    cmd == b"METRICS" || cmd == b"PERF"
}

pub(in super::super) fn parse_touch_sched_reset_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line) == b"TOUCHSCHEDRESET"
}

pub(in super::super) fn parse_scheduler_command(line: &[u8]) -> Option<SchedulerOperation> {
    let trimmed = trim_ascii_whitespace(line);
    if trimmed.eq_ignore_ascii_case(b"SCHEDPROFILE") {
        return Some(SchedulerOperation::Status);
    }
    let value = trim_ascii_whitespace(trimmed.strip_prefix(b"SCHEDPROFILE ")?);
    if value.eq_ignore_ascii_case(b"AUTO") {
        Some(SchedulerOperation::Automatic)
    } else if value.eq_ignore_ascii_case(b"INTERACTIVE") {
        Some(SchedulerOperation::Profile(SchedulerProfile::Interactive))
    } else if value.eq_ignore_ascii_case(b"UPLOAD") {
        Some(SchedulerOperation::Profile(SchedulerProfile::Upload))
    } else if value.eq_ignore_ascii_case(b"DIAGNOSTICS") {
        Some(SchedulerOperation::Profile(SchedulerProfile::Diagnostics))
    } else {
        None
    }
}

pub(in super::super) fn parse_metrics_net_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line) == b"METRICSNET"
}

pub(in super::super) fn parse_ping_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line) == b"PING"
}

pub(in super::super) fn parse_ui_cycle_step_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line) == b"UISTEP"
}

#[cfg(feature = "ble-foundation")]
pub(in super::super) fn parse_ble_probe_start_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line).eq_ignore_ascii_case(b"BLEPROBE START")
}

#[cfg(feature = "ble-foundation")]
pub(in super::super) fn parse_ble_probe_status_command(line: &[u8]) -> bool {
    let command = trim_ascii_whitespace(line);
    command.eq_ignore_ascii_case(b"BLEPROBE") || command.eq_ignore_ascii_case(b"BLEPROBE STATUS")
}

#[cfg(feature = "ui-provider-fixture")]
pub(in super::super) fn parse_ui_provider_fixture_step_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line) == b"UIFIXTURE"
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

pub(in super::super) fn parse_sdprobe_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line) == b"SDPROBE"
}
