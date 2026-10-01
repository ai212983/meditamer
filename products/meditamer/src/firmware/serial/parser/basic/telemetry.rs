use super::super::super::commands::{TelemetryDomain, TelemetrySetOperation};
use super::super::util::trim_ascii_whitespace;

pub(in super::super) fn parse_telemetry_status_command(line: &[u8]) -> bool {
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

pub(in super::super) fn parse_telemetry_set_command(line: &[u8]) -> Option<TelemetrySetOperation> {
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
