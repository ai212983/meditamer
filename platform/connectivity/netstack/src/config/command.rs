use super::{NetConfigSet, WifiCredentials, WifiRuntimePolicy, WIFI_PASSWORD_MAX, WIFI_SSID_MAX};

pub fn parse_netcfg_set_command(line: &[u8]) -> Option<NetConfigSet> {
    let t = trim(line);
    let cmd = b"NETCFG SET";
    if !t
        .get(..cmd.len())
        .is_some_and(|p| p.eq_ignore_ascii_case(cmd))
    {
        return None;
    }
    let i = skip(t, cmd.len());
    if i == t.len() {
        return None;
    }
    let json = trim(&t[i..]);
    let mut policy = WifiRuntimePolicy::defaults();
    macro_rules! u32f {
        ($k:literal,$f:ident) => {
            if let Some(v) = json_u32(json, $k) {
                policy.$f = v;
            }
        };
    }
    u32f!(b"connect_timeout_ms", connect_timeout_ms);
    u32f!(b"dhcp_timeout_ms", dhcp_timeout_ms);
    u32f!(b"pinned_dhcp_timeout_ms", pinned_dhcp_timeout_ms);
    u32f!(b"listener_timeout_ms", listener_timeout_ms);
    u32f!(b"scan_active_min_ms", scan_active_min_ms);
    u32f!(b"scan_active_max_ms", scan_active_max_ms);
    u32f!(b"scan_passive_ms", scan_passive_ms);
    u32f!(b"cooldown_ms", cooldown_ms);
    u32f!(b"driver_restart_backoff_ms", driver_restart_backoff_ms);
    macro_rules! u8f {
        ($k:literal,$f:ident) => {
            if let Some(v) = json_u32(json, $k) {
                policy.$f = v.min(u8::MAX as u32) as u8;
            }
        };
    }
    u8f!(b"retry_same_max", retry_same_max);
    u8f!(b"rotate_candidate_max", rotate_candidate_max);
    u8f!(b"rotate_auth_max", rotate_auth_max);
    u8f!(b"full_scan_reset_max", full_scan_reset_max);
    u8f!(b"driver_restart_max", driver_restart_max);
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
        let mut c = WifiCredentials {
            ssid: [0; WIFI_SSID_MAX],
            ssid_len: ssid.len() as u8,
            password: [0; WIFI_PASSWORD_MAX],
            password_len: password.len() as u8,
        };
        c.ssid[..ssid.len()].copy_from_slice(ssid);
        c.password[..password.len()].copy_from_slice(password);
        Some(c)
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
pub fn parse_netcfg_get_command(line: &[u8]) -> bool {
    trim(line).eq_ignore_ascii_case(b"NETCFG GET")
}
pub fn parse_net_start_command(line: &[u8]) -> bool {
    trim(line).eq_ignore_ascii_case(b"NET START")
}
pub fn parse_net_stop_command(line: &[u8]) -> bool {
    trim(line).eq_ignore_ascii_case(b"NET STOP")
}
pub fn parse_net_status_command(line: &[u8]) -> bool {
    trim(line).eq_ignore_ascii_case(b"NET STATUS")
}
pub fn parse_net_recover_command(line: &[u8]) -> bool {
    trim(line).eq_ignore_ascii_case(b"NET RECOVER")
}
pub fn parse_net_listener_command(line: &[u8]) -> Option<bool> {
    let t = trim(line);
    let c = b"NET LISTENER";
    if !t.get(..c.len()).is_some_and(|p| p.eq_ignore_ascii_case(c)) {
        return None;
    }
    let v = &t[skip(t, c.len())..];
    if v.eq_ignore_ascii_case(b"ON") {
        Some(true)
    } else if v.eq_ignore_ascii_case(b"OFF") {
        Some(false)
    } else {
        None
    }
}
fn json_key<'a>(j: &'a [u8], k: &[u8]) -> Option<&'a [u8]> {
    if j.is_empty() {
        return None;
    }
    let mut p = heapless::Vec::<u8, 96>::new();
    p.push(b'"').ok()?;
    for b in k {
        p.push(*b).ok()?;
    }
    p.push(b'"').ok()?;
    let i = find(j, p.as_slice())?;
    let i = skip(j, i + p.len());
    if i >= j.len() || j[i] != b':' {
        return None;
    }
    Some(&j[skip(j, i + 1)..])
}
fn json_u32(j: &[u8], k: &[u8]) -> Option<u32> {
    let (v, n) = number(json_key(j, k)?, 0)?;
    (n != 0 && v <= u32::MAX as u64).then_some(v as u32)
}
fn json_string<'a>(j: &'a [u8], k: &[u8]) -> Option<&'a [u8]> {
    let v = json_key(j, k)?;
    if v.first().copied() != Some(b'"') {
        return None;
    }
    let r = &v[1..];
    Some(&r[..r.iter().position(|b| *b == b'"')?])
}
fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    if n.is_empty() || h.len() < n.len() {
        return None;
    }
    (0..=h.len() - n.len()).find(|&i| &h[i..i + n.len()] == n)
}
fn trim(x: &[u8]) -> &[u8] {
    let (mut a, mut b) = (0, x.len());
    while a < b && x[a].is_ascii_whitespace() {
        a += 1;
    }
    while b > a && x[b - 1].is_ascii_whitespace() {
        b -= 1;
    }
    &x[a..b]
}
fn skip(x: &[u8], mut i: usize) -> usize {
    while i < x.len() && x[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}
fn number(x: &[u8], mut i: usize) -> Option<(u64, usize)> {
    let s = i;
    let mut v = 0u64;
    while i < x.len() && x[i].is_ascii_digit() {
        v = v.checked_mul(10)?.checked_add((x[i] - b'0') as u64)?;
        i += 1;
    }
    (i != s).then_some((v, i))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_credentials_with_internal_payload_spaces() {
        let config =
            parse_netcfg_set_command(br#"NETCFG SET {"ssid":"my network","password":"a  b"}"#)
                .unwrap();
        let credentials = config.credentials.unwrap();
        assert_eq!(
            &credentials.ssid[..credentials.ssid_len as usize],
            b"my network"
        );
        assert_eq!(
            &credentials.password[..credentials.password_len as usize],
            b"a  b"
        );
    }

    #[test]
    fn rejects_credential_overflow_and_numeric_overflow() {
        let mut line = heapless::String::<128>::new();
        line.push_str(r#"NETCFG SET {"ssid":""#).unwrap();
        for _ in 0..=WIFI_SSID_MAX {
            line.push('s').unwrap();
        }
        line.push_str(r#""}"#).unwrap();
        assert!(parse_netcfg_set_command(line.as_bytes()).is_none());
        assert!(
            parse_netcfg_set_command(br#"NETCFG SET {"connect_timeout_ms":4294967296}"#).is_some()
        );
        assert!(parse_netcfg_set_command(br#"NETCFG SET {"ssid":"x","password":"p"}"#).is_some());
    }

    #[test]
    fn preserves_existing_command_forms() {
        assert!(parse_netcfg_get_command(b" netcfg get\r\n"));
        assert_eq!(parse_net_listener_command(b"NET LISTENER ON"), Some(true));
        assert_eq!(parse_net_listener_command(b"NET LISTENER OFF"), Some(false));
        assert!(parse_net_start_command(b"NET START"));
        assert!(parse_net_stop_command(b"NET STOP"));
        assert!(parse_net_status_command(b"NET STATUS"));
        assert!(parse_net_recover_command(b"NET RECOVER"));
    }
}
