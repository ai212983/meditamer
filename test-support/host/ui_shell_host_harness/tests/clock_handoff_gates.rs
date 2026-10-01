//! Regression gates for the clock loading/navigation repair.
//!
//! Drives the REAL unmodified product policy modules (borrowed via
//! `#[path]` through the harness crate, not mirrors):
//! - `analog_clock_model::resolve_clock_deadline`: the deadline the fixed
//!   `AnalogClockScreen::next_deadline_ms` delegates to.
//! - `input_transition_readiness`: the readiness state machine plus the
//!   `needs_transition_boundary` / `next_transition_rendered` predicates
//!   the fixed `Backend::render_with` calls.
//!
//! Each test fails on the pre-fix behavior:
//! - `min` deadline stuck at the `0` retry sentinel (always-due timer);
//! - unconditional per-cycle boundary invalidation (full-canvas re-render
//!   every cycle while content loads);
//! - `rendered = refreshed` clearing an already-rendered boundary on a
//!   noop pass (re-arming the invalidation loop);
//! - a landed staging copy never advancing the handoff guard (input never
//!   admitted on the clock surface).

use ui_shell_host_harness::analog_clock_model::resolve_clock_deadline;
use ui_shell_host_harness::input_transition_readiness::{
    needs_transition_boundary, next_transition_rendered, InputTransitionReadiness,
};

// --- Deadline: max (not min) of tracker/retry dues --------------------------

#[test]
fn deadline_first_entry_is_immediate() {
    assert_eq!(resolve_clock_deadline(false, false, 0, 0, 0), 0);
}

#[test]
fn deadline_valid_rtc_ignores_zero_retry_sentinel() {
    // Tracker waits on the next wall-minute rollover; retry is the idle
    // sentinel. Pre-fix `min` returned 0 here: an always-due timer.
    assert_eq!(
        resolve_clock_deadline(false, false, 62_000, 0, 1_000),
        62_000
    );
}

#[test]
fn deadline_unavailable_read_waits_for_retry() {
    assert_eq!(
        resolve_clock_deadline(false, false, 1_000, 31_000, 1_000),
        31_000
    );
}

#[test]
fn deadline_target_means_near_future_cooperative_work() {
    // Rendering / asset-await / publish-ready all carry a target; the
    // executor must yield (future deadline) yet wake soon (row budget).
    assert_eq!(resolve_clock_deadline(false, true, 0, 0, 1_000), 1_008);
    assert_eq!(resolve_clock_deadline(false, true, 62_000, 0, 1_000), 1_008);
}

#[test]
fn deadline_failed_engine_parks() {
    assert_eq!(resolve_clock_deadline(true, false, 0, 0, 1_000), u64::MAX);
    assert_eq!(resolve_clock_deadline(true, true, 0, 0, 1_000), u64::MAX);
}

// --- Boundary gate: force only while pending AND unrendered -----------------

#[test]
fn boundary_forced_once_then_released() {
    assert!(needs_transition_boundary(true, false));
    // Already rendered: must NOT force another full-canvas invalidation.
    // The pre-fix unconditional gate returns true here (re-render loop).
    assert!(!needs_transition_boundary(true, true));
    assert!(!needs_transition_boundary(false, false));
    assert!(!needs_transition_boundary(false, true));
}

#[test]
fn noop_refresh_keeps_already_rendered_boundary() {
    // Pre-fix `rendered = refreshed` cleared true on a noop pass.
    assert!(next_transition_rendered(true, true, false));
    assert!(!next_transition_rendered(true, false, false));
    assert!(next_transition_rendered(true, false, true));
    assert!(next_transition_rendered(true, true, true));
    assert!(!next_transition_rendered(false, false, true));
}

// --- Readiness chain: content advances, Clean still required ----------------

#[test]
fn ambient_navigation_waits_for_content_then_clean() {
    let awaiting = InputTransitionReadiness::after_navigation(true, true);
    assert_eq!(awaiting, InputTransitionReadiness::AwaitAmbientContent);
    // Loading surface: nothing presented yet, input stays closed.
    assert!(!awaiting.allows_presentation(false));
    assert!(!awaiting.allows_presentation(true));
    // A landed staging copy advances exactly one step: Clean still needed.
    let clean_gated = awaiting.after_ambient_content(true);
    assert_eq!(clean_gated, InputTransitionReadiness::AwaitClean);
    assert!(!clean_gated.allows_presentation(false));
    assert!(clean_gated.allows_presentation(true));
}

#[test]
fn failed_copy_or_non_ambient_never_enables_input_early() {
    // Non-ambient destinations never enter the ambient wait.
    assert_eq!(
        InputTransitionReadiness::after_navigation(true, false),
        InputTransitionReadiness::Ready
    );
    assert_eq!(
        InputTransitionReadiness::after_navigation(false, true),
        InputTransitionReadiness::Ready
    );
    // A failed copy leaves the guard at content-wait: no presentation.
    let awaiting = InputTransitionReadiness::AwaitAmbientContent;
    assert!(!awaiting.allows_presentation(false));
    assert!(!awaiting.allows_presentation(true));
    // Content on a non-ambient surface is a no-op, never a bypass.
    assert_eq!(
        InputTransitionReadiness::Ready.after_ambient_content(true),
        InputTransitionReadiness::Ready
    );
}

#[test]
fn boundary_launcher_transition_keeps_input_closed_until_clean() {
    // A ready Boundary landing adds a Clean gate: no early admission on
    // Fast/partial work, admission only on the successful full refresh that
    // presents the final Launcher frame.
    let gated = InputTransitionReadiness::Ready.require_clean();
    assert_eq!(gated, InputTransitionReadiness::AwaitClean);
    assert!(!gated.allows_presentation(false));
    assert!(gated.allows_presentation(true));

    // Boundary policy must not bypass an ambient destination's prerequisite
    // content frame; that path adds its Clean gate only after content lands.
    assert_eq!(
        InputTransitionReadiness::AwaitAmbientContent.require_clean(),
        InputTransitionReadiness::AwaitAmbientContent
    );
}
