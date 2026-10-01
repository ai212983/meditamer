//! Timer-driven analog-clock scheduling tests: anchored Embassy deadlines
//! with a minimal RTC budget.
//!
//! Part 1 drives the REAL unmodified product policy
//! (`products/meditamer/src/firmware/ui/screen/analog_clock/model.rs`,
//! re-exported as `ui_shell_host_harness::analog_clock_model`):
//! [`WallAnchor`] estimation, absolute wall-minute boundaries, the shared
//! intent table, prefetch selection, and the RTC-free settle decision.
//!
//! Part 2 executes the REAL, unmodified production runner
//! (`products/meditamer/src/firmware/display/presentation/runtime_clock.rs`,
//! via `#[path]`) against a thin scripted backend that counts RTC
//! round trips. It proves the production path -- not a mirror -- spends
//! zero snapshots per row step, holds future frames without RTC, validates
//! Clean boundaries with exactly one snapshot, and recovers stale frames
//! with exactly one truth-check snapshot.
//!
//! Fixed midnights use synthetic local-epoch seconds (`t(h, m, s)`), the
//! same convention as the cadence suite.

use ui_shell_host_harness::analog_clock_model::{
    self, epoch_minute, intent_for_epoch, prefetch_epoch, settle_estimate, MinuteIntent,
    MinuteTracker, RenderTiming, TimeObservation, WallAnchor,
};

/// Local wall time under test, in seconds since the local epoch.
fn t(hour: u32, minute: u32, second: u32) -> u32 {
    hour * 3_600 + minute * 60 + second
}

// ---------------------------------------------------------------------------
// Part 1: anchored policy on the real product model.
// ---------------------------------------------------------------------------

#[test]
fn anchor_estimates_current_minute_from_monotonic_elapsed() {
    let anchor = WallAnchor::new(406_540, t(18, 30, 26));
    assert_eq!(
        anchor.estimated_epoch_minute(406_540),
        epoch_minute(t(18, 30, 26))
    );
    // 33 s later the estimate is still the same wall minute.
    assert_eq!(
        anchor.estimated_epoch_minute(406_540 + 33_000),
        epoch_minute(t(18, 30, 26))
    );
    // 34 s later the minute rolled over by estimate alone, no RTC.
    assert_eq!(
        anchor.estimated_epoch_minute(406_540 + 34_000),
        epoch_minute(t(18, 31, 0))
    );
}

#[test]
fn boundary_18_09_59_to_18_10_00_is_absolute_and_stable() {
    let anchor = WallAnchor::new(1_000, t(18, 9, 59));
    let upcoming = epoch_minute(t(18, 10, 0));
    // The 18:10:00 wall boundary is exactly 1 s after the anchor read.
    assert_eq!(anchor.boundary_ms(upcoming), 2_000);
    // Repeated calls return the same absolute deadline: never sliding.
    assert_eq!(anchor.boundary_ms(upcoming), 2_000);
    // 1 ms before the boundary the estimate still reads 18:09 ...
    assert_eq!(
        anchor.estimated_epoch_minute(1_999),
        epoch_minute(t(18, 9, 59))
    );
    // ... and exactly at the boundary it reads 18:10.
    assert_eq!(anchor.estimated_epoch_minute(2_000), upcoming);
}

#[test]
fn future_frame_holds_then_publishes_clean_at_boundary() {
    let anchor = WallAnchor::new(1_000, t(18, 9, 59));
    let upcoming = epoch_minute(t(18, 10, 0));
    // Landing on minute 10 of the hour takes the full-refresh path, and the
    // 10-minute class comes from the *target* minute, not elapsed time.
    assert_eq!(
        intent_for_epoch(Some(epoch_minute(t(18, 9, 0))), upcoming),
        MinuteIntent::Clean
    );
    // 1 ms before the boundary: hold until the absolute instant, no publish.
    assert_eq!(
        settle_estimate(
            upcoming,
            MinuteIntent::Clean,
            epoch_minute(t(18, 9, 59)),
            &anchor
        ),
        analog_clock_model::EstimatedSettle::HoldFuture { boundary_ms: 2_000 }
    );
    // At the boundary the same frame publishes Clean.
    assert_eq!(
        settle_estimate(upcoming, MinuteIntent::Clean, upcoming, &anchor),
        analog_clock_model::EstimatedSettle::PublishCurrent(MinuteIntent::Clean)
    );
}

#[test]
fn ordinary_minute_publishes_fast_at_boundary() {
    let anchor = WallAnchor::new(1_000, t(18, 10, 20));
    let upcoming = epoch_minute(t(18, 11, 0));
    assert_eq!(
        intent_for_epoch(Some(epoch_minute(t(18, 10, 0))), upcoming),
        MinuteIntent::Fast
    );
    // Held before the boundary ...
    assert!(matches!(
        settle_estimate(
            upcoming,
            MinuteIntent::Fast,
            epoch_minute(t(18, 10, 59)),
            &anchor
        ),
        analog_clock_model::EstimatedSettle::HoldFuture { .. }
    ));
    // ... published as a strict partial exactly at it.
    assert_eq!(
        settle_estimate(upcoming, MinuteIntent::Fast, upcoming, &anchor),
        analog_clock_model::EstimatedSettle::PublishCurrent(MinuteIntent::Fast)
    );
}

#[test]
fn first_frame_targets_current_minute_clean() {
    let anchor = WallAnchor::new(7_000, t(9, 2, 40));
    let estimated = anchor.estimated_epoch_minute(7_000);
    // First/reentry entry renders the current minute immediately, never a
    // prefetched future: the retained canvas may be empty or foreign.
    assert_eq!(prefetch_epoch(None, estimated), estimated);
    assert_eq!(intent_for_epoch(None, estimated), MinuteIntent::Clean);
}

#[test]
fn steady_state_prefetches_upcoming_minute() {
    let published = epoch_minute(t(18, 9, 0));
    // Current minute already on the panel: prepare the next one during the
    // preceding minute so it can publish exactly at the boundary.
    assert_eq!(prefetch_epoch(Some(published), published), published + 1);
    // A missed boundary while idle stages the upcoming minute instead of
    // restarting a catch-up render that cannot finish before the next
    // boundary; the skipped minute is recovery, then cadence stabilizes.
    assert_eq!(
        prefetch_epoch(Some(published), published + 2),
        published + 3
    );
    // A backwards jump the estimate reveals also stages the upcoming minute;
    // the intent table still classifies the estimate itself as Clean.
    assert_eq!(
        prefetch_epoch(Some(published + 5), published),
        published + 1
    );
    assert_eq!(
        intent_for_epoch(Some(published + 5), published),
        MinuteIntent::Clean
    );
}

#[test]
fn first_publish_straddling_boundary_stages_upcoming() {
    // Cold first frame published at 18:09:59, panel waveform straddles the
    // boundary so the next estimate already reads 18:10: stage 18:11, not a
    // repeated 18:10 catch-up that renders at :54/:51 instead of :00.
    let first = epoch_minute(t(18, 9, 59));
    let estimated = epoch_minute(t(18, 10, 0));
    assert_eq!(estimated, first + 1);
    let target = prefetch_epoch(Some(first), estimated);
    assert_eq!(target, epoch_minute(t(18, 11, 0)));
    // The staged target skips over the 18:10 Clean boundary, so it recovers
    // Clean via the unchanged intent table.
    assert_eq!(intent_for_epoch(Some(first), target), MinuteIntent::Clean);
}

#[test]
fn render_overrun_leaving_last_older_stages_upcoming() {
    // A ~55 s render overrun leaves last at 18:08 while the estimate already
    // reads 18:10: stage 18:11 and hold its deadline, skipping the initial
    // catch-up minute as recovery.
    let last = epoch_minute(t(18, 8, 0));
    let estimated = epoch_minute(t(18, 10, 0));
    let target = prefetch_epoch(Some(last), estimated);
    assert_eq!(target, epoch_minute(t(18, 11, 0)));
    assert_eq!(intent_for_epoch(Some(last), target), MinuteIntent::Clean);
}

/// A measured 55 s render records a 57 s budget (render + 2 s margin).
fn timing_with_55s_render() -> RenderTiming {
    let mut timing = RenderTiming::new();
    let epoch = epoch_minute(t(18, 10, 0));
    timing.begin(epoch, 1_000);
    timing.complete(epoch, 56_000);
    assert_eq!(timing.budget_ms(), 57_000);
    timing
}

#[test]
fn measured_late_first_publish_stages_two_minutes_out() {
    // First frame publishes at 18:10:55; the 18:11 boundary is only 5 s
    // away, far short of the measured 57 s budget, so the policy stages
    // 18:12 (estimated + 2) instead of an 18:11 catch-up that would
    // publish at :51 again.
    let timing = timing_with_55s_render();
    let anchor = WallAnchor::new(0, t(18, 10, 0));
    let now_ms = 55_000;
    assert_eq!(
        anchor.estimated_epoch_minute(now_ms),
        epoch_minute(t(18, 10, 0))
    );
    let last = epoch_minute(t(18, 10, 0));
    assert_eq!(
        timing.planned_prefetch(Some(last), &anchor, now_ms),
        epoch_minute(t(18, 12, 0))
    );
}

#[test]
fn measured_steady_state_stages_next_minute() {
    // Same 57 s budget, but half a second into 18:11: ~59.5 s remain to
    // the 18:12 boundary, which fits, so steady state stages the normal
    // upcoming minute (estimated + 1).
    let timing = timing_with_55s_render();
    let anchor = WallAnchor::new(0, t(18, 10, 0));
    let now_ms = 60_500;
    assert_eq!(
        anchor.estimated_epoch_minute(now_ms),
        epoch_minute(t(18, 11, 0))
    );
    let last = epoch_minute(t(18, 10, 0));
    assert_eq!(
        timing.planned_prefetch(Some(last), &anchor, now_ms),
        epoch_minute(t(18, 12, 0))
    );
}

#[test]
fn unknown_phase_stays_conservative() {
    // Before the first sample the default 60 s budget applies: the same
    // late :55 publish still stages two minutes out rather than guessing
    // an optimistic render.
    let timing = RenderTiming::new();
    assert_eq!(timing.budget_ms(), RenderTiming::DEFAULT_BUDGET_MS);
    let anchor = WallAnchor::new(0, t(18, 10, 0));
    let last = epoch_minute(t(18, 10, 0));
    assert_eq!(
        timing.planned_prefetch(Some(last), &anchor, 55_000),
        epoch_minute(t(18, 12, 0))
    );
}

#[test]
fn planned_prefetch_before_first_publish_targets_current() {
    // First ever frame renders immediately even with a measured budget:
    // the retained canvas may be empty or foreign.
    let timing = timing_with_55s_render();
    let anchor = WallAnchor::new(0, t(18, 10, 0));
    let now_ms = 55_000;
    let estimated = anchor.estimated_epoch_minute(now_ms);
    assert_eq!(timing.planned_prefetch(None, &anchor, now_ms), estimated);
}

#[test]
fn planned_prefetch_long_render_reaches_further_future() {
    // A 123 s render honestly stages past the next boundary: at :00.5
    // with a 125 s budget the 18:12 boundary (119.5 s out) still falls
    // short, so the target is 18:14.
    let mut timing = RenderTiming::new();
    let epoch = epoch_minute(t(18, 10, 0));
    timing.begin(epoch, 1_000);
    timing.complete(epoch, 124_000);
    assert_eq!(timing.budget_ms(), 125_000);
    let anchor = WallAnchor::new(0, t(18, 10, 0));
    let last = epoch_minute(t(18, 10, 0));
    assert_eq!(
        timing.planned_prefetch(Some(last), &anchor, 60_500),
        epoch_minute(t(18, 14, 0))
    );
}

#[test]
fn render_sample_not_restarted_per_row() {
    // Row ticks share one sample: repeated begins for the same epoch keep
    // the first instant, so the budget measures the whole frame.
    let mut timing = RenderTiming::new();
    let epoch = epoch_minute(t(18, 10, 0));
    timing.begin(epoch, 1_000);
    timing.begin(epoch, 2_000);
    timing.begin(epoch, 3_000);
    timing.complete(epoch, 56_000);
    assert_eq!(timing.budget_ms(), 57_000);
}

#[test]
fn render_sample_stale_epoch_resets() {
    // A retarget starts a new sample; completing the stale epoch records
    // nothing.
    let mut timing = RenderTiming::new();
    let first = epoch_minute(t(18, 10, 0));
    let second = epoch_minute(t(18, 11, 0));
    timing.begin(first, 1_000);
    timing.begin(second, 2_000);
    timing.complete(first, 50_000);
    assert_eq!(
        timing.budget_ms(),
        RenderTiming::DEFAULT_BUDGET_MS,
        "stale epoch completion must not record"
    );
    timing.complete(second, 57_000);
    assert_eq!(timing.budget_ms(), 57_000);
}

#[test]
fn render_budget_saturated_values_stay_bounded() {
    // A backwards clock step saturates to the bare margin, never wrapping.
    let mut timing = RenderTiming::new();
    let epoch = epoch_minute(t(18, 10, 0));
    timing.begin(epoch, 10_000);
    timing.complete(epoch, 5_000);
    assert_eq!(timing.budget_ms(), RenderTiming::SCHEDULING_MARGIN_MS);
    // A maximal budget near the monotonic ceiling still resolves in pure
    // arithmetic with no loop: finite, at or past the base candidate.
    let mut timing = RenderTiming::new();
    timing.begin(epoch, 0);
    timing.complete(epoch, u64::MAX);
    assert_eq!(timing.budget_ms(), u64::MAX);
    let anchor = WallAnchor::new(0, t(18, 10, 0));
    let now_ms = u64::MAX - 1_000;
    let estimated = anchor.estimated_epoch_minute(now_ms);
    let base = prefetch_epoch(Some(estimated), estimated);
    let planned = timing.planned_prefetch(Some(estimated), &anchor, now_ms);
    assert!(
        planned >= base,
        "saturated plan must stay past the candidate"
    );
    assert!(planned != u64::MAX || base == u64::MAX);
}

#[test]
fn prefetch_before_first_publish_targets_current_then_stages_next() {
    // Before any publish the first frame renders immediately.
    let estimated = epoch_minute(t(9, 2, 40));
    assert_eq!(prefetch_epoch(None, estimated), estimated);
    // Once established (last == current), the idle engine stages next.
    assert_eq!(prefetch_epoch(Some(estimated), estimated), estimated + 1);
    // Hour rollover: last 09:59, estimate 10:00 -> stage 10:01.
    let last_hour = epoch_minute(t(9, 59, 0));
    let current_hour = epoch_minute(t(10, 0, 0));
    assert_eq!(
        prefetch_epoch(Some(last_hour), current_hour),
        epoch_minute(t(10, 1, 0))
    );
    // Midnight rollover: last 23:59, estimate 00:00 (next day) -> stage 00:01.
    let last_midnight = epoch_minute(t(23, 59, 0));
    let current_midnight = epoch_minute(86_400);
    assert_eq!(
        prefetch_epoch(Some(last_midnight), current_midnight),
        current_midnight + 1
    );
    assert_eq!(
        intent_for_epoch(Some(last_midnight), current_midnight + 1),
        MinuteIntent::Clean,
        "staging across minute 0 keeps the hour-boundary Clean recovery"
    );
}

#[test]
fn midnight_and_hour_rollover() {
    // Anchor at 23:59:50; 10 s of monotonic elapsed crosses midnight.
    let anchor = WallAnchor::new(5_000, t(23, 59, 50));
    assert_eq!(anchor.estimate_local_epoch(15_000), 86_400);
    let rolled = anchor.estimated_epoch_minute(15_000);
    assert_eq!(rolled, epoch_minute(86_400));
    let (hour, minute) = analog_clock_model::clock_h_m(86_400);
    assert_eq!((hour, minute), (0, 0));
    // Landing on minute 0 of the hour recovers Clean (hour boundary).
    assert_eq!(
        intent_for_epoch(Some(epoch_minute(t(23, 59, 50))), rolled),
        MinuteIntent::Clean
    );
    // Ordinary hour crossing without a 10-minute landing stays Fast.
    assert_eq!(
        intent_for_epoch(Some(epoch_minute(t(9, 58, 0))), epoch_minute(t(9, 59, 0))),
        MinuteIntent::Fast
    );
}

#[test]
fn skipped_boundary_and_backwards_step_are_clean() {
    // Skipping unseen over the 18:10 boundary recovers Clean ...
    assert_eq!(
        intent_for_epoch(Some(epoch_minute(t(18, 8, 0))), epoch_minute(t(18, 11, 0))),
        MinuteIntent::Clean
    );
    // ... while a skip that crosses no 10-minute boundary stays Fast ...
    assert_eq!(
        intent_for_epoch(Some(epoch_minute(t(18, 11, 0))), epoch_minute(t(18, 13, 0))),
        MinuteIntent::Fast
    );
    // ... and any backwards step recovers Clean.
    assert_eq!(
        intent_for_epoch(Some(epoch_minute(t(18, 30, 0))), epoch_minute(t(18, 9, 0))),
        MinuteIntent::Clean
    );
}

#[test]
fn overrun_frame_never_publishes_as_future_or_current() {
    let anchor = WallAnchor::new(1_000, t(18, 9, 0));
    // A frame two minutes stale is discarded for the current minute, never
    // presented: the retarget carries the estimated current minute.
    assert_eq!(
        settle_estimate(
            epoch_minute(t(18, 9, 0)),
            MinuteIntent::Fast,
            epoch_minute(t(18, 11, 5)),
            &anchor
        ),
        analog_clock_model::EstimatedSettle::StaleRetarget {
            epoch_minute: epoch_minute(t(18, 11, 5))
        }
    );
    // A frame for a minute after the estimate is held, never published.
    assert!(matches!(
        settle_estimate(
            epoch_minute(t(18, 12, 0)),
            MinuteIntent::Fast,
            epoch_minute(t(18, 10, 30)),
            &anchor
        ),
        analog_clock_model::EstimatedSettle::HoldFuture { .. }
    ));
}

#[test]
fn anchor_never_fabricates_time() {
    let anchor = WallAnchor::new(10_000, t(18, 9, 59));
    // A backwards monotonic step saturates instead of wrapping into a
    // fabricated future or past.
    assert_eq!(
        anchor.estimate_local_epoch(5_000),
        t(18, 9, 59),
        "backwards monotonic step must not move the estimate"
    );
    // Far-future minutes keep a finite, ordered boundary.
    let near = anchor.boundary_ms(epoch_minute(t(18, 10, 0)));
    let far = anchor.boundary_ms(epoch_minute(t(19, 10, 0)));
    assert!(far > near);
}

#[test]
fn tracker_observe_and_epoch_table_agree() {
    // The RTC-read classifier and the estimate classifier share one table:
    // every fresh-minute observation matches `intent_for_epoch` for the same
    // last-published minute.
    let mut tracker = MinuteTracker::new();
    let first = t(18, 9, 0);
    let epoch = match tracker.observe(first, 1_000) {
        TimeObservation::Target {
            epoch_minute,
            intent,
        } => {
            assert_eq!(intent, intent_for_epoch(None, epoch_minute));
            epoch_minute
        }
        TimeObservation::Duplicate => panic!("first observation must target"),
    };
    tracker.published(epoch, 1_500);
    for (h, m) in [(18, 10), (18, 11), (18, 20), (19, 0)] {
        let local = t(h, m, 0);
        let current = epoch_minute(local);
        match tracker.observe(local, 61_000) {
            TimeObservation::Target {
                epoch_minute,
                intent,
            } => {
                assert_eq!(epoch_minute, current);
                assert_eq!(intent, intent_for_epoch(tracker.last_published(), current));
                tracker.published(current, 62_000);
            }
            TimeObservation::Duplicate => panic!("fresh minute must target"),
        }
    }
}

// ---------------------------------------------------------------------------
// Part 2: RTC budget of the real production runner.
// ---------------------------------------------------------------------------

extern crate self as rtc;

use std::cell::Cell;
use std::future::Future;
use std::task::{Context, Poll};

pub mod driver {
    /// Minimal wall-clock snapshot: only the fields the presentation path
    /// touches (`valid` filter, `local_epoch_seconds`).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct WallClockSnapshot {
        pub valid: bool,
        pub local_epoch_seconds: u32,
    }
}

std::thread_local! {
    static SCRIPT_SNAPSHOT: Cell<Option<driver::WallClockSnapshot>> =
        const { Cell::new(None) };
    static RTC_CALLS: Cell<usize> = const { Cell::new(0) };
}

fn set_snapshot(snapshot: Option<driver::WallClockSnapshot>) {
    SCRIPT_SNAPSHOT.set(snapshot);
}

fn valid_snapshot(local_epoch_seconds: u32) -> driver::WallClockSnapshot {
    driver::WallClockSnapshot {
        valid: true,
        local_epoch_seconds,
    }
}

fn rtc_calls() -> usize {
    RTC_CALLS.get()
}

fn reset_rtc_calls() {
    RTC_CALLS.set(0);
}

#[path = ""]
mod firmware {
    pub mod types {
        /// Stand-in for the panel driver handle: only the partial-refresh
        /// readiness query production performs.
        #[derive(Clone, Copy, Debug)]
        pub struct FakeInkplate {
            pub partial_ready: bool,
        }

        impl FakeInkplate {
            pub fn is_partial_refresh_ready(&self) -> bool {
                self.partial_ready
            }
        }

        /// Stand-in for the production context: only `inkplate` is read.
        pub struct DisplayContext {
            pub inkplate: FakeInkplate,
        }
    }

    #[path = ""]
    pub mod ui {
        #[path = ""]
        pub mod lvgl {
            // Real clock-model policy: the exact items the production
            // runner consumes.
            pub use ui_shell_host_harness::analog_clock_model::{
                resolve_clock_publish, ClockPublishDecision, EstimatedSettle, MinuteIntent,
            };

            // Real Fast/Clean/Wait resolver, pulled in unmodified.
            #[path = "../../../../products/meditamer/src/firmware/ui/lvgl/screen_update_state.rs"]
            mod screen_update_state;
            pub(crate) use screen_update_state::{
                select_fast_update_operation, FastUpdateOperation,
            };

            /// Opaque damage handle: production only passes it through to
            /// the strict-fast push. A unit struct keeps the pass-through
            /// shape with no unread fields.
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub struct DirtyArea;

            impl DirtyArea {
                pub const FULL: Self = Self;
            }

            /// Thin target-minute handle with the production field shape.
            /// Production reads `epoch_minute` (hold check, stale fallback)
            /// and `intent` (Clean/Fast arm); nothing else.
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub struct TargetMinute {
                pub epoch_minute: u64,
                pub hour: u8,
                pub minute: u8,
                pub intent: MinuteIntent,
            }

            impl TargetMinute {
                pub const fn new(
                    epoch_minute: u64,
                    hour: u8,
                    minute: u8,
                    intent: MinuteIntent,
                ) -> Self {
                    Self {
                        epoch_minute,
                        hour,
                        minute,
                        intent,
                    }
                }
            }

            /// Thin poll answer with the production variant shape.
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub enum ClockPoll {
                Inactive,
                Idle,
                NeedsTime,
                Step,
                PublishReady(TargetMinute),
            }

            /// Thin settle outcome with the production variant shape.
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub enum SettleOutcome {
                Published(MinuteIntent),
                Held,
            }

            /// Observable backend operations, in call order.
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub enum Op {
                Publish,
                ConfirmPublished,
                StrictFast,
                PaintFailure,
                Observe,
                Service,
            }

            /// Scripted backend: answers every `analog_clock_*` call the
            /// production runner makes and records the operation trace. No
            /// renderer, no canvas, no LVGL.
            pub struct Backend {
                pub polls: std::collections::VecDeque<ClockPoll>,
                pub estimated: Option<u64>,
                pub settle: SettleOutcome,
                pub settle_estimated: EstimatedSettle,
                pub damage: Option<DirtyArea>,
                pub pending_clean: bool,
                pub failure_pending: bool,
                pub refresh_outcome: crate::firmware::display::panel::refresh::StrictFastOutcome,
                pub ops: Vec<Op>,
            }

            impl Backend {
                pub fn analog_clock_report_activation(&mut self, _now_ms: u64) {}

                pub fn analog_clock_poll(&mut self, _now_ms: u64) -> ClockPoll {
                    self.polls.pop_front().unwrap_or(ClockPoll::Idle)
                }

                pub fn analog_clock_estimated_current(&self, _now_ms: u64) -> Option<u64> {
                    self.estimated
                }

                pub fn analog_clock_observe(
                    &mut self,
                    _snapshot: Option<rtc::driver::WallClockSnapshot>,
                    _now_ms: u64,
                ) {
                    self.ops.push(Op::Observe);
                }

                pub fn analog_clock_service(&mut self) -> Option<TargetMinute> {
                    self.ops.push(Op::Service);
                    None
                }

                pub fn analog_clock_settle(
                    &mut self,
                    _snapshot: Option<rtc::driver::WallClockSnapshot>,
                    _completed: TargetMinute,
                    _now_ms: u64,
                ) -> SettleOutcome {
                    self.settle
                }

                pub fn analog_clock_settle_estimated(
                    &mut self,
                    _completed: TargetMinute,
                    _now_ms: u64,
                ) -> EstimatedSettle {
                    self.settle_estimated
                }

                pub fn analog_clock_publish(
                    &mut self,
                    _display: &mut crate::firmware::types::FakeInkplate,
                ) -> Option<DirtyArea> {
                    self.ops.push(Op::Publish);
                    self.damage
                }

                pub fn analog_clock_confirm_published(
                    &mut self,
                    _target: TargetMinute,
                    _now_ms: u64,
                ) {
                    self.ops.push(Op::ConfirmPublished);
                }

                pub fn analog_clock_failure_pending(&self) -> bool {
                    self.failure_pending
                }

                pub fn analog_clock_paint_failure(
                    &mut self,
                    _display: &mut crate::firmware::types::FakeInkplate,
                ) {
                    self.ops.push(Op::PaintFailure);
                }

                pub fn pending_screen_update_intent(
                    &mut self,
                ) -> Option<shell::types::ScreenUpdateIntent> {
                    if self.pending_clean {
                        Some(shell::types::ScreenUpdateIntent::Clean)
                    } else {
                        None
                    }
                }
            }
        }
    }

    #[path = ""]
    pub mod display {
        pub mod panel {
            pub mod refresh {
                use crate::firmware::display::presentation::state::PresentationState;
                use crate::firmware::types::DisplayContext;
                use crate::firmware::ui::lvgl::{DirtyArea, Op};

                /// Thin strict-fast outcome with the production variant
                /// shape.
                #[derive(Clone, Copy, Debug, PartialEq, Eq)]
                pub enum StrictFastOutcome {
                    Pushed,
                    NotReady,
                    Failed,
                }

                /// Scripted panel push: records the attempt and answers with
                /// the backend's scripted outcome. No panel traffic.
                pub async fn refresh_panel_strict_fast(
                    _context: &mut DisplayContext,
                    state: &mut PresentationState,
                    _dirty: DirtyArea,
                    _source: &str,
                ) -> StrictFastOutcome {
                    match state.backend.as_mut() {
                        Some(backend) => {
                            backend.ops.push(Op::StrictFast);
                            backend.refresh_outcome
                        }
                        None => StrictFastOutcome::Failed,
                    }
                }
            }
        }

        pub mod wall_clock {
            /// Counted wall-clock fetch: every production RTC round trip
            /// passes through here, so `rtc_calls` is the exact per-test
            /// snapshot budget.
            pub async fn request_wall_clock_snapshot() -> Option<rtc::driver::WallClockSnapshot> {
                crate::RTC_CALLS.set(crate::RTC_CALLS.get() + 1);
                crate::SCRIPT_SNAPSHOT.get()
            }
        }

        #[path = ""]
        pub mod presentation {
            pub mod state {
                use crate::firmware::ui::lvgl::Backend;

                /// Stand-in for the production state: the runner only
                /// touches `backend`.
                pub struct PresentationState {
                    pub backend: Option<Backend>,
                }
            }

            // The REAL, unmodified production runner under test.
            #[path = "../../../../products/meditamer/src/firmware/display/presentation/runtime_clock.rs"]
            mod runtime_clock;

            /// Parent-visibility wrapper: production exposes the runner as
            /// `pub(super)`, so tests call it through this module.
            pub async fn run_clock_update(
                context: &mut crate::firmware::types::DisplayContext,
                state: &mut state::PresentationState,
                allow_clean: bool,
                now_ms: u64,
                clean_reason: &mut Option<&'static str>,
            ) {
                runtime_clock::process_runtime_clock_update(
                    context,
                    state,
                    allow_clean,
                    now_ms,
                    clean_reason,
                )
                .await;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Test support.
// ---------------------------------------------------------------------------

use firmware::display::panel::refresh::StrictFastOutcome;
use firmware::display::presentation::{run_clock_update, state::PresentationState};
use firmware::types::{DisplayContext, FakeInkplate};
use firmware::ui::lvgl::{
    Backend, ClockPoll, DirtyArea, EstimatedSettle, Op, SettleOutcome, TargetMinute,
};

/// Minimal immediate executor: every faked async primitive is ready on its
/// first poll, so no reactor or extra dependency is needed.
fn block_on<F: Future>(future: F) -> F::Output {
    fn noop_raw() -> std::task::RawWaker {
        unsafe fn clone(_: *const ()) -> std::task::RawWaker {
            noop_raw()
        }
        unsafe fn noop(_: *const ()) {}
        static VTABLE: std::task::RawWakerVTable =
            std::task::RawWakerVTable::new(clone, noop, noop, noop);
        std::task::RawWaker::new(std::ptr::null(), &VTABLE)
    }
    let waker = unsafe { std::task::Waker::from_raw(noop_raw()) };
    let mut context = Context::from_waker(&waker);
    let mut boxed = Box::pin(future);
    loop {
        if let Poll::Ready(value) = boxed.as_mut().poll(&mut context) {
            return value;
        }
        std::hint::spin_loop();
    }
}

fn fast_minute(hour: u8, minute: u8) -> TargetMinute {
    let target = TargetMinute::new(
        epoch_minute(t(u32::from(hour), u32::from(minute), 0)),
        hour,
        minute,
        MinuteIntent::Fast,
    );
    // Lock the thin pass-through shape: production reads epoch/intent, but
    // the hand-off still carries the wall hour/minute.
    assert_eq!((target.hour, target.minute), (hour, minute));
    target
}

fn clean_minute(hour: u8, minute: u8) -> TargetMinute {
    let target = TargetMinute::new(
        epoch_minute(t(u32::from(hour), u32::from(minute), 0)),
        hour,
        minute,
        MinuteIntent::Clean,
    );
    assert_eq!((target.hour, target.minute), (hour, minute));
    target
}

fn rig(
    polls: Vec<ClockPoll>,
    estimated: Option<u64>,
    settle: SettleOutcome,
    settle_estimated: EstimatedSettle,
    damage: Option<DirtyArea>,
    partial_ready: bool,
    refresh_outcome: StrictFastOutcome,
) -> (DisplayContext, PresentationState) {
    set_snapshot(Some(valid_snapshot(t(18, 10, 0))));
    reset_rtc_calls();
    let context = DisplayContext {
        inkplate: FakeInkplate { partial_ready },
    };
    let state = PresentationState {
        backend: Some(Backend {
            polls: polls.into(),
            estimated,
            settle,
            settle_estimated,
            damage,
            pending_clean: false,
            failure_pending: false,
            refresh_outcome,
            ops: Vec::new(),
        }),
    };
    (context, state)
}

fn ops_of(state: &PresentationState) -> &[Op] {
    &state.backend.as_ref().expect("backend present").ops
}

fn run_once(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    allow_clean: bool,
    now_ms: u64,
) -> Option<&'static str> {
    let mut clean_reason: Option<&'static str> = None;
    block_on(run_clock_update(
        context,
        state,
        allow_clean,
        now_ms,
        &mut clean_reason,
    ));
    clean_reason
}

// ---------------------------------------------------------------------------
// RTC budget: the real runner's snapshot discipline.
// ---------------------------------------------------------------------------

#[test]
fn full_frame_row_budget_costs_zero_rtc() {
    // 600 compose rows at 4 rows per tick = 150 Step cycles, then the
    // completed ordinary frame publishes. The old path spent one snapshot
    // per Step here (~150/frame); the anchored path spends none.
    let target = fast_minute(18, 11);
    let mut polls = vec![ClockPoll::Step; 150];
    polls.push(ClockPoll::PublishReady(target));
    let (mut context, mut state) = rig(
        polls,
        Some(target.epoch_minute),
        SettleOutcome::Held,
        EstimatedSettle::PublishCurrent(MinuteIntent::Fast),
        Some(DirtyArea::FULL),
        true,
        StrictFastOutcome::Pushed,
    );
    for step in 0..150 {
        let reason = run_once(&mut context, &mut state, true, 1_000 + step * 8);
        assert_eq!(reason, None);
    }
    assert_eq!(rtc_calls(), 0, "row steps must not fetch snapshots");
    let reason = run_once(&mut context, &mut state, true, 61_000);
    assert_eq!(reason, None);
    assert_eq!(rtc_calls(), 0, "ordinary-minute publish must not fetch");
    assert_eq!(
        ops_of(&state)[150..],
        [Op::Publish, Op::StrictFast, Op::ConfirmPublished]
    );
    assert!(
        ops_of(&state)[..150].iter().all(|op| *op == Op::Service),
        "steps do row work only"
    );
}

#[test]
fn future_frame_holds_with_zero_rtc_and_no_panel_traffic() {
    // A completed frame for a minute after the estimate holds: no snapshot,
    // no canvas copy, no panel push, no confirm.
    let target = clean_minute(18, 10);
    let (mut context, mut state) = rig(
        vec![ClockPoll::PublishReady(target)],
        Some(target.epoch_minute - 1),
        SettleOutcome::Held,
        EstimatedSettle::HoldFuture {
            boundary_ms: 99_999,
        },
        Some(DirtyArea::FULL),
        true,
        StrictFastOutcome::Pushed,
    );
    let reason = run_once(&mut context, &mut state, true, 1_000);
    assert_eq!(reason, None);
    assert_eq!(rtc_calls(), 0);
    assert!(ops_of(&state).is_empty());
}

#[test]
fn clean_boundary_publish_costs_exactly_one_rtc() {
    // 10-minute boundaries (and first frames) validate with one snapshot,
    // which doubles as the anchor resync: steady state is one RTC per
    // 10 minutes, not per row or per idle tick.
    let target = clean_minute(18, 10);
    let (mut context, mut state) = rig(
        vec![ClockPoll::PublishReady(target)],
        Some(target.epoch_minute),
        SettleOutcome::Published(MinuteIntent::Clean),
        EstimatedSettle::PublishCurrent(MinuteIntent::Clean),
        Some(DirtyArea::FULL),
        true,
        StrictFastOutcome::Pushed,
    );
    let reason = run_once(&mut context, &mut state, true, 61_000);
    assert_eq!(reason, Some("analog_clock"));
    assert_eq!(rtc_calls(), 1);
    assert_eq!(ops_of(&state), &[Op::Publish, Op::ConfirmPublished]);
}

#[test]
fn stale_estimate_recovers_with_exactly_one_truth_check() {
    // An overrun estimate spends one snapshot to recover from truth, then
    // holds the discarded frame without publishing or confirming it.
    let target = fast_minute(18, 9);
    let (mut context, mut state) = rig(
        vec![ClockPoll::PublishReady(target)],
        Some(target.epoch_minute + 2),
        SettleOutcome::Held,
        EstimatedSettle::StaleRetarget {
            epoch_minute: target.epoch_minute + 2,
        },
        Some(DirtyArea::FULL),
        true,
        StrictFastOutcome::Pushed,
    );
    let reason = run_once(&mut context, &mut state, true, 61_000);
    assert_eq!(reason, None);
    assert_eq!(rtc_calls(), 1, "stale recovery spends one truth-check");
    assert!(
        !ops_of(&state).contains(&Op::ConfirmPublished),
        "a stale frame must never advance deduplication"
    );
    assert!(
        !ops_of(&state).contains(&Op::Publish),
        "a stale frame must never reach the canvas"
    );
}

#[test]
fn entry_fetch_costs_one_rtc_then_steps_are_free() {
    // Entry (and unavailable-retry) fetches once to anchor; the row work
    // that follows spends nothing.
    let target = fast_minute(18, 11);
    let (mut context, mut state) = rig(
        vec![
            ClockPoll::NeedsTime,
            ClockPoll::Step,
            ClockPoll::Step,
            ClockPoll::Step,
        ],
        Some(target.epoch_minute),
        SettleOutcome::Held,
        EstimatedSettle::PublishCurrent(MinuteIntent::Fast),
        Some(DirtyArea::FULL),
        true,
        StrictFastOutcome::Pushed,
    );
    assert!(
        SCRIPT_SNAPSHOT
            .get()
            .is_some_and(|s| s.valid && s.local_epoch_seconds == t(18, 10, 0)),
        "fixture snapshot must be the valid 18:10 read the runner consumes"
    );
    let reason = run_once(&mut context, &mut state, true, 1_000);
    assert_eq!(reason, None);
    assert_eq!(rtc_calls(), 1);
    assert_eq!(ops_of(&state), &[Op::Observe]);
    for step in 1..4 {
        let reason = run_once(&mut context, &mut state, true, 1_000 + step * 8);
        assert_eq!(reason, None);
    }
    assert_eq!(rtc_calls(), 1, "anchored steps must not refetch");
}

#[test]
fn failure_placeholder_and_push_rejection_cost_zero_rtc() {
    // A failed screen surfaces its placeholder with no snapshot ...
    let (mut context, mut state) = rig(
        vec![ClockPoll::Step],
        None,
        SettleOutcome::Held,
        EstimatedSettle::HoldFuture { boundary_ms: 1_000 },
        Some(DirtyArea::FULL),
        true,
        StrictFastOutcome::Pushed,
    );
    state
        .backend
        .as_mut()
        .expect("backend present")
        .failure_pending = true;
    let reason = run_once(&mut context, &mut state, true, 1_000);
    assert_eq!(reason, None);
    assert_eq!(ops_of(&state), &[Op::Service, Op::PaintFailure]);
    assert_eq!(rtc_calls(), 0);
    // ... and a rejected strict push holds the staged frame without one.
    for outcome in [StrictFastOutcome::NotReady, StrictFastOutcome::Failed] {
        let target = fast_minute(18, 11);
        let (mut context, mut state) = rig(
            vec![ClockPoll::PublishReady(target)],
            Some(target.epoch_minute),
            SettleOutcome::Held,
            EstimatedSettle::PublishCurrent(MinuteIntent::Fast),
            Some(DirtyArea::FULL),
            true,
            outcome,
        );
        let reason = run_once(&mut context, &mut state, true, 61_000);
        assert_eq!(reason, None, "outcome={outcome:?}");
        assert_eq!(rtc_calls(), 0, "outcome={outcome:?}");
        assert_eq!(ops_of(&state), &[Op::Publish, Op::StrictFast]);
    }
}

#[test]
fn idle_and_inactive_cost_zero_rtc() {
    for poll in [ClockPoll::Idle, ClockPoll::Inactive] {
        let (mut context, mut state) = rig(
            vec![poll],
            None,
            SettleOutcome::Held,
            EstimatedSettle::HoldFuture { boundary_ms: 1_000 },
            Some(DirtyArea::FULL),
            true,
            StrictFastOutcome::Pushed,
        );
        let reason = run_once(&mut context, &mut state, true, 1_000);
        assert_eq!(reason, None);
        assert_eq!(rtc_calls(), 0, "poll={poll:?} must not fetch");
        assert!(ops_of(&state).is_empty());
    }
}

#[test]
fn unavailable_resync_pauses_ready_frame_until_retry_deadline() {
    assert!(!analog_clock_model::retry_ready(29_999, 30_000));
    assert!(analog_clock_model::retry_ready(30_000, 30_000));
    assert_eq!(
        analog_clock_model::clock_tick_deadline(false, true, Some(20_000), 0, 30_000, 1_000),
        30_000
    );
    assert_eq!(
        analog_clock_model::clock_tick_deadline(true, true, Some(20_000), 0, 30_000, 1_000),
        u64::MAX
    );
}

#[test]
fn confirmed_idle_clock_starts_prefetch_before_next_wall_boundary() {
    assert_eq!(
        analog_clock_model::clock_tick_deadline(false, true, None, 60_000, 0, 1_000),
        1_008
    );
    assert_eq!(
        analog_clock_model::clock_tick_deadline(false, false, None, 0, 30_000, 1_000),
        30_000
    );
}

#[test]
fn ready_future_frame_has_stable_absolute_deadline() {
    for now in [1_000, 10_000, 29_999] {
        assert_eq!(
            analog_clock_model::clock_tick_deadline(false, true, Some(30_000), 60_000, 0, now),
            30_000
        );
    }
}
