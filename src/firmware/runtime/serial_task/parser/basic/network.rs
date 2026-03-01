use crate::firmware::types::{
    NetConfigSet, WifiCredentials, WifiRuntimePolicy, WIFI_PASSWORD_MAX, WIFI_SSID_MAX,
};

use super::super::util::{find_subslice, parse_u64_ascii, trim_ascii_whitespace};

pub(in super::super) fn parse_netcfg_set_command(line: &[u8]) -> Option<NetConfigSet> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"NETCFG SET";
    if !trimmed
        .get(..cmd.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(cmd))
    {
        return None;
    }

    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i == trimmed.len() {
        return None;
    }
    let json = trim_ascii_whitespace(&trimmed[i..]);

    let mut policy = WifiRuntimePolicy::defaults();
    if let Some(value) = json_u32(json, b"connect_timeout_ms") {
        policy.connect_timeout_ms = value;
    }
    if let Some(value) = json_u32(json, b"dhcp_timeout_ms") {
        policy.dhcp_timeout_ms = value;
    }
    if let Some(value) = json_u32(json, b"pinned_dhcp_timeout_ms") {
        policy.pinned_dhcp_timeout_ms = value;
    }
    if let Some(value) = json_u32(json, b"listener_timeout_ms") {
        policy.listener_timeout_ms = value;
    }
    if let Some(value) = json_u32(json, b"scan_active_min_ms") {
        policy.scan_active_min_ms = value;
    }
    if let Some(value) = json_u32(json, b"scan_active_max_ms") {
        policy.scan_active_max_ms = value;
    }
    if let Some(value) = json_u32(json, b"scan_passive_ms") {
        policy.scan_passive_ms = value;
    }
    if let Some(value) = json_u32(json, b"retry_same_max") {
        policy.retry_same_max = value.min(u8::MAX as u32) as u8;
    }
    if let Some(value) = json_u32(json, b"rotate_candidate_max") {
        policy.rotate_candidate_max = value.min(u8::MAX as u32) as u8;
    }
    if let Some(value) = json_u32(json, b"rotate_auth_max") {
        policy.rotate_auth_max = value.min(u8::MAX as u32) as u8;
    }
    if let Some(value) = json_u32(json, b"full_scan_reset_max") {
        policy.full_scan_reset_max = value.min(u8::MAX as u32) as u8;
    }
    if let Some(value) = json_u32(json, b"driver_restart_max") {
        policy.driver_restart_max = value.min(u8::MAX as u32) as u8;
    }
    if let Some(value) = json_u32(json, b"cooldown_ms") {
        policy.cooldown_ms = value;
    }
    if let Some(value) = json_u32(json, b"driver_restart_backoff_ms") {
        policy.driver_restart_backoff_ms = value;
    }

    let ssid = json_string(json, b"ssid");
    let password = json_string(json, b"password");
    let credentials = if let Some(ssid) = ssid {
        if ssid.is_empty() || ssid.len() > WIFI_SSID_MAX {
            return None;
        }
        let password = password.unwrap_or(&[]);
        if password.len() > WIFI_PASSWORD_MAX {
            return None;
        }
        let mut credentials = WifiCredentials {
            ssid: [0u8; WIFI_SSID_MAX],
            ssid_len: ssid.len() as u8,
            password: [0u8; WIFI_PASSWORD_MAX],
            password_len: password.len() as u8,
        };
        credentials.ssid[..ssid.len()].copy_from_slice(ssid);
        credentials.password[..password.len()].copy_from_slice(password);
        Some(credentials)
    } else {
        if password.is_some() {
            return None;
        }
        None
    };

    Some(NetConfigSet {
        credentials,
        policy: policy.sanitized(),
    })
}

pub(in super::super) fn parse_netcfg_get_command(line: &[u8]) -> bool {
    let trimmed = trim_ascii_whitespace(line);
    trimmed.eq_ignore_ascii_case(b"NETCFG GET")
}

pub(in super::super) fn parse_net_start_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line).eq_ignore_ascii_case(b"NET START")
}

pub(in super::super) fn parse_net_stop_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line).eq_ignore_ascii_case(b"NET STOP")
}

pub(in super::super) fn parse_net_status_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line).eq_ignore_ascii_case(b"NET STATUS")
}

pub(in super::super) fn parse_net_recover_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line).eq_ignore_ascii_case(b"NET RECOVER")
}

pub(in super::super) fn parse_net_listener_command(line: &[u8]) -> Option<bool> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"NET LISTENER";
    if !trimmed
        .get(..cmd.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(cmd))
    {
        return None;
    }
    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i == trimmed.len() {
        return None;
    }
    let value = &trimmed[i..];
    if value.eq_ignore_ascii_case(b"ON") {
        return Some(true);
    }
    if value.eq_ignore_ascii_case(b"OFF") {
        return Some(false);
    }
    None
}

fn json_key_start<'a>(json: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    if json.is_empty() {
        return None;
    }
    let mut pattern = heapless::Vec::<u8, 96>::new();
    pattern.push(b'"').ok()?;
    for byte in key {
        pattern.push(*byte).ok()?;
    }
    pattern.push(b'"').ok()?;
    let idx = find_subslice(json, pattern.as_slice())?;
    let mut i = idx + pattern.len();
    while i < json.len() && json[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= json.len() || json[i] != b':' {
        return None;
    }
    i += 1;
    while i < json.len() && json[i].is_ascii_whitespace() {
        i += 1;
    }
    Some(&json[i..])
}

fn json_u32(json: &[u8], key: &[u8]) -> Option<u32> {
    let value = json_key_start(json, key)?;
    let (parsed, next_i) = parse_u64_ascii(value, 0)?;
    if next_i == 0 || parsed > u32::MAX as u64 {
        return None;
    }
    Some(parsed as u32)
}

fn json_string<'a>(json: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let value = json_key_start(json, key)?;
    if value.first().copied() != Some(b'"') {
        return None;
    }
    let rest = &value[1..];
    let end = rest.iter().position(|byte| *byte == b'"')?;
    Some(&rest[..end])
}
