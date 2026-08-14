use super::*;

pub(super) const WIFI_SCAN_ENTRY_PROMISC_DIAG: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_SCAN_ENTRY_PROMISC_DIAG") {
        Some(value) => Some(value),
        None => option_env!("WIFI_SCAN_ENTRY_PROMISC_DIAG"),
    },
);
pub(super) const WIFI_POST_START_PROMISC_DIAG: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_POST_START_PROMISC_DIAG") {
        Some(value) => Some(value),
        None => option_env!("WIFI_POST_START_PROMISC_DIAG"),
    },
);
pub(super) const WIFI_SOFTWARE_RESET_ON_POST_START_PROMISC_ZERO: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_SOFTWARE_RESET_ON_POST_START_PROMISC_ZERO") {
        Some(value) => Some(value),
        None => option_env!("WIFI_SOFTWARE_RESET_ON_POST_START_PROMISC_ZERO"),
    },
);
pub(super) const WIFI_POST_START_PROMISC_ZERO_HARD_REINIT: bool = parse_nonzero_flag(
    match option_env!("MEDITAMER_WIFI_POST_START_PROMISC_ZERO_HARD_REINIT") {
        Some(value) => Some(value),
        None => option_env!("WIFI_POST_START_PROMISC_ZERO_HARD_REINIT"),
    },
);
const WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS_DEFAULT: u64 = 120;
const WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS_MIN: u64 = 50;
const WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS_MAX: u64 = 3_000;
pub(super) const WIFI_SCAN_ENTRY_PROMISC_DIAG_CHANNELS: [u8; 4] = [8, 1, 6, 11];
pub(super) const WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS: u64 = {
    let configured = match option_env!("MEDITAMER_WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS") {
        Some(value) => Some(value),
        None => option_env!("WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS"),
    };
    match configured {
        Some(raw) => match parse_ascii_u64(raw) {
            Some(value)
                if value >= WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS_MIN
                    && value <= WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS_MAX =>
            {
                value
            }
            _ => WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS_DEFAULT,
        },
        None => WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS_DEFAULT,
    }
};
