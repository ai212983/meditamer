use super::super::commands::SdWaitTarget;
use super::{
    util::{parse_u64_ascii, trim_ascii_whitespace},
    SDWAIT_DEFAULT_TIMEOUT_MS,
};

pub(super) fn parse_sdwait_command(line: &[u8]) -> Option<(SdWaitTarget, u32)> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"SDWAIT";
    if !trimmed.starts_with(cmd) {
        return None;
    }

    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i == trimmed.len() {
        return Some((SdWaitTarget::Next, SDWAIT_DEFAULT_TIMEOUT_MS));
    }

    let token_start = i;
    while i < trimmed.len() && !trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let token = &trimmed[token_start..i];
    let target = if token == b"LAST" {
        SdWaitTarget::Last
    } else if token == b"NEXT" {
        SdWaitTarget::Next
    } else {
        let (id, next_i) = parse_u64_ascii(trimmed, token_start)?;
        if next_i != i || id > u32::MAX as u64 {
            return None;
        }
        SdWaitTarget::Id(id as u32)
    };

    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i == trimmed.len() {
        return Some((target, SDWAIT_DEFAULT_TIMEOUT_MS));
    }

    let (timeout_ms, next_i) = parse_u64_ascii(trimmed, i)?;
    if timeout_ms > u32::MAX as u64 {
        return None;
    }
    i = next_i;
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i != trimmed.len() {
        return None;
    }
    Some((target, timeout_ms as u32))
}

pub(super) fn parse_sdrwverify_command(line: &[u8]) -> Option<u32> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"SDRWVERIFY";
    if !trimmed.starts_with(cmd) {
        return None;
    }

    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let (lba, next_i) = parse_u64_ascii(trimmed, i)?;
    if lba > u32::MAX as u64 {
        return None;
    }
    i = next_i;
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i != trimmed.len() {
        return None;
    }

    Some(lba as u32)
}
