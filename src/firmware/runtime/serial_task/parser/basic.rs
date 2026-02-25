use crate::firmware::types::RuntimeMode;
use crate::firmware::types::TimeSyncCommand;
#[cfg(feature = "asset-upload-http")]
use crate::firmware::types::{WifiCredentials, WIFI_PASSWORD_MAX, WIFI_SSID_MAX};

use super::util::{find_subslice, parse_i32_ascii, parse_u64_ascii, trim_ascii_whitespace};

pub(super) fn parse_timeset_command(line: &[u8]) -> Option<TimeSyncCommand> {
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

pub(super) fn parse_repaint_command(line: &[u8]) -> bool {
    let cmd = trim_ascii_whitespace(line);
    cmd == b"REPAINT" || cmd == b"REFRESH"
}

pub(super) fn parse_repaint_marble_command(line: &[u8]) -> bool {
    let cmd = trim_ascii_whitespace(line);
    cmd == b"REPAINT_MARBLE" || cmd == b"MARBLE"
}

pub(super) fn parse_metrics_command(line: &[u8]) -> bool {
    let cmd = trim_ascii_whitespace(line);
    cmd == b"METRICS" || cmd == b"PERF"
}

pub(super) fn parse_allocator_status_command(line: &[u8]) -> bool {
    let cmd = trim_ascii_whitespace(line);
    cmd == b"PSRAM" || cmd == b"ALLOCATOR" || cmd == b"HEAP"
}

pub(super) fn parse_allocator_alloc_probe_command(line: &[u8]) -> Option<u32> {
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

pub(super) fn parse_runmode_command(line: &[u8]) -> Option<RuntimeMode> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"RUNMODE";
    if !trimmed.starts_with(cmd) {
        return None;
    }

    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i == trimmed.len() {
        return None;
    }

    let mode = &trimmed[i..];
    if mode.eq_ignore_ascii_case(b"UPLOAD") {
        return Some(RuntimeMode::Upload);
    }
    if mode.eq_ignore_ascii_case(b"NORMAL") {
        return Some(RuntimeMode::Normal);
    }
    None
}

pub(super) fn parse_mode_status_command(line: &[u8]) -> bool {
    let trimmed = trim_ascii_whitespace(line);
    if trimmed.eq_ignore_ascii_case(b"MODE") {
        return true;
    }

    let cmd = b"MODE";
    if !trimmed.starts_with(cmd) {
        return false;
    }
    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i == trimmed.len() {
        return true;
    }

    trimmed[i..].eq_ignore_ascii_case(b"STATUS")
}

pub(super) fn parse_modeset_command(
    line: &[u8],
) -> Option<super::super::commands::ModeSetOperation> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"MODE";
    if !trimmed.starts_with(cmd) {
        return None;
    }

    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i == trimmed.len() {
        return None;
    }

    let service_start = i;
    while i < trimmed.len() && !trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let service = &trimmed[service_start..i];
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i == trimmed.len() {
        return None;
    }

    let state = &trimmed[i..];
    let enabled = if state.eq_ignore_ascii_case(b"ON") {
        true
    } else if state.eq_ignore_ascii_case(b"OFF") {
        false
    } else {
        return None;
    };

    if service.eq_ignore_ascii_case(b"UPLOAD") {
        return Some(super::super::commands::ModeSetOperation::Upload(enabled));
    }

    if service.eq_ignore_ascii_case(b"ASSETS")
        || service.eq_ignore_ascii_case(b"ASSET_READ")
        || service.eq_ignore_ascii_case(b"ASSET_READS")
    {
        return Some(super::super::commands::ModeSetOperation::AssetReads(
            enabled,
        ));
    }

    None
}

#[cfg(feature = "asset-upload-http")]
pub(super) fn parse_wifiset_command(line: &[u8]) -> Option<WifiCredentials> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"WIFISET";
    if !trimmed.starts_with(cmd) {
        return None;
    }

    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i == trimmed.len() {
        return None;
    }

    let ssid_start = i;
    while i < trimmed.len() && !trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let ssid = &trimmed[ssid_start..i];
    if ssid.is_empty() || ssid.len() > WIFI_SSID_MAX {
        return None;
    }

    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let password = if i == trimmed.len() {
        &[][..]
    } else {
        let password_start = i;
        while i < trimmed.len() && !trimmed[i].is_ascii_whitespace() {
            i += 1;
        }
        let password = &trimmed[password_start..i];
        while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
            i += 1;
        }
        if i != trimmed.len() || password.len() > WIFI_PASSWORD_MAX {
            return None;
        }
        password
    };

    let mut credentials = WifiCredentials {
        ssid: [0u8; WIFI_SSID_MAX],
        ssid_len: ssid.len() as u8,
        password: [0u8; WIFI_PASSWORD_MAX],
        password_len: password.len() as u8,
    };
    credentials.ssid[..ssid.len()].copy_from_slice(ssid);
    credentials.password[..password.len()].copy_from_slice(password);
    Some(credentials)
}

pub(super) fn parse_touch_wizard_command(line: &[u8]) -> bool {
    let cmd = trim_ascii_whitespace(line);
    cmd == b"TOUCH_WIZARD" || cmd == b"TOUCH_CAL" || cmd == b"CAL_TOUCH"
}

pub(super) fn parse_touch_wizard_dump_command(line: &[u8]) -> bool {
    let cmd = trim_ascii_whitespace(line);
    cmd == b"TOUCH_WIZARD_DUMP" || cmd == b"TOUCH_DUMP" || cmd == b"WIZARD_DUMP"
}

pub(super) fn parse_sdprobe_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line) == b"SDPROBE"
}
