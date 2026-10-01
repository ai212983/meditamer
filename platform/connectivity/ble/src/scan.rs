//! Bounded, restartable scan windows (shared BLE runtime and roles plan,
//! Phase 2/3: "Scan in fixed windows, keep a bounded set of results, and
//! recognize one chosen advertising device").
//!
//! Host-testable and clock-free: every call that needs "now" takes it as an
//! explicit `now_ticks: u64` parameter (`platform/connectivity/arbitration`'s pattern),
//! so restart, deadline, full-capacity, and stale-event behavior is
//! deterministic under `cargo test`. A tick is whatever monotonic unit the
//! caller's clock uses; this module never interprets its magnitude, only
//! compares it.

use heapless::{String, Vec};

use crate::capacity::{SCAN_NAME_MAX, SCAN_RESULTS_MAX};

/// One BLE advertiser observed during a [`ScanWindow`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanResult {
    /// Public or random device address, as advertised.
    pub address: [u8; 6],
    /// Advertised local name, if any AD structure carried one. Truncation
    /// never happens (see [`crate::capacity::SCAN_NAME_MAX`]'s doc); a name
    /// that would not fit is rejected by [`ScanWindow::observe`] instead.
    pub name: String<SCAN_NAME_MAX>,
    /// Last received signal strength, in dBm.
    pub rssi: i8,
    /// The `now_ticks` value at the most recent sighting of this address.
    pub last_seen_ticks: u64,
}

/// One raw advertisement, as the caller decoded it off the radio.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Advertisement<'a> {
    pub address: [u8; 6],
    pub name: &'a str,
    pub rssi: i8,
}

/// Why [`ScanWindow::observe`] did not record an advertisement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObserveError {
    /// The window's deadline has already passed as of the given tick.
    WindowExpired,
    /// The advertised name does not fit [`crate::capacity::SCAN_NAME_MAX`].
    NameTooLong,
    /// The bounded result set is full and this address is not already in
    /// it. The existing set is left unchanged (plan-wide "drop the newest"
    /// convention, matching the reviewed BLE controller patch's own RX
    /// overflow policy) and [`ScanWindow::overflow_count`] increments.
    ResultSetFull,
}

/// A fixed-duration scan window over a bounded, deduplicated result set.
///
/// Every window has a `generation`, incremented on [`ScanWindow::restart`].
/// [`ScanWindow::observe_if_current`] rejects advertisements tagged with a
/// stale generation instead of ever mixing results across a restart.
pub struct ScanWindow {
    results: Vec<ScanResult, SCAN_RESULTS_MAX>,
    deadline_ticks: u64,
    generation: u32,
    overflow_count: u32,
}

impl ScanWindow {
    /// Start a fresh window ending at `deadline_ticks`, generation 0.
    pub const fn new(deadline_ticks: u64) -> Self {
        Self {
            results: Vec::new(),
            deadline_ticks,
            generation: 0,
            overflow_count: 0,
        }
    }

    /// This window's generation. Callers that hold an advertisement queued
    /// from before a [`ScanWindow::restart`] compare against this to decide
    /// whether it is stale before calling [`ScanWindow::observe`].
    pub const fn generation(&self) -> u32 {
        self.generation
    }

    /// How many observations were dropped because the bounded result set
    /// was full of other addresses.
    pub const fn overflow_count(&self) -> u32 {
        self.overflow_count
    }

    /// The current bounded result set, most-recently-updated order is not
    /// guaranteed.
    pub fn results(&self) -> &[ScanResult] {
        &self.results
    }

    /// Whether `now_ticks` is at or past this window's deadline.
    pub const fn is_expired(&self, now_ticks: u64) -> bool {
        now_ticks >= self.deadline_ticks
    }

    /// Clear the result set and overflow counter, bump the generation, and
    /// set a new deadline. Neutral state after a restart: nothing from the
    /// previous window survives (plan Phase 3: "repeated runtime restarts").
    pub fn restart(&mut self, new_deadline_ticks: u64) {
        self.results.clear();
        self.overflow_count = 0;
        self.generation = self.generation.wrapping_add(1);
        self.deadline_ticks = new_deadline_ticks;
    }

    /// Record or update one advertisement, only if `observed_generation`
    /// still matches [`ScanWindow::generation`]. Use this for
    /// advertisements that were queued (e.g. across an await point) and
    /// might have outlived a restart; use [`ScanWindow::observe`] directly
    /// when the caller has no such queuing.
    pub fn observe_if_current(
        &mut self,
        observed_generation: u32,
        now_ticks: u64,
        advertisement: &Advertisement<'_>,
    ) -> Result<(), ObserveError> {
        if observed_generation != self.generation {
            // A stale event: silently dropped, not an error -- the plan's
            // "stale events" case is defined behavior, not a fault.
            return Ok(());
        }
        self.observe(now_ticks, advertisement)
    }

    /// Record or update one advertisement against the current generation.
    pub fn observe(
        &mut self,
        now_ticks: u64,
        advertisement: &Advertisement<'_>,
    ) -> Result<(), ObserveError> {
        if self.is_expired(now_ticks) {
            return Err(ObserveError::WindowExpired);
        }
        let name = String::try_from(advertisement.name).map_err(|_| ObserveError::NameTooLong)?;

        if let Some(existing) = self
            .results
            .iter_mut()
            .find(|result| result.address == advertisement.address)
        {
            existing.name = name;
            existing.rssi = advertisement.rssi;
            existing.last_seen_ticks = now_ticks;
            return Ok(());
        }

        self.results
            .push(ScanResult {
                address: advertisement.address,
                name,
                rssi: advertisement.rssi,
                last_seen_ticks: now_ticks,
            })
            .map_err(|_| {
                self.overflow_count = self.overflow_count.saturating_add(1);
                ObserveError::ResultSetFull
            })
    }

    /// Find the result for one chosen advertising device, if it has been
    /// observed in this window (plan Phase 3: "recognize one chosen
    /// advertising device").
    pub fn recognize(&self, address: &[u8; 6]) -> Option<&ScanResult> {
        self.results
            .iter()
            .find(|result| &result.address == address)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn advertisement(address: [u8; 6], name: &str, rssi: i8) -> Advertisement<'_> {
        Advertisement {
            address,
            name,
            rssi,
        }
    }

    #[test]
    fn observes_and_recognizes_a_chosen_device() {
        let mut window = ScanWindow::new(1_000);
        let target = [1, 2, 3, 4, 5, 6];
        window
            .observe(10, &advertisement(target, "target", -40))
            .unwrap();
        window
            .observe(20, &advertisement([9, 9, 9, 9, 9, 9], "other", -60))
            .unwrap();

        let found = window.recognize(&target).expect("target observed");
        assert_eq!(found.name.as_str(), "target");
        assert_eq!(found.rssi, -40);
        assert_eq!(found.last_seen_ticks, 10);
        assert!(window.recognize(&[0; 6]).is_none());
    }

    #[test]
    fn repeated_sightings_update_in_place_without_growing_the_set() {
        let mut window = ScanWindow::new(1_000);
        let address = [1; 6];
        window
            .observe(10, &advertisement(address, "name", -50))
            .unwrap();
        window
            .observe(20, &advertisement(address, "name", -30))
            .unwrap();

        assert_eq!(window.results().len(), 1);
        assert_eq!(window.recognize(&address).unwrap().rssi, -30);
        assert_eq!(window.recognize(&address).unwrap().last_seen_ticks, 20);
    }

    #[test]
    fn full_result_set_drops_new_addresses_and_counts_overflow() {
        let mut window = ScanWindow::new(1_000);
        for index in 0..SCAN_RESULTS_MAX {
            let address = [index as u8; 6];
            window
                .observe(10, &advertisement(address, "name", -50))
                .unwrap();
        }
        assert_eq!(window.results().len(), SCAN_RESULTS_MAX);

        let overflow_address = [200; 6];
        let error = window
            .observe(11, &advertisement(overflow_address, "overflow", -20))
            .unwrap_err();
        assert_eq!(error, ObserveError::ResultSetFull);
        assert_eq!(window.overflow_count(), 1);
        assert_eq!(window.results().len(), SCAN_RESULTS_MAX);
        assert!(window.recognize(&overflow_address).is_none());

        // An update to an address already in the full set still succeeds.
        window
            .observe(12, &advertisement([0; 6], "renamed", -10))
            .unwrap();
        assert_eq!(window.recognize(&[0; 6]).unwrap().rssi, -10);
    }

    #[test]
    fn window_expiry_rejects_further_observations() {
        let mut window = ScanWindow::new(100);
        assert!(!window.is_expired(99));
        assert!(window.is_expired(100));

        let error = window
            .observe(100, &advertisement([1; 6], "late", -50))
            .unwrap_err();
        assert_eq!(error, ObserveError::WindowExpired);
    }

    #[test]
    fn restart_clears_results_overflow_and_bumps_generation() {
        let mut window = ScanWindow::new(100);
        for index in 0..SCAN_RESULTS_MAX {
            window
                .observe(10, &advertisement([index as u8; 6], "filler", -50))
                .unwrap();
        }
        assert_eq!(window.results().len(), SCAN_RESULTS_MAX);
        // Every address 0..SCAN_RESULTS_MAX is now taken; this one is new.
        let overflow_target = [200; 6];
        assert!(window
            .observe(10, &advertisement(overflow_target, "x", -1))
            .is_err());
        assert_eq!(window.overflow_count(), 1);
        let generation_before = window.generation();

        window.restart(500);

        assert!(window.results().is_empty());
        assert_eq!(window.overflow_count(), 0);
        assert_eq!(window.generation(), generation_before.wrapping_add(1));
        assert!(!window.is_expired(100));
        assert!(window.is_expired(500));
    }

    #[test]
    fn stale_generation_observations_are_dropped_without_error() {
        let mut window = ScanWindow::new(1_000);
        let stale_generation = window.generation();
        window.restart(2_000);

        window
            .observe_if_current(
                stale_generation,
                1_500,
                &advertisement([1; 6], "stale", -50),
            )
            .expect("stale events are not an error");

        assert!(window.results().is_empty());

        window
            .observe_if_current(
                window.generation(),
                1_500,
                &advertisement([1; 6], "current", -50),
            )
            .unwrap();
        assert_eq!(window.results().len(), 1);
    }

    #[test]
    fn name_too_long_is_rejected() {
        let mut window = ScanWindow::new(1_000);
        let mut long_name = heapless::String::<64>::new();
        for _ in 0..(SCAN_NAME_MAX + 1) {
            long_name.push('a').unwrap();
        }
        let error = window
            .observe(10, &advertisement([1; 6], long_name.as_str(), -50))
            .unwrap_err();
        assert_eq!(error, ObserveError::NameTooLong);
        assert!(window.results().is_empty());
    }
}
