//! Production presentation regression for the repaired analog-clock
//! first-frame Clean bug.
//!
//! The seven `analog_clock_publish` tests only exercise the extracted
//! `resolve_clock_publish` policy table. They cannot catch the original
//! defect: the `Clean` branch of `push_published_frame` in
//! `products/meditamer/src/firmware/display/presentation/runtime_clock.rs`
//! set `clean_reason` and called `analog_clock_confirm_published` WITHOUT
//! calling `analog_clock_publish` first. The first frame is always Clean, so
//! the merged full repaint only ever covered the entry-time loading canvas
//! while `mark_published` discarded the finished staging -- a permanently
//! blank clock.
//!
//! This suite imports that REAL, unmodified production module via `#[path]`
//! and executes its real `process_runtime_clock_update` (through a
//! visibility wrapper; the runner itself is `pub(super)` in production).
//! Everything around it is a thin fake with no renderer duplication:
//!
//! - `rtc::driver::WallClockSnapshot`: minimal stand-in (this test crate
//!   aliased as `rtc`, carrying only the fields production touches).
//! - `firmware::types::{DisplayContext, FakeInkplate}`: carries only the
//!   `inkplate` handle and `is_partial_refresh_ready` production reads.
//! - `firmware::ui::lvgl::Backend`: scripted answers for every
//!   `analog_clock_*` / `pending_screen_update_intent` call production
//!   makes, recording an `Op` trace (`Publish` / `ConfirmPublished` /
//!   `StrictFast` / `PaintFailure`). No rendering, no LVGL.
//! - `ClockPoll` / `TargetMinute` / `SettleOutcome` / `DirtyArea`: thin
//!   types with the same API shape production uses (opaque pass-through;
//!   production never inspects their fields here).
//! - Genuine product logic reused, not mirrored: `resolve_clock_publish`,
//!   `MinuteIntent`, `ClockPublishDecision` come from the real clock model
//!   via `ui_shell_host_harness::analog_clock_model`, and
//!   `select_fast_update_operation` / `FastUpdateOperation` are the real
//!   `screen_update_state.rs` pulled in via `#[path]`.
//! - `shell::types::ScreenUpdateIntent` is the real shell crate.
//!
//! The regression assertion is the exact `Op` trace: with the pre-fix code
//! the first-Clean run records `[ConfirmPublished]` (confirm without
//! publish) and this suite's `assert_eq!(ops, [Publish, ConfirmPublished])`
//! fails. (Verified by reasoning against the fixed source's publish-first
//! branch; the production file is never mutated -- sibling workers own it.)

#![allow(dead_code)]

extern crate self as rtc;

use std::cell::Cell;
use std::future::Future;
use std::task::{Context, Poll};

// ---------------------------------------------------------------------------
// `rtc` stand-in: `rtc::driver::WallClockSnapshot` as production uses it.
// ---------------------------------------------------------------------------

pub mod driver {
    /// Minimal wall-clock snapshot: only the fields the presentation path
    /// and the clock screen touch (`valid` filter, `local_epoch_seconds`).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct WallClockSnapshot {
        pub valid: bool,
        pub local_epoch_seconds: u32,
    }
}

std::thread_local! {
    static SCRIPT_SNAPSHOT: Cell<Option<driver::WallClockSnapshot>> =
        const { Cell::new(None) };
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

fn scripted_snapshot() -> Option<driver::WallClockSnapshot> {
    SCRIPT_SNAPSHOT
        .get()
        .or(Some(valid_snapshot(9 * 3_600 + 60)))
}

// ---------------------------------------------------------------------------
// Fake `firmware` hierarchy matching the production module paths used by
// `runtime_clock.rs`:
// - `super::super::panel::refresh`  -> `firmware::display::panel::refresh`
// - `super::super::wall_clock`      -> `firmware::display::wall_clock`
// - `super::state::PresentationState`
//   -> `firmware::display::presentation::state::PresentationState`
// - `crate::firmware::types::DisplayContext`
// - `crate::firmware::ui::lvgl::{...}`
// ---------------------------------------------------------------------------

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
            // Real clock-model policy: the exact decision table the
            // production branch consumes.
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
            /// the strict-fast push.
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub struct DirtyArea {
                pub x0: u16,
                pub y0: u16,
                pub x1: u16,
                pub y1: u16,
            }

            impl DirtyArea {
                pub const FULL: Self = Self {
                    x0: 0,
                    y0: 0,
                    x1: 599,
                    y1: 599,
                };
            }

            /// Thin target-minute handle with the production field shape.
            /// Production passes it through opaquely here (settle/confirm),
            /// so no clock math is duplicated.
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

            /// Observable backend operations, in call order. The regression
            /// assertions below match this trace exactly.
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub enum Op {
                Publish,
                ConfirmPublished,
                StrictFast,
                PaintFailure,
            }

            /// Scripted backend: answers every `analog_clock_*` call the
            /// production runner makes and records the operation trace. No
            /// renderer, no canvas, no LVGL.
            pub struct Backend {
                pub poll: ClockPoll,
                pub settle: SettleOutcome,
                pub damage: Option<DirtyArea>,
                pub pending_clean: bool,
                pub failure_pending: bool,
                pub refresh_outcome: crate::firmware::display::panel::refresh::StrictFastOutcome,
                pub ops: Vec<Op>,
            }

            impl Backend {
                pub fn analog_clock_report_activation(&mut self, _now_ms: u64) {}

                pub fn analog_clock_poll(&mut self, _now_ms: u64) -> ClockPoll {
                    self.poll
                }

                pub fn analog_clock_estimated_current(&self, _now_ms: u64) -> Option<u64> {
                    // Legacy regression rigs predate the anchor: no estimate
                    // means "not provably future", so the runner proceeds to
                    // the scripted settle exactly as before.
                    None
                }

                pub fn analog_clock_observe(
                    &mut self,
                    _snapshot: Option<rtc::driver::WallClockSnapshot>,
                    _now_ms: u64,
                ) {
                }

                pub fn analog_clock_service(&mut self) -> Option<TargetMinute> {
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
                    match self.settle {
                        SettleOutcome::Published(intent) => EstimatedSettle::PublishCurrent(intent),
                        SettleOutcome::Held => EstimatedSettle::HoldFuture {
                            boundary_ms: u64::MAX,
                        },
                    }
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
            /// Scripted wall-clock fetch: returns the per-test snapshot.
            pub async fn request_wall_clock_snapshot() -> Option<rtc::driver::WallClockSnapshot> {
                crate::scripted_snapshot()
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
    Backend, ClockPoll, DirtyArea, MinuteIntent, Op, SettleOutcome, TargetMinute,
};
use ui_shell_host_harness::analog_clock_model::{MinuteTracker, TimeObservation};

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

fn first_frame_clean_target() -> TargetMinute {
    // The first observed wall minute is always Clean; derive the intent
    // from the REAL minute policy rather than hard-coding it.
    let mut tracker = MinuteTracker::new();
    let (epoch_minute, intent) = match tracker.observe(9 * 3_600 + 60, 1_000) {
        TimeObservation::Target {
            epoch_minute,
            intent,
        } => (epoch_minute, intent),
        TimeObservation::Duplicate => panic!("first observation must target"),
    };
    assert_eq!(intent, MinuteIntent::Clean);
    TargetMinute::new(epoch_minute, 9, 1, intent)
}

fn fast_target() -> TargetMinute {
    TargetMinute::new(561, 9, 21, MinuteIntent::Fast)
}

fn rig(
    target: TargetMinute,
    damage: Option<DirtyArea>,
    pending_clean: bool,
    partial_ready: bool,
    refresh_outcome: StrictFastOutcome,
) -> (DisplayContext, PresentationState) {
    set_snapshot(Some(valid_snapshot(9 * 3_600 + 60)));
    let context = DisplayContext {
        inkplate: FakeInkplate { partial_ready },
    };
    let state = PresentationState {
        backend: Some(Backend {
            poll: ClockPoll::PublishReady(target),
            settle: SettleOutcome::Published(target.intent),
            damage,
            pending_clean,
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

// ---------------------------------------------------------------------------
// Regression suite: the real runner's publish-before-confirm contract.
// ---------------------------------------------------------------------------

#[test]
fn first_clean_frame_publishes_before_confirm() {
    // THE regression: the pre-fix Clean branch confirmed the minute without
    // publishing, recording `[ConfirmPublished]`. The fixed production code
    // must record publish strictly before confirm and emit the Clean merge
    // reason for the first frame.
    let target = first_frame_clean_target();
    let (mut context, mut state) = rig(
        target,
        Some(DirtyArea::FULL),
        false,
        true,
        StrictFastOutcome::Pushed,
    );
    let mut clean_reason: Option<&'static str> = None;
    block_on(run_clock_update(
        &mut context,
        &mut state,
        true,
        1_000,
        &mut clean_reason,
    ));
    assert_eq!(clean_reason, Some("analog_clock"));
    assert_eq!(ops_of(&state), &[Op::Publish, Op::ConfirmPublished]);
}

#[test]
fn gated_clean_holds_without_publish_or_confirm() {
    // Clean not allowed this cycle: the staged frame is held for retry --
    // no canvas copy is even attempted, no minute advance, no merge reason.
    let target = first_frame_clean_target();
    let (mut context, mut state) = rig(
        target,
        Some(DirtyArea::FULL),
        false,
        true,
        StrictFastOutcome::Pushed,
    );
    let mut clean_reason: Option<&'static str> = None;
    block_on(run_clock_update(
        &mut context,
        &mut state,
        false,
        1_000,
        &mut clean_reason,
    ));
    assert_eq!(clean_reason, None);
    assert!(ops_of(&state).is_empty());
}

#[test]
fn clean_publish_failure_holds_without_confirm() {
    // The staging-to-canvas copy did not land (`None` damage): the frame
    // stays staged for retry instead of confirming a canvas the panel never
    // covered. Publish is attempted exactly once, confirm never runs.
    let target = first_frame_clean_target();
    let (mut context, mut state) = rig(target, None, false, true, StrictFastOutcome::Pushed);
    let mut clean_reason: Option<&'static str> = None;
    block_on(run_clock_update(
        &mut context,
        &mut state,
        true,
        1_000,
        &mut clean_reason,
    ));
    assert_eq!(clean_reason, None);
    assert_eq!(ops_of(&state), &[Op::Publish]);
}

#[test]
fn fast_frame_confirms_after_strict_push() {
    // Fast path characterization: publish, strict-partial push, then
    // confirm -- with no Clean merge reason.
    let target = fast_target();
    let (mut context, mut state) = rig(
        target,
        Some(DirtyArea::FULL),
        false,
        true,
        StrictFastOutcome::Pushed,
    );
    let mut clean_reason: Option<&'static str> = None;
    block_on(run_clock_update(
        &mut context,
        &mut state,
        true,
        61_000,
        &mut clean_reason,
    ));
    assert_eq!(clean_reason, None);
    assert_eq!(
        ops_of(&state),
        &[Op::Publish, Op::StrictFast, Op::ConfirmPublished]
    );
}

#[test]
fn fast_strict_push_rejection_holds_without_confirm() {
    // An intended strict push the panel rejects (`NotReady`) or that fails
    // (`Failed`) is not a successful panel: the completed frame stays
    // staged for retry, never confirmed.
    for outcome in [StrictFastOutcome::NotReady, StrictFastOutcome::Failed] {
        let target = fast_target();
        let (mut context, mut state) = rig(target, Some(DirtyArea::FULL), false, true, outcome);
        let mut clean_reason: Option<&'static str> = None;
        block_on(run_clock_update(
            &mut context,
            &mut state,
            true,
            61_000,
            &mut clean_reason,
        ));
        assert_eq!(clean_reason, None, "outcome={outcome:?}");
        assert_eq!(
            ops_of(&state),
            &[Op::Publish, Op::StrictFast],
            "outcome={outcome:?}"
        );
    }
}

#[test]
fn fast_with_copending_clean_takes_full_merge() {
    // Clean wins over a co-ready Fast frame: the canvas is published, then
    // the merged full repaint is requested and the minute confirmed.
    let target = fast_target();
    let (mut context, mut state) = rig(
        target,
        Some(DirtyArea::FULL),
        true,
        true,
        StrictFastOutcome::Pushed,
    );
    let mut clean_reason: Option<&'static str> = None;
    block_on(run_clock_update(
        &mut context,
        &mut state,
        true,
        61_000,
        &mut clean_reason,
    ));
    assert_eq!(clean_reason, Some("analog_clock"));
    assert_eq!(ops_of(&state), &[Op::Publish, Op::ConfirmPublished]);
}
