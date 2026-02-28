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

pub fn detect_port() -> Option<String> {
    let mut candidates = collect_candidates();

    let hint = std::env::var("HOSTCTL_PORT_HINT")
        .ok()
        .filter(|s| !s.trim().is_empty());

    if let Some(hint) = hint {
        candidates.retain(|c| c.contains(&hint));
    }

    let cu_ports: Vec<_> = candidates
        .iter()
        .filter(|c| c.starts_with("/dev/cu."))
        .cloned()
        .collect();
    if cu_ports.len() == 1 {
        return cu_ports.into_iter().next();
    }

    let linux_ports: Vec<_> = candidates
        .iter()
        .filter(|c| c.starts_with("/dev/ttyUSB") || c.starts_with("/dev/ttyACM"))
        .cloned()
        .collect();
    if linux_ports.len() == 1 {
        return linux_ports.into_iter().next();
    }

    let tty_ports: Vec<_> = candidates
        .iter()
        .filter(|c| c.starts_with("/dev/tty."))
        .cloned()
        .collect();
    if tty_ports.len() == 1 {
        return tty_ports.into_iter().next();
    }

    if candidates.len() == 1 {
        return candidates.into_iter().next();
    }

    None
}

pub fn list_candidates() -> Vec<String> {
    collect_candidates()
}
