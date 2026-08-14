use core::sync::atomic::Ordering;

use super::super::counters::{LOG_FILTER_MASK, LOG_FILTER_MASK_ALL};

pub(crate) fn log_filter_mask() -> u32 {
    LOG_FILTER_MASK.load(Ordering::Relaxed)
}

pub(crate) fn log_filter_enabled(domain: u32) -> bool {
    (log_filter_mask() & domain) != 0
}

pub(crate) fn set_log_filter_mask(mask: u32) -> u32 {
    let normalized = mask & LOG_FILTER_MASK_ALL;
    LOG_FILTER_MASK.store(normalized, Ordering::Relaxed);
    normalized
}

pub(crate) fn set_log_filter_domain(domain: u32, enabled: bool) -> u32 {
    let domain = domain & LOG_FILTER_MASK_ALL;
    let _ = LOG_FILTER_MASK.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
        let next = if enabled {
            current | domain
        } else {
            current & !domain
        };
        Some(next)
    });
    log_filter_mask()
}
