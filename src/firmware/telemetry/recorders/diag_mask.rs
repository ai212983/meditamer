pub(crate) fn diag_mask() -> u32 {
    DIAG_MASK.load(Ordering::Relaxed)
}

pub(crate) fn diag_enabled(domain: u32) -> bool {
    (diag_mask() & domain) != 0
}

pub(crate) fn diag_set_mask(mask: u32) -> u32 {
    let normalized = mask & DIAG_MASK_ALL;
    DIAG_MASK.store(normalized, Ordering::Relaxed);
    normalized
}

pub(crate) fn diag_set_domain(domain: u32, enabled: bool) -> u32 {
    let domain = domain & DIAG_MASK_ALL;
    let _ = DIAG_MASK.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
        let next = if enabled {
            current | domain
        } else {
            current & !domain
        };
        Some(next)
    });
    diag_mask()
}
