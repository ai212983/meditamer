//! Ordered physical input consumption and its direct presentation work.
use super::super::panel::refresh::{refresh_panel, RefreshRequest};
use super::{process_gestures, PresentationState};
use crate::firmware::{
    touch::{
        config::{
            TOUCH_CONTROLLER_ACTIVE_SLOTS, TOUCH_LVGL_MULTITOUCH_FRAMES,
            TOUCH_LVGL_MULTITOUCH_RESET, TOUCH_PIPELINE_EVENTS,
        },
        lvgl_multitouch::LvglMultitouchFrame,
        merge::{batch_candidate_action, next_touch_source, BatchCandidateAction, NextTouchSource},
        types::{TouchEvent, TouchEventKind},
    },
    types::DisplayContext,
};
use core::sync::atomic::Ordering;
use embassy_time::Instant;
use shell::types::ScreenUpdateIntent;

/// One LVGL frame carries at most this many retained single-touch events.
/// Stack-resident (`heapless::Vec`); a fuller queue stays buffered for later
/// frames. Three retained entries cover the core Down, latest Move, Up drag
/// batch after consecutive Move collapse.
const TOUCH_BATCH_CAPACITY: usize = 3;

/// Drain `TOUCH_PIPELINE_EVENTS` and `TOUCH_LVGL_MULTITOUCH_FRAMES` as one
/// combined, timestamp-ordered stream instead of exhausting one queue before
/// touching the other (see `touch::merge`). A pending multitouch-reset
/// discards any buffered multitouch lookahead and runs before either queue
/// is consulted again, since it invalidates whatever was queued before it.
pub(super) async fn drain_touch_streams_in_order(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    allow_full_repaint: bool,
) {
    let mut next_single: Option<TouchEvent> = None;
    let mut next_multi: Option<LvglMultitouchFrame> = None;
    let mut frontlight_refresh_deferred = false;
    let started_us = Instant::now().as_micros();
    let mut events = 0u32;
    let mut max_event_us = 0u64;
    loop {
        if TOUCH_LVGL_MULTITOUCH_RESET.swap(false, Ordering::AcqRel) {
            let discarded = discard_multitouch_lookahead(&mut next_multi);
            process_multitouch_reset(context, state, discarded).await;
        }
        if next_single.is_none() {
            next_single = TOUCH_PIPELINE_EVENTS.try_receive().ok();
        }
        if next_multi.is_none() {
            next_multi = TOUCH_LVGL_MULTITOUCH_FRAMES.try_receive().ok();
        }
        match next_touch_source(
            next_single.map(|event| event.time_ms()),
            next_multi.map(|frame| frame.t_ms),
        ) {
            None => break,
            Some(NextTouchSource::Single) => {
                let first = next_single.take().expect("Single source has an event");
                // Collect and synchronously render the batch in an inner
                // block so the stack-resident `heapless::Vec` (and any borrow
                // of it) is dropped before the first await below. Only the
                // small `Copy` tail crosses into the async completion, so
                // the batch never enters the display future/task pool.
                let (retained, event_started_us, tail) = {
                    let Some(batch) = collect_single_touch_batch(
                        first,
                        &mut next_single,
                        &mut next_multi,
                        state.touch_equivalence.requires_single_event_batches(),
                    ) else {
                        // Admission rejected the first event; drop it and keep
                        // draining, mirroring the old per-event early return.
                        continue;
                    };
                    let retained = batch.len() as u32;
                    let event_started_us = Instant::now().as_micros();
                    let tail =
                        render_single_touch_batch(context, state, &batch, allow_full_repaint);
                    (retained, event_started_us, tail)
                };
                let preview_pending = tail.is_some()
                    && state
                        .backend
                        .as_ref()
                        .is_some_and(|backend| backend.frontlight_preview_pending());
                if frontlight_refresh_deferred || preview_pending {
                    frontlight_refresh_deferred = true;
                    finish_single_touch_batch_without_refresh(context, state).await;
                } else {
                    finish_single_touch_batch(context, state, tail).await;
                }
                events += retained;
                max_event_us =
                    max_event_us.max(Instant::now().as_micros().saturating_sub(event_started_us));
                if state.refresh_tracking.recovery_required() {
                    break;
                }
            }
            Some(NextTouchSource::Multitouch) => {
                let frame = next_multi.take().expect("Multitouch source has a frame");
                let event_started_us = Instant::now().as_micros();
                process_multitouch_frame(context, state, frame).await;
                events += 1;
                max_event_us =
                    max_event_us.max(Instant::now().as_micros().saturating_sub(event_started_us));
            }
        }
    }
    let elapsed_us = Instant::now().as_micros().saturating_sub(started_us);
    if elapsed_us >= 500_000 {
        console::println!(
            "LVGL_INPUT_BATCH events={} elapsed_us={} max_event_us={}",
            events,
            elapsed_us,
            max_event_us
        );
    }
}

/// Build one timestamp-ordered single-touch batch starting from the already
/// selected `first` event. Only ready (already queued) events join: a
/// candidate at or past the multitouch lookahead frame defers back into
/// `next_single` so the normal timestamp selector dispatches the frame first.
/// Every `Down`/`Up`/`Cancel` and non-`Move` edge is retained; only a `Move`
/// superseded by the batch's trailing `Move` collapses (newest wins), so the
/// retained move still renders once below. The batch ends immediately after a
/// terminal `Up`/`Cancel`, so a later gesture never combines past it, and a
/// candidate beyond capacity stays buffered for the next frame. Admission is
/// checked per event: rejected candidates are dropped (with the same trace
/// record as the old per-event path) without joining or ending the batch.
/// Returns `None` when admission rejects `first` itself.
fn collect_single_touch_batch(
    first: TouchEvent,
    next_single: &mut Option<TouchEvent>,
    next_multi: &mut Option<LvglMultitouchFrame>,
    single_event_batches: bool,
) -> Option<heapless::Vec<TouchEvent, TOUCH_BATCH_CAPACITY>> {
    if !crate::firmware::touch::admission::accepts(first.admission_epoch) {
        #[cfg(feature = "ui-interaction-trace")]
        crate::firmware::interaction_trace::record(
            first.trace_id,
            38,
            first.time_ms(),
            first.admission_epoch,
            0,
        );
        return None;
    }
    let mut batch: heapless::Vec<TouchEvent, TOUCH_BATCH_CAPACITY> = heapless::Vec::new();
    // Admission already checked above; capacity exceeds one.
    let _ = batch.push(first);
    if single_event_batches || matches!(first.kind, TouchEventKind::Up | TouchEventKind::Cancel) {
        return Some(batch);
    }
    loop {
        if next_multi.is_none() {
            *next_multi = TOUCH_LVGL_MULTITOUCH_FRAMES.try_receive().ok();
        }
        let Some(candidate) = TOUCH_PIPELINE_EVENTS.try_receive().ok() else {
            break;
        };
        if !crate::firmware::touch::admission::accepts(candidate.admission_epoch) {
            #[cfg(feature = "ui-interaction-trace")]
            crate::firmware::interaction_trace::record(
                candidate.trace_id,
                38,
                candidate.time_ms(),
                candidate.admission_epoch,
                0,
            );
            continue;
        }
        let last = batch.last().expect("batch holds the first event");
        match batch_candidate_action(
            candidate.kind == TouchEventKind::Move,
            last.kind == TouchEventKind::Move,
            last.admission_epoch,
            candidate.admission_epoch,
            candidate.time_ms(),
            next_multi.map(|frame| frame.t_ms),
        ) {
            BatchCandidateAction::CollapseIntoLastMove => {
                if let Some(slot) = batch.last_mut() {
                    *slot = candidate;
                }
            }
            BatchCandidateAction::Defer => {
                *next_single = Some(candidate);
                break;
            }
            BatchCandidateAction::Retain => {
                if batch.len() >= TOUCH_BATCH_CAPACITY {
                    *next_single = Some(candidate);
                    break;
                }
                let _ = batch.push(candidate);
                if matches!(candidate.kind, TouchEventKind::Up | TouchEventKind::Cancel) {
                    break;
                }
            }
        }
    }
    Some(batch)
}

/// Compact refresh tail carried across `refresh_panel` awaits. Stores only
/// the input time and a one-byte phase; the static phase label is resolved
/// at `RefreshRequest::from_touch` so no `&'static str` fat pointer crosses
/// the await. Trace identity/kind ride along only when
/// `ui-interaction-trace` needs them for records 9/10.
#[derive(Clone, Copy)]
struct TouchRefreshTail {
    input_ms: u64,
    phase: TouchRefreshPhase,
    #[cfg(feature = "ui-interaction-trace")]
    trace_id: u32,
    #[cfg(feature = "ui-interaction-trace")]
    kind: TouchEventKind,
}

/// Minimal phase set that can leave a batch behind with a panel refresh.
#[derive(Clone, Copy)]
enum TouchRefreshPhase {
    Pressed,
    Released,
    Cancelled,
    Drag,
}

impl TouchRefreshPhase {
    const fn label(self) -> &'static str {
        match self {
            Self::Pressed => "pressed",
            Self::Released => "released",
            Self::Cancelled => "cancelled",
            Self::Drag => "drag",
        }
    }
}

/// Synchronous half of one retained batch: everything that borrows the batch.
/// Only the small `Copy` [`TouchRefreshTail`] below outlives this function;
/// the caller must drop the batch before awaiting the async completion so
/// the batch never enters the display future/task pool. Render one retained
/// (guaranteed nonempty) batch through a single LVGL frame. Admission was
/// already checked per event during collection. Per-event latency logs, trace
/// records, and `touch_equivalence` observations still run once per retained
/// event, in order; only the frame publish, navigation drain, dirty-area
/// scan, and panel refresh collapse into one pass. The single refresh (if
/// any, performed later by [`finish_single_touch_batch`]) takes the last
/// phased event's label and input time, since that is the gesture state the
/// batch leaves behind. `handle_touch` itself is untouched: the
/// synthetic-equivalence path in `presentation.rs` keeps its own per-event
/// render. A clean request or a batch with no phased tail maps to `None`:
/// both skip straight to gestures.
fn render_single_touch_batch(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    batch: &[TouchEvent],
    allow_full_repaint: bool,
) -> Option<TouchRefreshTail> {
    for event in batch {
        #[cfg(feature = "ui-interaction-trace")]
        crate::firmware::interaction_trace::record(
            event.trace_id,
            7,
            event.time_ms(),
            crate::firmware::interaction_trace::kind(event.kind),
            (u32::from(event.x) << 16) | u32::from(event.y),
        );
        if matches!(
            event.kind,
            TouchEventKind::Down | TouchEventKind::Up | TouchEventKind::Cancel
        ) {
            let dequeue_latency_ms = Instant::now().as_millis().saturating_sub(event.time_ms());
            console::println!(
                "LVGL_TOUCH phase={:?} x={} y={} queue_ms={} active_slots={}",
                event.kind,
                event.x,
                event.y,
                dequeue_latency_ms,
                TOUCH_CONTROLLER_ACTIVE_SLOTS.load(Ordering::Acquire),
            );
        }
    }
    let rendered = {
        // Per-event scopes live inside `handle_touch_batch`; the aggregate
        // render here must not be attributed to `batch[0]`. The scope must
        // not survive panel or gesture awaits.
        state
            .backend
            .as_mut()
            .and_then(|backend| backend.handle_touch_batch(&mut context.inkplate, batch))
    };
    for event in batch {
        #[cfg(feature = "ui-interaction-trace")]
        crate::firmware::interaction_trace::record(
            event.trace_id,
            8,
            event.time_ms(),
            u32::from(rendered.is_some()),
            0,
        );
        state.touch_equivalence.observe_prime_event(
            *event,
            rendered.is_some(),
            Instant::now().as_millis(),
        );
    }
    let refresh_tail = batch.iter().rev().find_map(|event| {
        let phase = match event.kind {
            TouchEventKind::Down => TouchRefreshPhase::Pressed,
            TouchEventKind::Up => TouchRefreshPhase::Released,
            TouchEventKind::Cancel => TouchRefreshPhase::Cancelled,
            TouchEventKind::Move => TouchRefreshPhase::Drag,
            TouchEventKind::Tap | TouchEventKind::LongPress | TouchEventKind::Swipe(_) => {
                return None;
            }
        };
        Some(TouchRefreshTail {
            input_ms: event.time_ms(),
            phase,
            #[cfg(feature = "ui-interaction-trace")]
            trace_id: event.trace_id,
            #[cfg(feature = "ui-interaction-trace")]
            kind: event.kind,
        })
    });
    if let Some(dirty) = rendered {
        let snapshot = context.inkplate.binary_framebuffer_debug_snapshot();
        for event in batch {
            state
                .touch_equivalence
                .observe_physical(*event, snapshot, dirty);
        }
        state.record_dirty(dirty);
    }
    let clean_requested = state.backend.as_mut().is_some_and(|backend| {
        backend.pending_screen_update_intent() == Some(ScreenUpdateIntent::Clean)
    });
    if clean_requested && !allow_full_repaint {
        console::println!(
            "LVGL_REPAINT full_repaint=true reason=sticky_button status=deferred clean_admitted=false"
        );
    }
    if clean_requested {
        return None;
    }
    refresh_tail
}

/// Async completion of one rendered batch: at most one panel refresh, then
/// the recovery check and one gesture run, preserving queue order. Carries
/// only the `Copy` tail across awaits; neither the batch `Vec` nor any
/// borrow of it reaches this future.
async fn finish_single_touch_batch(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    tail: Option<TouchRefreshTail>,
) {
    refresh_touch_tail(context, state, tail).await;
    if state.refresh_tracking.recovery_required() {
        return;
    }
    process_gestures(context, state).await;
}

async fn finish_single_touch_batch_without_refresh(
    context: &mut DisplayContext,
    state: &mut PresentationState,
) {
    if state.refresh_tracking.recovery_required() {
        return;
    }
    process_gestures(context, state).await;
}

pub(in crate::firmware::display) async fn finish_frontlight_effect_refresh(
    context: &mut DisplayContext,
    state: &mut PresentationState,
) {
    if state.refresh_tracking.recovery_required() {
        return;
    }
    // A later event in the drained stream may have dismissed the modal and
    // requested a Clean. Preserve that stronger transaction; the next cycle
    // will present it after the physical frontlight effect has been applied.
    if state.backend.as_mut().is_some_and(|backend| {
        backend.pending_screen_update_intent() == Some(ScreenUpdateIntent::Clean)
    }) {
        return;
    }
    if let Some(dirty) = state.take_dirty() {
        let _ = refresh_panel(context, state, RefreshRequest::from_service(dirty)).await;
    }
}

async fn refresh_touch_tail(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    tail: Option<TouchRefreshTail>,
) {
    let Some(tail) = tail else {
        return;
    };
    let Some(dirty) = state.take_dirty() else {
        return;
    };
    #[cfg(feature = "ui-interaction-trace")]
    crate::firmware::interaction_trace::record(
        tail.trace_id,
        9,
        tail.input_ms,
        crate::firmware::interaction_trace::kind(tail.kind),
        0,
    );
    let refreshed = refresh_panel(
        context,
        state,
        RefreshRequest::from_touch(dirty, tail.input_ms, tail.phase.label()),
    )
    .await;
    #[cfg(feature = "ui-interaction-trace")]
    crate::firmware::interaction_trace::record(
        tail.trace_id,
        10,
        tail.input_ms,
        u32::from(refreshed),
        0,
    );
    let _ = refreshed;
}

async fn process_multitouch_frame(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    frame: LvglMultitouchFrame,
) {
    if !crate::firmware::touch::admission::accepts(frame.admission_epoch) {
        #[cfg(feature = "ui-interaction-trace")]
        crate::firmware::interaction_trace::record(
            frame.trace_id,
            38,
            frame.t_ms,
            frame.admission_epoch,
            1,
        );
        return;
    }
    #[cfg(feature = "ui-interaction-trace")]
    crate::firmware::interaction_trace::record(
        frame.trace_id,
        30,
        frame.t_ms,
        frame.trace_frame_id,
        u32::from(frame.active_mask),
    );
    let rendered = {
        // The frame and the LVGL callbacks it synchronously causes share one
        // origin. Drop the scope before awaiting unrelated gesture handling.
        #[cfg(feature = "ui-interaction-trace")]
        let _trace_scope = crate::firmware::interaction_trace::UiScope::enter(frame.trace_id);
        state
            .backend
            .as_mut()
            .and_then(|backend| backend.handle_multitouch(&mut context.inkplate, frame))
    };
    #[cfg(feature = "ui-interaction-trace")]
    crate::firmware::interaction_trace::record(
        frame.trace_id,
        32,
        frame.t_ms,
        u32::from(rendered.is_some()),
        u32::from(frame.active_mask),
    );
    state
        .touch_equivalence
        .observe_pipeline_replay_render(rendered.is_some(), "multitouch_frame");
    if let Some(dirty) = rendered {
        state.record_dirty(dirty);
    }
    process_gestures(context, state).await;
}

#[cfg(feature = "ui-interaction-trace")]
fn discard_multitouch_lookahead(next_multi: &mut Option<LvglMultitouchFrame>) -> u32 {
    if let Some(frame) = next_multi.take() {
        crate::firmware::interaction_trace::record(
            frame.trace_id,
            13,
            frame.t_ms,
            frame.trace_frame_id,
            u32::MAX,
        );
        1
    } else {
        0
    }
}

#[cfg(not(feature = "ui-interaction-trace"))]
fn discard_multitouch_lookahead(next_multi: &mut Option<LvglMultitouchFrame>) -> u32 {
    let _ = next_multi.take();
    0
}

async fn process_multitouch_reset(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    discarded: u32,
) {
    #[cfg(feature = "ui-interaction-trace")]
    let discarded = {
        let mut discarded = discarded;
        while let Ok(frame) = TOUCH_LVGL_MULTITOUCH_FRAMES.try_receive() {
            crate::firmware::interaction_trace::record(
                frame.trace_id,
                13,
                frame.t_ms,
                frame.trace_frame_id,
                u32::MAX,
            );
            discarded = discarded.saturating_add(1);
        }
        discarded
    };
    #[cfg(not(feature = "ui-interaction-trace"))]
    {
        let _ = discarded;
        while TOUCH_LVGL_MULTITOUCH_FRAMES.try_receive().is_ok() {}
    }
    let reset_ms = Instant::now().as_millis();
    let rendered = {
        // Delivery discontinuity does not retain a unique causal contact.
        // Keep forced LVGL releases at the explicit unknown origin.
        #[cfg(feature = "ui-interaction-trace")]
        let _trace_scope = crate::firmware::interaction_trace::UiScope::enter(0);
        state
            .backend
            .as_mut()
            .and_then(|backend| backend.reset_multitouch(&mut context.inkplate, reset_ms))
    };
    #[cfg(feature = "ui-interaction-trace")]
    crate::firmware::interaction_trace::record(
        0,
        33,
        reset_ms,
        discarded,
        u32::from(rendered.is_some()),
    );
    state
        .touch_equivalence
        .observe_pipeline_replay_render(rendered.is_some(), "multitouch_reset");
    if let Some(dirty) = rendered {
        state.record_dirty(dirty);
    }
    if !crate::firmware::update::transport_quiet() {
        console::println!("LVGL_MULTITOUCH phase=reset reason=delivery_discontinuity");
    }
    process_gestures(context, state).await;
}
