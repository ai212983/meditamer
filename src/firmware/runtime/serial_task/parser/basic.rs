use crate::firmware::app_state::{BaseMode, DayBackground, DiagKind, DiagTargets, OverlayMode};
use crate::firmware::types::TimeSyncCommand;
#[cfg(feature = "asset-upload-http")]
use crate::firmware::types::{
    NetConfigSet, WifiCredentials, WifiRuntimePolicy, WIFI_PASSWORD_MAX, WIFI_SSID_MAX,
};

use super::super::commands::{StateSetOperation, TelemetryDomain, TelemetrySetOperation};
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

pub(super) fn parse_metrics_net_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line) == b"METRICSNET"
}

pub(super) fn parse_telemetry_status_command(line: &[u8]) -> bool {
    let trimmed = trim_ascii_whitespace(line);
    if trimmed.eq_ignore_ascii_case(b"TELEM") || trimmed.eq_ignore_ascii_case(b"TELEMETRY") {
        return true;
    }

    let cmd = if trimmed.len() >= b"TELEM".len() && trimmed[..5].eq_ignore_ascii_case(b"TELEM") {
        b"TELEM".as_slice()
    } else if trimmed.len() >= b"TELEMETRY".len() && trimmed[..9].eq_ignore_ascii_case(b"TELEMETRY")
    {
        b"TELEMETRY".as_slice()
    } else {
        return false;
    };

    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    i < trimmed.len() && trimmed[i..].eq_ignore_ascii_case(b"STATUS")
}

pub(super) fn parse_telemetry_set_command(line: &[u8]) -> Option<TelemetrySetOperation> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd =
        if trimmed.len() >= b"TELEMSET".len() && trimmed[..8].eq_ignore_ascii_case(b"TELEMSET") {
            b"TELEMSET".as_slice()
        } else if trimmed.len() >= b"TELEMETRYSET".len()
            && trimmed[..12].eq_ignore_ascii_case(b"TELEMETRYSET")
        {
            b"TELEMETRYSET".as_slice()
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

    let token_start = i;
    while i < trimmed.len() && !trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let token = &trimmed[token_start..i];

    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }

    if token.eq_ignore_ascii_case(b"DEFAULT") {
        if i != trimmed.len() {
            return None;
        }
        return Some(TelemetrySetOperation::Default);
    }
    if token.eq_ignore_ascii_case(b"NONE") {
        if i != trimmed.len() {
            return None;
        }
        return Some(TelemetrySetOperation::All { enabled: false });
    }
    if token.eq_ignore_ascii_case(b"ALL") {
        let enabled = if i == trimmed.len() {
            true
        } else {
            parse_on_off(&trimmed[i..])?
        };
        return Some(TelemetrySetOperation::All { enabled });
    }

    let domain = parse_telemetry_domain(token)?;
    if i == trimmed.len() {
        return None;
    }
    let enabled = parse_on_off(&trimmed[i..])?;
    Some(TelemetrySetOperation::Domain { domain, enabled })
}

fn parse_telemetry_domain(token: &[u8]) -> Option<TelemetryDomain> {
    if token.eq_ignore_ascii_case(b"WIFI") {
        return Some(TelemetryDomain::Wifi);
    }
    if token.eq_ignore_ascii_case(b"REASSOC")
        || token.eq_ignore_ascii_case(b"SCAN")
        || token.eq_ignore_ascii_case(b"WIFI_SCAN")
    {
        return Some(TelemetryDomain::Reassoc);
    }
    if token.eq_ignore_ascii_case(b"NET") || token.eq_ignore_ascii_case(b"NETWORK") {
        return Some(TelemetryDomain::Net);
    }
    if token.eq_ignore_ascii_case(b"HTTP") {
        return Some(TelemetryDomain::Http);
    }
    if token.eq_ignore_ascii_case(b"SD") || token.eq_ignore_ascii_case(b"STORAGE") {
        return Some(TelemetryDomain::Sd);
    }
    None
}

fn parse_on_off(token: &[u8]) -> Option<bool> {
    if token.eq_ignore_ascii_case(b"ON")
        || token.eq_ignore_ascii_case(b"ENABLE")
        || token.eq_ignore_ascii_case(b"ENABLED")
    {
        return Some(true);
    }
    if token.eq_ignore_ascii_case(b"OFF")
        || token.eq_ignore_ascii_case(b"DISABLE")
        || token.eq_ignore_ascii_case(b"DISABLED")
    {
        return Some(false);
    }
    None
}

pub(super) fn parse_ping_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line) == b"PING"
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

pub(super) fn parse_state_get_command(line: &[u8]) -> bool {
    let trimmed = trim_ascii_whitespace(line);
    trimmed.eq_ignore_ascii_case(b"STATE")
        || trimmed.eq_ignore_ascii_case(b"STATE GET")
        || trimmed.eq_ignore_ascii_case(b"STATE STATUS")
}

pub(super) fn parse_diag_get_command(line: &[u8]) -> bool {
    let trimmed = trim_ascii_whitespace(line);
    trimmed.eq_ignore_ascii_case(b"DIAG")
        || trimmed.eq_ignore_ascii_case(b"DIAG GET")
        || trimmed.eq_ignore_ascii_case(b"DIAG STATUS")
}

pub(super) fn parse_state_set_command(line: &[u8]) -> Option<StateSetOperation> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"STATE SET";
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
    let kv = &trimmed[i..];
    let mut split = kv.splitn(2, |byte| *byte == b'=');
    let key = split.next()?;
    let value = split.next()?;
    if key.eq_ignore_ascii_case(b"base") {
        if value.eq_ignore_ascii_case(b"DAY") {
            return Some(StateSetOperation::Base(BaseMode::Day));
        }
        if value.eq_ignore_ascii_case(b"TOUCH_WIZARD") {
            return Some(StateSetOperation::Base(BaseMode::TouchWizard));
        }
        return None;
    }
    if key.eq_ignore_ascii_case(b"day_bg") {
        if value.eq_ignore_ascii_case(b"SUMINAGASHI") {
            return Some(StateSetOperation::DayBackground(DayBackground::Suminagashi));
        }
        if value.eq_ignore_ascii_case(b"SHANSHUI") {
            return Some(StateSetOperation::DayBackground(DayBackground::Shanshui));
        }
        return None;
    }
    if key.eq_ignore_ascii_case(b"overlay") {
        if value.eq_ignore_ascii_case(b"NONE") {
            return Some(StateSetOperation::Overlay(OverlayMode::None));
        }
        if value.eq_ignore_ascii_case(b"CLOCK") {
            return Some(StateSetOperation::Overlay(OverlayMode::Clock));
        }
        return None;
    }
    if key.eq_ignore_ascii_case(b"upload") {
        if value.eq_ignore_ascii_case(b"ON") {
            return Some(StateSetOperation::Upload(true));
        }
        if value.eq_ignore_ascii_case(b"OFF") {
            return Some(StateSetOperation::Upload(false));
        }
        return None;
    }
    if key.eq_ignore_ascii_case(b"assets") {
        if value.eq_ignore_ascii_case(b"ON") {
            return Some(StateSetOperation::AssetReads(true));
        }
        if value.eq_ignore_ascii_case(b"OFF") {
            return Some(StateSetOperation::AssetReads(false));
        }
        return None;
    }
    None
}

pub(super) fn parse_state_diag_command(line: &[u8]) -> Option<(DiagKind, DiagTargets)> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"STATE DIAG";
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
    let args = &trimmed[i..];
    let mut kind = None;
    let mut targets = DiagTargets::none();
    for token in args.split(|byte| *byte == b' ') {
        if token.is_empty() {
            continue;
        }
        let mut split = token.splitn(2, |byte| *byte == b'=');
        let key = split.next()?;
        let value = split.next()?;
        if key.eq_ignore_ascii_case(b"kind") {
            if value.eq_ignore_ascii_case(b"NONE") {
                kind = Some(DiagKind::None);
            } else if value.eq_ignore_ascii_case(b"DEBUG") {
                kind = Some(DiagKind::Debug);
            } else if value.eq_ignore_ascii_case(b"TEST") {
                kind = Some(DiagKind::Test);
            } else {
                return None;
            }
        } else if key.eq_ignore_ascii_case(b"targets") {
            let mut bits = 0u8;
            for target in value.split(|byte| *byte == b'|') {
                if target.eq_ignore_ascii_case(b"NONE") {
                    bits = 0;
                    continue;
                }
                if target.eq_ignore_ascii_case(b"SD") {
                    bits |= 1 << 0;
                } else if target.eq_ignore_ascii_case(b"WIFI") {
                    bits |= 1 << 1;
                } else if target.eq_ignore_ascii_case(b"DISPLAY") {
                    bits |= 1 << 2;
                } else if target.eq_ignore_ascii_case(b"TOUCH") {
                    bits |= 1 << 3;
                } else if target.eq_ignore_ascii_case(b"IMU") {
                    bits |= 1 << 4;
                } else if target.is_empty() {
                    continue;
                } else {
                    return None;
                }
            }
            targets = DiagTargets::from_persisted(bits);
        }
    }
    Some((kind?, targets))
}

#[cfg(feature = "asset-upload-http")]
pub(super) fn parse_netcfg_set_command(line: &[u8]) -> Option<NetConfigSet> {
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

#[cfg(feature = "asset-upload-http")]
pub(super) fn parse_netcfg_get_command(line: &[u8]) -> bool {
    let trimmed = trim_ascii_whitespace(line);
    trimmed.eq_ignore_ascii_case(b"NETCFG GET")
}

#[cfg(feature = "asset-upload-http")]
pub(super) fn parse_net_start_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line).eq_ignore_ascii_case(b"NET START")
}

#[cfg(feature = "asset-upload-http")]
pub(super) fn parse_net_stop_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line).eq_ignore_ascii_case(b"NET STOP")
}

#[cfg(feature = "asset-upload-http")]
pub(super) fn parse_net_status_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line).eq_ignore_ascii_case(b"NET STATUS")
}

#[cfg(feature = "asset-upload-http")]
pub(super) fn parse_net_recover_command(line: &[u8]) -> bool {
    trim_ascii_whitespace(line).eq_ignore_ascii_case(b"NET RECOVER")
}

#[cfg(feature = "asset-upload-http")]
pub(super) fn parse_net_listener_command(line: &[u8]) -> Option<bool> {
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

#[cfg(feature = "asset-upload-http")]
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

#[cfg(feature = "asset-upload-http")]
fn json_u32(json: &[u8], key: &[u8]) -> Option<u32> {
    let value = json_key_start(json, key)?;
    let (parsed, next_i) = parse_u64_ascii(value, 0)?;
    if next_i == 0 || parsed > u32::MAX as u64 {
        return None;
    }
    Some(parsed as u32)
}

#[cfg(feature = "asset-upload-http")]
fn json_string<'a>(json: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let value = json_key_start(json, key)?;
    if value.first().copied() != Some(b'"') {
        return None;
    }
    let rest = &value[1..];
    let end = rest.iter().position(|byte| *byte == b'"')?;
    Some(&rest[..end])
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_state_set_forms() {
        assert!(matches!(
            parse_state_set_command(b"STATE SET base=DAY"),
            Some(StateSetOperation::Base(BaseMode::Day))
        ));
        assert!(matches!(
            parse_state_set_command(b"STATE SET base=TOUCH_WIZARD"),
            Some(StateSetOperation::Base(BaseMode::TouchWizard))
        ));
        assert!(matches!(
            parse_state_set_command(b"STATE SET day_bg=SUMINAGASHI"),
            Some(StateSetOperation::DayBackground(DayBackground::Suminagashi))
        ));
        assert!(matches!(
            parse_state_set_command(b"STATE SET day_bg=SHANSHUI"),
            Some(StateSetOperation::DayBackground(DayBackground::Shanshui))
        ));
        assert!(matches!(
            parse_state_set_command(b"STATE SET overlay=NONE"),
            Some(StateSetOperation::Overlay(OverlayMode::None))
        ));
        assert!(matches!(
            parse_state_set_command(b"STATE SET overlay=CLOCK"),
            Some(StateSetOperation::Overlay(OverlayMode::Clock))
        ));
        assert!(matches!(
            parse_state_set_command(b"STATE SET upload=ON"),
            Some(StateSetOperation::Upload(true))
        ));
        assert!(matches!(
            parse_state_set_command(b"STATE SET assets=OFF"),
            Some(StateSetOperation::AssetReads(false))
        ));
    }

    #[test]
    fn parses_diag_get_forms() {
        assert!(parse_diag_get_command(b"DIAG"));
        assert!(parse_diag_get_command(b"DIAG GET"));
        assert!(parse_diag_get_command(b"DIAG STATUS"));
        assert!(!parse_diag_get_command(b"DIAG START"));
    }

    #[test]
    fn rejects_invalid_state_set_pairs() {
        assert!(parse_state_set_command(b"STATE SET foo=bar").is_none());
        assert!(parse_state_set_command(b"STATE SET overlay=bad").is_none());
        assert!(parse_state_set_command(b"STATE SET upload=maybe").is_none());
        assert!(parse_state_set_command(b"STATE SET day_bg=night").is_none());
    }

    #[test]
    fn parses_state_diag_and_targets() {
        let (kind, targets) =
            parse_state_diag_command(b"STATE DIAG kind=DEBUG targets=SD|WIFI").expect("diag parse");
        assert!(matches!(kind, DiagKind::Debug));
        assert_eq!(targets.as_persisted(), (1 << 0) | (1 << 1));

        let (none_kind, none_targets) =
            parse_state_diag_command(b"STATE DIAG kind=NONE targets=NONE").expect("diag none");
        assert!(matches!(none_kind, DiagKind::None));
        assert_eq!(none_targets.as_persisted(), 0);
    }

    #[test]
    fn rejects_invalid_state_diag_values() {
        assert!(parse_state_diag_command(b"STATE DIAG kind=BAD targets=SD").is_none());
        assert!(parse_state_diag_command(b"STATE DIAG kind=TEST targets=SD|GPS").is_none());
        assert!(parse_state_diag_command(b"STATE DIAG targets=SD").is_none());
    }

    #[cfg(feature = "asset-upload-http")]
    #[test]
    fn parses_netcfg_set_json() {
        let parsed = parse_netcfg_set_command(
            br#"NETCFG SET {"ssid":"Suprematic","password":"abc12345","connect_timeout_ms":28000,"dhcp_timeout_ms":22000,"pinned_dhcp_timeout_ms":48000}"#,
        )
        .expect("netcfg parse");
        let creds = parsed.credentials.expect("credentials");
        assert_eq!(&creds.ssid[..creds.ssid_len as usize], b"Suprematic");
        assert_eq!(&creds.password[..creds.password_len as usize], b"abc12345");
        assert_eq!(parsed.policy.connect_timeout_ms, 28_000);
        assert_eq!(parsed.policy.dhcp_timeout_ms, 22_000);
        assert_eq!(parsed.policy.pinned_dhcp_timeout_ms, 48_000);
    }

    #[cfg(feature = "asset-upload-http")]
    #[test]
    fn parses_netcfg_set_policy_only() {
        let parsed = parse_netcfg_set_command(
            br#"NETCFG SET {"connect_timeout_ms":31000,"rotate_auth_max":7}"#,
        )
        .expect("policy parse");
        assert!(parsed.credentials.is_none());
        assert_eq!(parsed.policy.connect_timeout_ms, 31_000);
        assert_eq!(parsed.policy.rotate_auth_max, 7);
    }

    #[cfg(feature = "asset-upload-http")]
    #[test]
    fn parses_net_listener_command() {
        assert_eq!(parse_net_listener_command(b"NET LISTENER ON"), Some(true));
        assert_eq!(parse_net_listener_command(b"NET LISTENER OFF"), Some(false));
        assert_eq!(parse_net_listener_command(b"NET LISTENER maybe"), None);
    }
}
