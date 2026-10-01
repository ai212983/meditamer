//! Read-token polling is cooperative: time spent yielded to other tasks is not
//! time spent clocking the card. Keep a separate gross bound for stalled work.

#[cfg(target_os = "none")]
pub(crate) const SD_PROTOCOL_WAIT_DEADLINE_MS: u32 = 250;
#[cfg(target_os = "none")]
pub(crate) const SD_DATA_TOKEN_POLL_BURST_BYTES: usize = 16;
pub(crate) const SD_DATA_TOKEN_ACTIVE_TIMEOUT_US: u64 = 300_000;
pub(crate) const SD_DATA_TOKEN_WALL_CEILING_MS: u32 = 2_000;

pub(crate) const fn data_token_wait_expired(active_us: u64, elapsed_ms: u32) -> bool {
    active_us >= SD_DATA_TOKEN_ACTIVE_TIMEOUT_US || elapsed_ms >= SD_DATA_TOKEN_WALL_CEILING_MS
}

#[cfg(test)]
mod tests {
    use super::data_token_wait_expired;

    #[test]
    fn yielded_time_does_not_consume_the_read_budget() {
        assert!(!data_token_wait_expired(1_000, 737));
        assert!(!data_token_wait_expired(299_999, 1_999));
        assert!(data_token_wait_expired(300_000, 301));
    }

    #[test]
    fn gross_ceiling_still_bounds_a_starved_transaction() {
        assert!(!data_token_wait_expired(1_000, 1_999));
        assert!(data_token_wait_expired(1_000, 2_000));
    }
}
