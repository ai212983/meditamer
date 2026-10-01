//! Shared log-domain filtering.
//!
//! One mask, five domain bits, and the read/write operations that gate a
//! `println!` call by domain. `platform/connectivity/netstack` and product diagnostics
//! (SD, HTTP body handling) both filter through this registry, so one serial
//! `TELEM`/`TELEMSET` command surface controls both rather than each crate
//! keeping a private copy in sync by hand.

#![no_std]

use core::sync::atomic::{AtomicU32, Ordering};

pub const LOG_DOMAIN_WIFI: u32 = 1 << 0;
pub const LOG_DOMAIN_REASSOC: u32 = 1 << 1;
pub const LOG_DOMAIN_NET: u32 = 1 << 2;
pub const LOG_DOMAIN_HTTP: u32 = 1 << 3;
pub const LOG_DOMAIN_SD: u32 = 1 << 4;
pub const LOG_FILTER_MASK_ALL: u32 =
    LOG_DOMAIN_WIFI | LOG_DOMAIN_REASSOC | LOG_DOMAIN_NET | LOG_DOMAIN_HTTP | LOG_DOMAIN_SD;
pub const LOG_FILTER_MASK_DEFAULT: u32 = LOG_FILTER_MASK_ALL;

static LOG_FILTER_MASK: AtomicU32 = AtomicU32::new(LOG_FILTER_MASK_DEFAULT);

pub fn log_filter_mask() -> u32 {
    LOG_FILTER_MASK.load(Ordering::Relaxed)
}

pub fn log_filter_enabled(domain: u32) -> bool {
    (log_filter_mask() & domain) != 0
}

pub fn set_log_filter_mask(mask: u32) -> u32 {
    let normalized = mask & LOG_FILTER_MASK_ALL;
    LOG_FILTER_MASK.store(normalized, Ordering::Relaxed);
    normalized
}

pub fn set_log_filter_domain(domain: u32, enabled: bool) -> u32 {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_toggle_is_masked_to_known_bits() {
        set_log_filter_mask(0);
        assert_eq!(
            set_log_filter_domain(LOG_DOMAIN_WIFI, true),
            LOG_DOMAIN_WIFI
        );
        assert_eq!(
            set_log_filter_domain(0xFFFF_FFFF, true),
            LOG_FILTER_MASK_ALL
        );
        assert!(log_filter_enabled(LOG_DOMAIN_SD));
        assert_eq!(
            set_log_filter_domain(LOG_DOMAIN_SD, false),
            LOG_FILTER_MASK_ALL & !LOG_DOMAIN_SD
        );
        assert!(!log_filter_enabled(LOG_DOMAIN_SD));
    }

    #[test]
    fn mask_write_is_normalized_to_known_bits() {
        assert_eq!(set_log_filter_mask(0xFFFF_FFFF), LOG_FILTER_MASK_ALL);
        assert_eq!(set_log_filter_mask(0), 0);
        assert_eq!(log_filter_mask(), 0);
    }
}
