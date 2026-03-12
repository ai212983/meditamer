use std::path::Path;

use glob::glob;

const PATTERNS: &[&str] = &[
    "/dev/cu.usbserial*",
    "/dev/cu.usbmodem*",
    "/dev/cu.SLAB_USBtoUART*",
    "/dev/cu.wchusbserial*",
    "/dev/tty.usbserial*",
    "/dev/tty.usbmodem*",
    "/dev/tty.SLAB_USBtoUART*",
    "/dev/tty.wchusbserial*",
    "/dev/ttyUSB*",
    "/dev/ttyACM*",
];

fn collect_candidates() -> Vec<String> {
    let mut out = Vec::new();
    for pattern in PATTERNS {
        let Ok(entries) = glob(pattern) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.display().to_string();
            if !out.iter().any(|existing| existing == &path) {
                out.push(path);
            }
        }
    }
    out
}

fn select_candidate(candidates: &[String], hint: Option<&str>) -> Option<String> {
    let mut filtered = candidates.to_vec();
    if let Some(hint) = hint.filter(|s| !s.trim().is_empty()) {
        filtered.retain(|c| c.contains(hint));
    }

    let cu_ports: Vec<_> = filtered
        .iter()
        .filter(|c| c.starts_with("/dev/cu."))
        .cloned()
        .collect();
    if cu_ports.len() == 1 {
        return cu_ports.into_iter().next();
    }

    let linux_ports: Vec<_> = filtered
        .iter()
        .filter(|c| c.starts_with("/dev/ttyUSB") || c.starts_with("/dev/ttyACM"))
        .cloned()
        .collect();
    if linux_ports.len() == 1 {
        return linux_ports.into_iter().next();
    }

    let tty_ports: Vec<_> = filtered
        .iter()
        .filter(|c| c.starts_with("/dev/tty."))
        .cloned()
        .collect();
    if tty_ports.len() == 1 {
        return tty_ports.into_iter().next();
    }

    if filtered.len() == 1 {
        return filtered.into_iter().next();
    }

    None
}

fn paired_cu_port(port: &str) -> Option<String> {
    port.strip_prefix("/dev/tty.")
        .map(|suffix| format!("/dev/cu.{suffix}"))
}

fn canonicalize_with_candidates(port: &str, candidates: &[String]) -> String {
    if let Some(cu_port) = paired_cu_port(port) {
        if candidates.iter().any(|candidate| candidate == &cu_port) {
            return cu_port;
        }
    }
    port.to_string()
}

pub fn canonicalize_port(port: &str) -> String {
    let candidates = collect_candidates();
    let rewritten = canonicalize_with_candidates(port, &candidates);
    if rewritten != port {
        return rewritten;
    }
    if let Some(cu_port) = paired_cu_port(port) {
        if Path::new(&cu_port).exists() {
            return cu_port;
        }
    }
    port.to_string()
}

pub fn detect_port() -> Option<String> {
    let candidates = collect_candidates();
    let hint = std::env::var("HOSTCTL_PORT_HINT").ok();
    select_candidate(&candidates, hint.as_deref())
}

pub fn list_candidates() -> Vec<String> {
    collect_candidates()
}

#[cfg(test)]
mod tests {
    use super::{canonicalize_with_candidates, select_candidate};

    #[test]
    fn select_candidate_prefers_single_cu_port_over_matching_tty() {
        let candidates = vec![
            "/dev/cu.usbserial-510".to_string(),
            "/dev/tty.usbserial-510".to_string(),
        ];
        let selected = select_candidate(&candidates, None).expect("selects cu");
        assert_eq!(selected, "/dev/cu.usbserial-510");
    }

    #[test]
    fn select_candidate_rejects_ambiguous_multi_port_sets() {
        let candidates = vec![
            "/dev/cu.usbserial-510".to_string(),
            "/dev/cu.usbserial-540".to_string(),
        ];
        assert!(select_candidate(&candidates, None).is_none());
    }

    #[test]
    fn explicit_tty_port_is_rewritten_to_matching_cu_port() {
        let candidates = vec![
            "/dev/cu.usbserial-510".to_string(),
            "/dev/tty.usbserial-510".to_string(),
        ];
        let canonical = canonicalize_with_candidates("/dev/tty.usbserial-510", &candidates);
        assert_eq!(canonical, "/dev/cu.usbserial-510");
    }
}
