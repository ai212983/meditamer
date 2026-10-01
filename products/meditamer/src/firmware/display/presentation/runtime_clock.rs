//! Runtime analog-clock ambient update path.
//!
//! Drives the selectable Durer-dial clock surface inside the normal display
//! cycle: monotonic poll, wall-clock fetch only when due, bounded
//! cooperative row budgets, and an atomic canvas publish with the standard
//! Fast/Clean panel semantics. Rendering progress never requests a physical
//! refresh; only a completed, freshness-checked frame does.
//!
//! RTC budget per frame is O(1), not O(rows): each valid snapshot anchors
//! monotonic-to-local time on the screen, and every row step, hold
//! decision, and ordinary-minute publish runs off that anchor estimate.
//! Snapshots happen at entry/unavailable-retry, once per Clean-boundary
//! validation (which doubles as the anchor resync, every 10 minutes in
//! steady state), and once per anomaly (a stale frame needing truth).
//! Ordinary Fast minutes publish with zero RTC traffic; a completed frame
//! whose minute already ended is discarded in favor of the current minute
//! rather than presenting the wrong time.

use super::super::panel::refresh::{refresh_panel_strict_fast, StrictFastOutcome};
use super::super::wall_clock::request_wall_clock_snapshot;
use crate::firmware::types::DisplayContext;
use crate::firmware::ui::lvgl::{
    resolve_clock_publish, select_fast_update_operation, Backend, ClockPoll, ClockPublishDecision,
    EstimatedSettle, FastUpdateOperation, MinuteIntent, SettleOutcome, TargetMinute,
};

use super::state::PresentationState;

/// Advance the analog-clock surface one display cycle. Sets `clean_reason`
/// when a completed frame takes the full-refresh path; Fast frames push a
/// strict partial immediately (retained across NotReady by the completed
/// staging frame, which simply publishes on a later cycle). Clean wins over
/// co-ready requests through the caller's existing merge.
pub(super) async fn process_runtime_clock_update(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    allow_clean: bool,
    now_ms: u64,
    clean_reason: &mut Option<&'static str>,
) {
    // One-shot activation probe: proves this path runs on the flashed
    // binary and captures the full first-poll gate vector for diagnosis.
    if let Some(backend) = state.backend.as_mut() {
        backend.analog_clock_report_activation(now_ms);
    }
    let poll = state
        .backend
        .as_mut()
        .map(|backend| backend.analog_clock_poll(now_ms))
        .unwrap_or(ClockPoll::Inactive);
    match poll {
        ClockPoll::Inactive | ClockPoll::Idle => {}
        ClockPoll::NeedsTime => {
            let snapshot = request_wall_clock_snapshot().await;
            if let Some(backend) = state.backend.as_mut() {
                backend.analog_clock_observe(snapshot, now_ms);
            }
        }
        ClockPoll::Step => {
            // Bounded cooperative row budgets only: staging rows in PSRAM,
            // never the panel, and never an RTC round trip. A frame that
            // completes here is classified on the next cycle from the
            // anchor estimate, so a full frame costs zero snapshots.
            state
                .backend
                .as_mut()
                .and_then(Backend::analog_clock_service);
            // A failed screen never reaches settle: surface the failure
            // placeholder once instead of leaving the loading fill up.
            if state
                .backend
                .as_ref()
                .is_some_and(Backend::analog_clock_failure_pending)
            {
                if let Some(backend) = state.backend.as_mut() {
                    backend.analog_clock_paint_failure(&mut context.inkplate);
                }
            }
        }
        ClockPoll::PublishReady(target) => {
            // A frame for a future minute holds with zero RTC: the anchor
            // estimate says the wall boundary has not arrived yet. The
            // absolute boundary deadline (not a sliding now-plus) wakes
            // this path again exactly when the frame becomes current.
            let future = state
                .backend
                .as_ref()
                .and_then(|backend| backend.analog_clock_estimated_current(now_ms))
                .is_some_and(|estimated| estimated < target.epoch_minute);
            if future {
                return;
            }
            if target.intent == MinuteIntent::Clean {
                // First frames, 10-minute boundaries, and jump recovery:
                // one snapshot validates the boundary AND resyncs the
                // anchor, so steady state costs one RTC per 10 minutes.
                let snapshot = request_wall_clock_snapshot().await;
                if state.backend.is_some() {
                    settle_completed(
                        context,
                        state,
                        allow_clean,
                        now_ms,
                        target,
                        snapshot,
                        clean_reason,
                    )
                    .await;
                }
                return;
            }
            // Ordinary minutes publish from the anchor estimate with no
            // RTC. Only a stale estimate (overrun past the minute) spends
            // one snapshot to recover from truth.
            let outcome = state
                .backend
                .as_mut()
                .map(|backend| backend.analog_clock_settle_estimated(target, now_ms))
                .unwrap_or(EstimatedSettle::StaleRetarget {
                    epoch_minute: target.epoch_minute,
                });
            match outcome {
                EstimatedSettle::PublishCurrent(intent) => {
                    push_published_frame(
                        context,
                        state,
                        allow_clean,
                        now_ms,
                        target,
                        intent,
                        clean_reason,
                    )
                    .await;
                }
                EstimatedSettle::HoldFuture { .. } => {}
                EstimatedSettle::StaleRetarget { .. } => {
                    let snapshot = request_wall_clock_snapshot().await;
                    if state.backend.is_some() {
                        settle_completed(
                            context,
                            state,
                            allow_clean,
                            now_ms,
                            target,
                            snapshot,
                            clean_reason,
                        )
                        .await;
                    }
                }
            }
        }
    }
}

/// Freshness-check one completed frame and, when current, publish it with
/// the frame's own refresh class. A stale frame is discarded (the settle
/// already retargeted to the fresh minute); an unavailable clock holds the
/// completed staging frame for a later cycle without fabricating time.
async fn settle_completed(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    allow_clean: bool,
    now_ms: u64,
    completed: TargetMinute,
    snapshot: Option<rtc::driver::WallClockSnapshot>,
    clean_reason: &mut Option<&'static str>,
) {
    let outcome = state
        .backend
        .as_mut()
        .map(|backend| backend.analog_clock_settle(snapshot, completed, now_ms))
        .unwrap_or(SettleOutcome::Held);
    let intent = match outcome {
        SettleOutcome::Held => return,
        SettleOutcome::Published(intent) => intent,
    };
    push_published_frame(
        context,
        state,
        allow_clean,
        now_ms,
        completed,
        intent,
        clean_reason,
    )
    .await;
}

/// Push a settled frame to the panel. Fast minutes take the strict partial
/// path; Clean minutes (first frame, 10-minute boundaries, skips, jumps)
/// join the caller's full-refresh merge. Both publish the settled staging
/// bits into the retained canvas first; minute deduplication advances only
/// when the panel actually covers the frame: a failed canvas copy or a
/// Fast push the scheduler rejects stays staged with its target intact and
/// retries on later cycles until a strict partial push -- or a Clean
/// recovery repaint -- covers it. No overlay pending-flag machinery is
/// involved.
async fn push_published_frame(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    allow_clean: bool,
    now_ms: u64,
    completed: TargetMinute,
    intent: MinuteIntent,
    clean_reason: &mut Option<&'static str>,
) {
    let clean_pending = clean_reason.is_some()
        || state.backend.as_mut().is_some_and(|backend| {
            backend.pending_screen_update_intent() == Some(shell::types::ScreenUpdateIntent::Clean)
        });
    // A Clean-classified frame always goes full once Clean is allowed; a
    // Fast frame goes full too when another Clean request is already
    // pending (Clean wins), and waits otherwise. The merged full repaint
    // covers whatever is in the retained canvas, so the settled staging
    // bits must land there first: publish, then confirm only when the
    // copy succeeded. A failed canvas copy holds the staged frame for
    // retry instead of confirming a frame the panel never covered.
    if intent == MinuteIntent::Clean || clean_pending {
        let landed = if allow_clean {
            state
                .backend
                .as_mut()
                .and_then(|backend| backend.analog_clock_publish(&mut context.inkplate))
                .is_some()
        } else {
            false
        };
        if resolve_clock_publish(intent, clean_pending, allow_clean, landed)
            == ClockPublishDecision::RequestCleanAndConfirm
        {
            *clean_reason = Some("analog_clock");
            // The merged full repaint covers the published canvas this
            // cycle, so the minute is honestly complete.
            if let Some(backend) = state.backend.as_mut() {
                backend.analog_clock_confirm_published(completed, now_ms);
            }
        }
        return;
    }
    let operation = select_fast_update_operation(
        false,
        allow_clean,
        context.inkplate.is_partial_refresh_ready(),
    );
    if operation != FastUpdateOperation::StrictFast {
        // NotReady (or Clean covered above): the completed frame stays
        // staged and this same publish is retried on a later cycle until a
        // strict partial push -- or a Clean recovery repaint -- covers it.
        return;
    }
    let damage = state
        .backend
        .as_mut()
        .and_then(|backend| backend.analog_clock_publish(&mut context.inkplate));
    if let Some(dirty) = damage {
        match refresh_panel_strict_fast(context, state, dirty, "analog_clock").await {
            StrictFastOutcome::Pushed => {
                if let Some(backend) = state.backend.as_mut() {
                    backend.analog_clock_confirm_published(completed, now_ms);
                }
            }
            StrictFastOutcome::NotReady | StrictFastOutcome::Failed => {
                // An intended refresh is not a successful panel: keep the
                // completed frame staged for retry.
            }
        }
    }
}
