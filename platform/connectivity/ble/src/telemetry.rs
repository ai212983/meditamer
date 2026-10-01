//! Shared runtime telemetry counters (shared BLE runtime and roles plan,
//! Phase 2).
//!
//! Plain saturating counters, same shape as the existing `ble-foundation`
//! probe's `PHASE1S_*` atomics and the reviewed vendor patch's own queue/RX
//! counters -- but living here as one struct a caller updates and reads
//! explicitly, rather than free-standing statics, so it stays host-testable
//! without a global-state reset between tests.

/// Counters for one shared-runtime session. Every field saturates rather
/// than wrapping, matching this repository's telemetry convention
/// elsewhere (e.g. the reviewed BLE controller patch's queue counters).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    pub scan_overflow: u32,
    pub hid_decode_failures: u32,
    pub input_edge_overflow: u32,
    pub connection_attempts: u32,
    pub connection_faults: u32,
    pub reconnects: u32,
}

impl Counters {
    pub const fn new() -> Self {
        Self {
            scan_overflow: 0,
            hid_decode_failures: 0,
            input_edge_overflow: 0,
            connection_attempts: 0,
            connection_faults: 0,
            reconnects: 0,
        }
    }

    pub fn record_scan_overflow(&mut self) {
        self.scan_overflow = self.scan_overflow.saturating_add(1);
    }

    pub fn record_hid_decode_failure(&mut self) {
        self.hid_decode_failures = self.hid_decode_failures.saturating_add(1);
    }

    pub fn record_input_edge_overflow(&mut self) {
        self.input_edge_overflow = self.input_edge_overflow.saturating_add(1);
    }

    pub fn record_connection_attempt(&mut self) {
        self.connection_attempts = self.connection_attempts.saturating_add(1);
    }

    pub fn record_connection_fault(&mut self) {
        self.connection_faults = self.connection_faults.saturating_add(1);
    }

    pub fn record_reconnect(&mut self) {
        self.reconnects = self.reconnects.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_start_at_zero_and_increment_independently() {
        let mut counters = Counters::new();
        counters.record_scan_overflow();
        counters.record_hid_decode_failure();
        counters.record_hid_decode_failure();

        assert_eq!(counters.scan_overflow, 1);
        assert_eq!(counters.hid_decode_failures, 2);
        assert_eq!(counters.input_edge_overflow, 0);
    }

    #[test]
    fn counters_saturate_instead_of_wrapping() {
        let mut counters = Counters {
            connection_faults: u32::MAX,
            ..Counters::new()
        };
        counters.record_connection_fault();
        assert_eq!(counters.connection_faults, u32::MAX);
    }
}
