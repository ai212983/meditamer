//! Product input publication: complete state plus one bounded,
//! generation-tagged edge queue with clear-and-reconcile recovery.
//!
//! [`InputPublisher`] holds the latest complete [`GamepadSample`] plus one
//! bounded, ordered queue of pending [`ButtonEdge`]s, both tagged with a
//! connection `generation`. Disconnect, decode failure, or edge-queue
//! overflow all go through [`InputPublisher::clear_and_reconcile`]: the
//! queue empties and the latest sample becomes the new neutral baseline, so
//! a consumer draining edges after such an event never replays a stale
//! press/release pair from before it. Host-testable and clock-free, same as
//! [`crate::scan`]: callers supply `now_ticks`.

use heapless::Deque;

use crate::capacity::{EDGE_QUEUE_MAX, GAMEPAD_AXES_MAX};
use crate::hid::DecodedReport;

/// One decoded, product-neutral gamepad sample -- the latest complete state,
/// not a delta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GamepadSample {
    pub buttons: u16,
    pub hat: Option<i32>,
    /// Descriptor-order values; interpret using the active report layout.
    pub axes: [Option<i32>; GAMEPAD_AXES_MAX],
    /// The connection generation this sample was decoded under.
    pub generation: u32,
    /// The caller's monotonic tick when this sample was decoded.
    pub receipt_ticks: u64,
}

impl GamepadSample {
    /// The neutral sample for a fresh or just-recovered connection: no
    /// buttons pressed, no hat/axis data (a consumer should not assume
    /// centered/null values it never actually received).
    pub const fn neutral(generation: u32, receipt_ticks: u64) -> Self {
        Self {
            buttons: 0,
            hat: None,
            axes: [None; GAMEPAD_AXES_MAX],
            generation,
            receipt_ticks,
        }
    }

    fn from_decoded(decoded: &DecodedReport, generation: u32, receipt_ticks: u64) -> Self {
        Self {
            buttons: decoded.buttons,
            hat: decoded.hat,
            axes: decoded.axes,
            generation,
            receipt_ticks,
        }
    }
}

/// One button transition between two [`GamepadSample`]s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ButtonEdge {
    /// Bit index into [`GamepadSample::buttons`] (0..
    /// [`crate::capacity::GAMEPAD_BUTTONS_MAX`]).
    pub button: u8,
    pub pressed: bool,
    /// The connection generation this edge was produced under.
    pub generation: u32,
    /// The caller's monotonic tick when this edge was produced.
    pub ticks: u64,
}

/// Why [`InputPublisher::publish`] rejected a sample.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublishError {
    /// `generation` does not match [`InputPublisher::generation`]: a stale
    /// event from before the last [`InputPublisher::clear_and_reconcile`].
    StaleGeneration,
}

/// Latest [`GamepadSample`] plus a bounded, ordered, generation-tagged
/// queue of [`ButtonEdge`]s.
pub struct InputPublisher {
    latest: GamepadSample,
    edges: Deque<ButtonEdge, EDGE_QUEUE_MAX>,
    generation: u32,
    overflow_count: u32,
    /// Set by [`Self::clear_and_reconcile`]. While set, [`Self::publish`]
    /// adopts each incoming sample as the new baseline without diffing it
    /// against the previous one -- no edges, pressed or released -- and
    /// only clears once a sample reports every button up (a genuinely
    /// neutral report), not merely the first report to arrive. This is what
    /// stops a button the peer was already holding before the reset from
    /// being read as a fresh press the moment it reports again: the
    /// synthetic neutral baseline `clear_and_reconcile` installs is never
    /// itself diffed against real peer data until the peer has actually
    /// confirmed that baseline.
    awaiting_neutral_report: bool,
}

impl InputPublisher {
    /// Start at generation 0 with a neutral sample.
    pub const fn new() -> Self {
        Self {
            latest: GamepadSample::neutral(0, 0),
            edges: Deque::new(),
            generation: 0,
            overflow_count: 0,
            awaiting_neutral_report: false,
        }
    }

    /// The current connection generation.
    pub const fn generation(&self) -> u32 {
        self.generation
    }

    /// The latest complete sample.
    pub const fn latest(&self) -> &GamepadSample {
        &self.latest
    }

    /// How many edges were dropped because the bounded queue was full since
    /// the last [`InputPublisher::clear_and_reconcile`].
    pub const fn overflow_count(&self) -> u32 {
        self.overflow_count
    }

    /// Pending edges, oldest first.
    pub fn pending_edges(&self) -> impl Iterator<Item = &ButtonEdge> {
        self.edges.iter()
    }

    /// Take and clear all pending edges, oldest first.
    pub fn drain_edges(&mut self) -> heapless::Vec<ButtonEdge, EDGE_QUEUE_MAX> {
        let mut drained = heapless::Vec::new();
        while let Some(edge) = self.edges.pop_front() {
            // Capacity matches `edges`' own capacity, so this never fails.
            let _ = drained.push(edge);
        }
        drained
    }

    /// Publish a newly decoded report as this generation's latest sample,
    /// diffing it against the previous sample's buttons to append edges.
    /// Overflow drops the newest edge and counts it (plan-wide convention),
    /// but the sample itself always publishes -- an edge-queue overflow
    /// must never make [`InputPublisher::latest`] stale.
    ///
    /// While [`Self::clear_and_reconcile`]'s suppression is active, this
    /// adopts `decoded` as the new baseline without diffing -- no edges at
    /// all, held or fresh -- until a sample proves every button up; see
    /// that method and the `awaiting_neutral_report` field for why.
    pub fn publish(
        &mut self,
        generation: u32,
        now_ticks: u64,
        decoded: &DecodedReport,
    ) -> Result<(), PublishError> {
        if generation != self.generation {
            return Err(PublishError::StaleGeneration);
        }

        let sample = GamepadSample::from_decoded(decoded, generation, now_ticks);

        if self.awaiting_neutral_report {
            if sample.buttons == 0 {
                self.awaiting_neutral_report = false;
            }
            self.latest = sample;
            return Ok(());
        }

        let previous_buttons = self.latest.buttons;
        let changed = previous_buttons ^ sample.buttons;
        for button in 0..16u8 {
            if changed & (1 << button) == 0 {
                continue;
            }
            let pressed = sample.buttons & (1 << button) != 0;
            let edge = ButtonEdge {
                button,
                pressed,
                generation,
                ticks: now_ticks,
            };
            if self.edges.push_back(edge).is_err() {
                self.overflow_count = self.overflow_count.saturating_add(1);
            }
        }

        self.latest = sample;
        Ok(())
    }

    /// Clear the pending-edge queue and overflow counter, bump the
    /// generation, set a fresh neutral sample as the new baseline, and
    /// suppress edge production until a sample actually confirms that
    /// baseline. Call this on disconnect, HID decode failure, or after
    /// observing [`InputPublisher::overflow_count`] rise -- the plan's
    /// "clear and reconcile" recovery for all three triggers, and the same
    /// "cancel the owner, do not synthesize release or click" contract
    /// `CaptureGate` and `Gpio36Classifier::cancel_accepted` hold for local
    /// input: whatever was held before this fires is simply over, not
    /// released, and must not resurface as a fresh press the moment the
    /// peer's next report still shows it down.
    pub fn clear_and_reconcile(&mut self, now_ticks: u64) {
        self.edges.clear();
        self.overflow_count = 0;
        self.generation = self.generation.wrapping_add(1);
        self.latest = GamepadSample::neutral(self.generation, now_ticks);
        self.awaiting_neutral_report = true;
    }
}

impl Default for InputPublisher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hid::DecodedReport;

    fn decoded(buttons: u16) -> DecodedReport {
        DecodedReport {
            buttons,
            hat: None,
            axes: [None; GAMEPAD_AXES_MAX],
        }
    }

    #[test]
    fn publish_updates_latest_and_appends_edges_for_changed_buttons() {
        let mut publisher = InputPublisher::new();
        publisher.publish(0, 10, &decoded(0b0000_0001)).unwrap();

        assert_eq!(publisher.latest().buttons, 0b0000_0001);
        let edges: heapless::Vec<_, EDGE_QUEUE_MAX> = publisher.pending_edges().copied().collect();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].button, 0);
        assert!(edges[0].pressed);
        assert_eq!(edges[0].ticks, 10);
    }

    #[test]
    fn multiple_simultaneous_button_changes_produce_ordered_edges() {
        let mut publisher = InputPublisher::new();
        publisher.publish(0, 10, &decoded(0b0000_0000)).unwrap();
        publisher.publish(0, 20, &decoded(0b0000_0101)).unwrap();

        let edges: heapless::Vec<_, EDGE_QUEUE_MAX> = publisher.pending_edges().copied().collect();
        assert_eq!(edges.len(), 2);
        assert_eq!(edges[0].button, 0);
        assert_eq!(edges[1].button, 2);
        assert!(edges.iter().all(|edge| edge.pressed));
    }

    #[test]
    fn releasing_a_button_produces_a_release_edge() {
        let mut publisher = InputPublisher::new();
        publisher.publish(0, 10, &decoded(0b0000_0001)).unwrap();
        publisher.drain_edges();
        publisher.publish(0, 20, &decoded(0b0000_0000)).unwrap();

        let edges: heapless::Vec<_, EDGE_QUEUE_MAX> = publisher.pending_edges().copied().collect();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].button, 0);
        assert!(!edges[0].pressed);
    }

    #[test]
    fn drain_edges_empties_the_queue_in_order() {
        let mut publisher = InputPublisher::new();
        publisher.publish(0, 10, &decoded(0b0000_0011)).unwrap();

        let drained = publisher.drain_edges();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].button, 0);
        assert_eq!(drained[1].button, 1);
        assert_eq!(publisher.pending_edges().count(), 0);
    }

    #[test]
    fn full_edge_queue_drops_newest_edges_and_counts_overflow_but_latest_still_publishes() {
        let mut publisher = InputPublisher::new();
        // Fill the queue with EDGE_QUEUE_MAX press edges across distinct
        // buttons, one publish per bit so every edge is queued.
        let mut buttons: u16 = 0;
        for button in 0..(EDGE_QUEUE_MAX as u8) {
            buttons |= 1 << button;
            publisher
                .publish(0, u64::from(button), &decoded(buttons))
                .unwrap();
        }
        assert_eq!(publisher.pending_edges().count(), EDGE_QUEUE_MAX);
        assert_eq!(publisher.overflow_count(), 0);

        // Every bit 0..EDGE_QUEUE_MAX is already pressed and queued (16
        // bits, the queue's own capacity). Releasing one more button is a
        // 17th distinct edge, which the full queue cannot hold.
        buttons &= !1;
        publisher.publish(0, 99, &decoded(buttons)).unwrap();

        assert_eq!(publisher.overflow_count(), 1);
        assert_eq!(publisher.pending_edges().count(), EDGE_QUEUE_MAX);
        // The sample itself still published despite the overflow.
        assert_eq!(publisher.latest().buttons, buttons);
    }

    #[test]
    fn stale_generation_publish_is_rejected() {
        let mut publisher = InputPublisher::new();
        publisher.clear_and_reconcile(100);
        assert_eq!(publisher.generation(), 1);

        let error = publisher.publish(0, 200, &decoded(1)).unwrap_err();
        assert_eq!(error, PublishError::StaleGeneration);
        assert_eq!(publisher.latest().buttons, 0);
    }

    #[test]
    fn clear_and_reconcile_empties_queue_resets_overflow_and_bumps_generation_to_neutral() {
        let mut publisher = InputPublisher::new();
        publisher.publish(0, 10, &decoded(0b0000_0001)).unwrap();
        assert_eq!(publisher.pending_edges().count(), 1);

        publisher.clear_and_reconcile(500);

        assert_eq!(publisher.pending_edges().count(), 0);
        assert_eq!(publisher.overflow_count(), 0);
        assert_eq!(publisher.generation(), 1);
        let neutral = publisher.latest();
        assert_eq!(neutral.buttons, 0);
        assert_eq!(neutral.hat, None);
        assert!(neutral.axes.iter().all(Option::is_none));
        assert_eq!(neutral.generation, 1);
        assert_eq!(neutral.receipt_ticks, 500);
    }

    #[test]
    fn reconcile_after_decode_failure_never_replays_pre_failure_edges() {
        let mut publisher = InputPublisher::new();
        publisher.publish(0, 10, &decoded(0b0000_0001)).unwrap();
        // Simulate a decode failure: the caller does not publish a sample
        // for the malformed report, but does recover.
        publisher.clear_and_reconcile(50);

        assert_eq!(publisher.pending_edges().count(), 0);

        // The reconnected peer's first report still shows the same button
        // held from before the failure -- suppressed, not a fresh press.
        publisher.publish(1, 55, &decoded(0b0000_0001)).unwrap();
        assert_eq!(publisher.pending_edges().count(), 0);
        assert_eq!(publisher.latest().buttons, 0b0000_0001);

        // A genuinely neutral report ends suppression -- itself no edge,
        // since the queue never held a stale press for it to release.
        publisher.publish(1, 58, &decoded(0)).unwrap();
        assert_eq!(publisher.pending_edges().count(), 0);

        // Only now does a real edge get produced.
        publisher.publish(1, 60, &decoded(0b0000_0010)).unwrap();
        let edges: heapless::Vec<_, EDGE_QUEUE_MAX> = publisher.pending_edges().copied().collect();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].button, 1);
        assert!(edges[0].pressed);
    }

    /// The general form of the case above: suppression holds across any
    /// number of reports until one actually reports every button up, not
    /// just the first report after reconcile -- a button released and a
    /// different one pressed in the interim still produces no edges.
    #[test]
    fn clear_and_reconcile_suppresses_every_report_until_a_neutral_one_arrives() {
        let mut publisher = InputPublisher::new();
        publisher.publish(0, 10, &decoded(0b0000_0011)).unwrap();
        publisher.clear_and_reconcile(50);

        publisher.publish(1, 55, &decoded(0b0000_0011)).unwrap();
        publisher.publish(1, 60, &decoded(0b0000_0001)).unwrap();
        publisher.publish(1, 65, &decoded(0b0000_0101)).unwrap();
        assert_eq!(publisher.pending_edges().count(), 0);
        assert_eq!(publisher.latest().buttons, 0b0000_0101);

        publisher.publish(1, 70, &decoded(0)).unwrap();
        assert_eq!(publisher.pending_edges().count(), 0);

        publisher.publish(1, 75, &decoded(0b0000_0001)).unwrap();
        let edges: heapless::Vec<_, EDGE_QUEUE_MAX> = publisher.pending_edges().copied().collect();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].button, 0);
        assert!(edges[0].pressed);
    }

    /// A reconnect whose very first report is already neutral ends
    /// suppression immediately, with no edge for arriving at a baseline
    /// that was already the current one.
    #[test]
    fn clear_and_reconcile_then_an_immediately_neutral_report_ends_suppression_without_an_edge() {
        let mut publisher = InputPublisher::new();
        publisher.publish(0, 10, &decoded(0b0000_0001)).unwrap();
        publisher.clear_and_reconcile(50);

        publisher.publish(1, 55, &decoded(0)).unwrap();
        assert_eq!(publisher.pending_edges().count(), 0);

        publisher.publish(1, 60, &decoded(0b0000_0001)).unwrap();
        let edges: heapless::Vec<_, EDGE_QUEUE_MAX> = publisher.pending_edges().copied().collect();
        assert_eq!(edges.len(), 1);
        assert!(edges[0].pressed);
    }
}
