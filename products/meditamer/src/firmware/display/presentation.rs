use core::sync::atomic::Ordering;

use embassy_time::Instant;

mod input;
mod runtime_clock;
mod state;
use input::drain_touch_streams_in_order;
mod touch_equivalence;

use super::panel::refresh;
use super::panel::refresh::{
    full_refresh_panel, refresh_panel, refresh_panel_strict_fast, service_panel_power_lease,
    RefreshRequest, StrictFastOutcome,
};
use super::panel::refresh_tracking::CompletedRefresh;
use super::wall_clock::request_wall_clock_snapshot;
pub(in crate::firmware::display) use state::PresentationState;
use touch_equivalence::SyntheticTouchPhase;

use crate::firmware::environment::{EnvironmentSnapshot, EnvironmentStateSnapshot};
use crate::firmware::{
    touch::{config::TOUCH_CONTROLLER_ACTIVE_SLOTS, tasks::request_touch_pipeline_replay_probe},
    types::{DisplayContext, UiCycleStepStatus, UiCycleTarget},
    ui::lvgl::{
        select_fast_update_operation, take_gesture, AmbientHomeAction, Backend,
        FastUpdateOperation, FrontlightCalibrationEffect, InitError, LvglGestureEvent,
        LvglGestureKind, LvglGestureState, UiCycleStepError,
    },
};
use shell::types::{ScreenUpdateIntent, ScreenUpdateRequest};

/// Cache observations without rendering or refreshing from the delivery path.
/// The next normal render combines them with ready touch/timer/clock changes.
pub(super) fn update_environment(state: &mut PresentationState, reading: EnvironmentSnapshot) {
    if let Some(backend) = state.backend.as_mut() {
        backend.update_environment(reading);
    }
}

pub(super) fn update_environment_state(
    state: &mut PresentationState,
    observation: EnvironmentStateSnapshot,
) {
    if let Some(backend) = state.backend.as_mut() {
        backend.update_environment_state(observation);
    }
}

pub(super) async fn show_frontlight_calibration(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    initial_level: u8,
) -> bool {
    if !state.is_ready() {
        return false;
    }
    let (opened, dirty) = state
        .backend
        .as_mut()
        .map(|backend| backend.show_frontlight_calibration(&mut context.inkplate, initial_level))
        .unwrap_or((false, None));
    if !opened {
        return false;
    }
    if let Some(dirty) = dirty {
        state.record_dirty(dirty);
    }
    if let Some(dirty) = state.take_dirty() {
        let _ = refresh_panel(context, state, RefreshRequest::from_service(dirty)).await;
    }
    true
}

pub(super) fn take_frontlight_calibration_effect(
    state: &mut PresentationState,
) -> Option<FrontlightCalibrationEffect> {
    state
        .backend
        .as_mut()
        .and_then(Backend::take_frontlight_calibration_effect)
}

pub(super) fn frontlight_calibration_active(state: &PresentationState) -> bool {
    state
        .backend
        .as_ref()
        .is_some_and(Backend::frontlight_calibration_active)
}

fn frontlight_calibration_effect_pending(state: &PresentationState) -> bool {
    state
        .backend
        .as_ref()
        .is_some_and(Backend::frontlight_calibration_effect_pending)
}

/// Install the large Backend value synchronously, returning only its render
/// status so construction and cleanup temporaries stay outside the async poll.
#[inline(never)]
fn initialize_backend_and_install(
    context: &mut DisplayContext,
    state: &mut PresentationState,
) -> Result<bool, InitError> {
    let persisted_settings = context
        .app_state_store
        .load_ui_settings()
        .unwrap_or_default();
    #[cfg(feature = "ui-initialization-fixture")]
    Backend::verify_initialization(&mut state.backend, &mut context.inkplate);
    Backend::initialize_into(
        &mut state.backend,
        &mut context.inkplate,
        persisted_settings,
    )
}

pub(super) async fn initialize(
    context: &mut DisplayContext,
    state: &mut PresentationState,
) -> bool {
    let startup_rendered = match initialize_backend_and_install(context, state) {
        Ok(rendered) => rendered,
        Err(error) => {
            let reason = match error {
                InitError::AlreadyInitialized => "already_initialized",
                InitError::MemoryPoolUnavailable => "memory_pool",
                InitError::DisplayCreationFailed => "display_create",
                InitError::InputCreationFailed => "input_create",
                InitError::ShellConfigurationFailed => "shell_configuration",
                InitError::SurfaceCreationFailed => "surface_create",
                InitError::SurfaceActivationFailed => "surface_activate",
                InitError::SurfaceCleanupFailed => "surface_cleanup",
                InitError::CallbackRouteUnavailable => "callback_route",
            };
            console::println!("LVGL init=failed reason={}", reason);
            return false;
        }
    };
    console::println!(
        "LVGL_PANEL_TRANSPORT selected=gpio_reference partial_strategy=gate_neutral_drain cleanup=none touch_window={}",
        crate::firmware::panel_bus::touch_waveform_window_mode()
    );
    let lease_policy = state.panel_power_lease.policy();
    console::println!(
        "LVGL_PANEL_FINALIZATION mode={} idle_ms={}",
        if lease_policy.enabled() {
            "terminal_hold_lease"
        } else {
            "shutdown_after_every_refresh"
        },
        lease_policy.idle_ms()
    );

    state.last_service_ms = Instant::now().as_millis();
    let refresh_started_ms = Instant::now().as_millis();
    if !full_refresh_panel(context, true).await {
        state
            .refresh_tracking
            .record_failure(Instant::now().as_millis());
        console::println!("LVGL init=failed reason=startup_refresh");
        return false;
    }
    let refresh_ms = Instant::now()
        .as_millis()
        .saturating_sub(refresh_started_ms);
    state.record_refresh_success(CompletedRefresh::Full, Instant::now().as_millis());
    log_framebuffer_debug(context, "startup");
    console::println!(
        "LVGL init=ready display=600x600 format=L8_to_I1 loop=app_event startup_rendered={} startup_refresh=full refresh_ms={}",
        startup_rendered,
        refresh_ms,
    );
    let active_surface = state
        .backend
        .as_ref()
        .and_then(Backend::active_surface_label)
        .unwrap_or("unknown");
    console::println!("UI_STATE screen={} state=entered", active_surface);
    true
}

fn log_framebuffer_debug(context: &DisplayContext, phase: &str) {
    if option_env!("MEDITAMER_PANEL_FRAME_DEBUG").is_none() {
        return;
    }
    let snapshot = context.inkplate.binary_framebuffer_debug_snapshot();
    console::println!(
        "PANEL_FRAME phase={} current_hash={:#010x} previous_hash={:?} changed_bytes={} changed_pixels={} rows={:?}..{:?} byte_columns={:?}..{:?}",
        phase,
        snapshot.current_hash,
        snapshot.previous_hash,
        snapshot.changed_bytes,
        snapshot.changed_pixels,
        snapshot.min_row,
        snapshot.max_row,
        snapshot.min_byte_column,
        snapshot.max_byte_column,
    );
}

pub(super) async fn process_cycle(context: &mut DisplayContext, state: &mut PresentationState) {
    // Service availability never prevents Clean, including recovery. Physical
    // transactions still enforce panel bus quiescence and rail safety.
    let allow_full_repaint = true;
    if !recover_if_due(context, state, allow_full_repaint).await
        || !service_pending_fast(context, state, allow_full_repaint).await
    {
        return;
    }

    process_synthetic_touch_equivalence(context, state).await;
    if state.refresh_tracking.recovery_required() {
        return;
    }
    drain_touch_streams_in_order(context, state, allow_full_repaint).await;
    if state.refresh_tracking.recovery_required() {
        return;
    }
    if frontlight_calibration_effect_pending(state) {
        return;
    }

    // Keep touch streams drained and recovery/rail cleanup live during the
    // thermal experiment, but suppress timer rendering and normal updates.
    if super::timer_paused() {
        state.last_service_ms = Instant::now().as_millis();
        service_panel_power_lease(context, state).await;
        return;
    }

    refresh_gesture_page_when_idle(context, state).await;
    if state.refresh_tracking.recovery_required() {
        return;
    }

    let now_ms = Instant::now().as_millis();
    let elapsed_ms = now_ms
        .saturating_sub(state.last_service_ms)
        .min(u64::from(u32::MAX)) as u32;
    state.last_service_ms = now_ms;
    let rendered = state
        .backend
        .as_mut()
        .and_then(|backend| backend.run_timers(&mut context.inkplate, elapsed_ms));
    state
        .touch_equivalence
        .observe_pipeline_replay_render(rendered.is_some(), "lvgl_timer");
    if let Some(dirty) = rendered {
        state.record_dirty(dirty);
    }

    if !process_screen_updates(context, state, allow_full_repaint, now_ms).await {
        return;
    }
    flush_ui_settings(context, state, now_ms);
    service_panel_power_lease(context, state).await;
}

pub(super) async fn finish_frontlight_effect_refresh(
    context: &mut DisplayContext,
    state: &mut PresentationState,
) {
    input::finish_frontlight_effect_refresh(context, state).await;
}

async fn recover_if_due(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    allow_clean: bool,
) -> bool {
    if state.backend.is_some()
        && allow_clean
        && state
            .refresh_tracking
            .recovery_retry_due(Instant::now().as_millis())
    {
        let reason = if state.refresh_tracking.startup_complete() {
            "clean_recovery_retry"
        } else {
            "startup_retry"
        };
        refresh::recover_retained_frame(context, state, reason).await;
    }
    !state.refresh_tracking.recovery_required() && state.is_ready()
}

async fn service_pending_fast(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    allow_clean: bool,
) -> bool {
    let (fast_pending, clean_pending) = state
        .backend
        .as_mut()
        .map(|backend| {
            (
                backend.has_pending_screen_update(ScreenUpdateIntent::Fast),
                backend.has_pending_screen_update(ScreenUpdateIntent::Clean),
            )
        })
        .unwrap_or((false, false));
    if !fast_pending {
        return true;
    }

    match select_fast_update_operation(
        clean_pending,
        allow_clean,
        context.inkplate.is_partial_refresh_ready(),
    ) {
        FastUpdateOperation::StrictFast => {
            let damage = state.pending_dirty;
            let update = state.backend.as_mut().and_then(|backend| {
                backend.take_pending_screen_update_for(ScreenUpdateIntent::Fast, damage)
            });
            state.pending_dirty = None;
            if let Some(dirty) = update.and_then(|update| update.damage) {
                apply_strict_fast(context, state, dirty, "screen_update").await;
            }
        }
        FastUpdateOperation::Clean => {
            let damage = state.pending_dirty;
            let intent = if clean_pending {
                ScreenUpdateIntent::Clean
            } else {
                ScreenUpdateIntent::Fast
            };
            let _ = state
                .backend
                .as_mut()
                .and_then(|backend| backend.take_pending_screen_update_for(intent, damage));
            let reason = if clean_pending {
                "screen_update"
            } else {
                "fast_safety_fallback"
            };
            refresh::force_full_repaint(context, state, reason).await;
        }
        FastUpdateOperation::Wait => return false,
    }
    !state.refresh_tracking.recovery_required()
}

async fn process_screen_updates(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    allow_clean: bool,
    now_ms: u64,
) -> bool {
    let mut clean_reason = promote_clock_removal(context, state, allow_clean);
    process_ambient_update(context, state, allow_clean, now_ms, &mut clean_reason).await;
    runtime_clock::process_runtime_clock_update(
        context,
        state,
        allow_clean,
        now_ms,
        &mut clean_reason,
    )
    .await;
    if state.refresh_tracking.recovery_required() {
        return false;
    }

    if allow_clean
        && state.backend.as_mut().is_some_and(|backend| {
            backend.pending_screen_update_intent() == Some(ScreenUpdateIntent::Clean)
        })
    {
        let damage = state.pending_dirty;
        let update = state
            .backend
            .as_mut()
            .and_then(|backend| backend.take_pending_screen_update(damage));
        debug_assert!(update.is_some_and(|update| update.intent == ScreenUpdateIntent::Clean));
        clean_reason.get_or_insert("screen_update");
    }

    if let Some(reason) = clean_reason {
        refresh::force_full_repaint(context, state, reason).await;
        return !state.refresh_tracking.recovery_required();
    }

    let update_waiting = state
        .backend
        .as_mut()
        .is_some_and(|backend| backend.pending_screen_update_intent().is_some());
    if !update_waiting {
        if let Some(dirty) = state.take_dirty() {
            refresh_panel(context, state, RefreshRequest::from_service(dirty)).await;
        }
    }
    !state.refresh_tracking.recovery_required()
}

fn promote_clock_removal(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    allow_clean: bool,
) -> Option<&'static str> {
    if allow_clean
        && state
            .backend
            .as_mut()
            .and_then(|backend| {
                backend.promote_requested_clock_removal(
                    &mut context.inkplate,
                    ScreenUpdateRequest {
                        intent: ScreenUpdateIntent::Clean,
                    },
                )
            })
            .is_some()
    {
        Some("clock_dismiss")
    } else {
        None
    }
}

async fn process_ambient_update(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    allow_clean: bool,
    now_ms: u64,
    clean_reason: &mut Option<&'static str>,
) {
    let action = state
        .backend
        .as_mut()
        .map(|backend| backend.ambient_home_poll(now_ms, allow_clean))
        .unwrap_or(AmbientHomeAction::None);
    match action {
        AmbientHomeAction::None => {}
        AmbientHomeAction::FetchForShow => {
            let policy = ClockUpdatePolicy {
                allow_clean,
                clean_pending: clean_reason.is_some(),
                now_ms,
            };
            if process_clock_update(context, state, policy).await {
                clean_reason.get_or_insert("screen_update");
            }
        }
        AmbientHomeAction::FetchForUpdate | AmbientHomeAction::FetchForReturn => {
            process_clean_ambient_update(context, state, allow_clean, now_ms, action, clean_reason)
                .await;
        }
    }
}

#[derive(Clone, Copy)]
struct ClockUpdatePolicy {
    allow_clean: bool,
    clean_pending: bool,
    now_ms: u64,
}

async fn process_clock_update(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    policy: ClockUpdatePolicy,
) -> bool {
    let clean_pending = policy.clean_pending
        || state.backend.as_mut().is_some_and(|backend| {
            backend.pending_screen_update_intent() == Some(ScreenUpdateIntent::Clean)
        });
    let admission = select_fast_update_operation(
        clean_pending,
        policy.allow_clean,
        context.inkplate.is_partial_refresh_ready(),
    );
    if admission == FastUpdateOperation::Wait {
        if let Some(backend) = state.backend.as_mut() {
            backend.mark_fast_clock_deferred();
        }
        return false;
    }

    let Some(snapshot) = request_wall_clock_snapshot()
        .await
        .filter(|snapshot| snapshot.valid)
    else {
        return false;
    };
    let update = state.backend.as_mut().and_then(|backend| {
        backend.render_clock_fast(
            &mut context.inkplate,
            snapshot.local_epoch_seconds,
            policy.now_ms,
            ScreenUpdateRequest {
                intent: ScreenUpdateIntent::Fast,
            },
        )
    });
    let Some(update) = update else {
        return false;
    };
    let clean_pending = policy.clean_pending
        || state
            .backend
            .as_mut()
            .is_some_and(|backend| backend.has_pending_screen_update(ScreenUpdateIntent::Clean));
    let operation = select_fast_update_operation(
        clean_pending,
        policy.allow_clean,
        context.inkplate.is_partial_refresh_ready(),
    );
    if operation == FastUpdateOperation::Clean {
        if let Some(backend) = state.backend.as_mut() {
            backend.clear_pending_fast_clock();
        }
        true
    } else if operation == FastUpdateOperation::StrictFast {
        if let Some(dirty) = update.damage {
            apply_strict_fast(context, state, dirty, "clock_overlay").await;
        }
        false
    } else {
        false
    }
}

async fn process_clean_ambient_update(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    allow_clean: bool,
    now_ms: u64,
    action: AmbientHomeAction,
    clean_reason: &mut Option<&'static str>,
) {
    if !allow_clean {
        console::println!(
            "LVGL_REPAINT full_repaint=true reason=ambient_home status=deferred clean_admitted=false"
        );
        return;
    }
    let snapshot = request_wall_clock_snapshot().await;
    let update = state.backend.as_mut().and_then(|backend| {
        backend.ambient_home_apply(
            &mut context.inkplate,
            action,
            snapshot,
            now_ms,
            ScreenUpdateRequest {
                intent: ScreenUpdateIntent::Clean,
            },
        )
    });
    if update.is_some() {
        *clean_reason = Some("ambient_home");
    }
}

#[inline(never)]
fn flush_ui_settings(context: &mut DisplayContext, state: &mut PresentationState, now_ms: u64) {
    let Some(settings) = state
        .backend
        .as_mut()
        .and_then(|backend| backend.take_due_settings_write(now_ms))
    else {
        return;
    };
    let saved = context.app_state_store.save_ui_settings(settings);
    if let Some(backend) = state.backend.as_mut() {
        backend.complete_settings_write(saved, Instant::now().as_millis());
    }
    console::println!(
        "UI_SETTINGS_SAVE status={}",
        if saved { "complete" } else { "retry_scheduled" },
    );
}

pub(super) async fn handle_ui_cycle_step(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    target: UiCycleTarget,
) -> UiCycleStepStatus {
    if !state.is_ready() {
        return UiCycleStepStatus::NotReady;
    }
    let (step, surface) = match state
        .backend
        .as_mut()
        .expect("ready LVGL state has backend")
        .cycle_step(&mut context.inkplate, target)
    {
        Ok(dirty) => {
            let surface = state
                .backend
                .as_ref()
                .and_then(Backend::active_surface_label)
                .unwrap_or("unknown");
            (dirty, surface)
        }
        Err(UiCycleStepError::Busy) => return UiCycleStepStatus::Busy,
        Err(UiCycleStepError::NavigationFault) => return UiCycleStepStatus::NavigationFault,
        Err(UiCycleStepError::NoDirty) => return UiCycleStepStatus::NoDirty,
    };
    state.record_dirty(step);
    let Some(dirty) = state.take_dirty() else {
        return UiCycleStepStatus::NoDirty;
    };
    if refresh_panel(context, state, RefreshRequest::from_ui_cycle(dirty)).await {
        console::println!("UI_CYCLE_VISIBLE surface={} status=ok", surface);
        UiCycleStepStatus::Applied
    } else {
        UiCycleStepStatus::RefreshFailed
    }
}

#[cfg(feature = "ui-provider-fixture")]
pub(super) async fn handle_ui_provider_fixture_step(
    context: &mut DisplayContext,
    state: &mut PresentationState,
) -> UiCycleStepStatus {
    if !state.is_ready() {
        return UiCycleStepStatus::NotReady;
    }
    let (step, surface) = match state
        .backend
        .as_mut()
        .expect("ready LVGL state has backend")
        .provider_fixture_step(&mut context.inkplate)
    {
        Ok(dirty) => {
            let surface = state
                .backend
                .as_ref()
                .and_then(Backend::active_surface_label)
                .unwrap_or("unknown");
            (dirty, surface)
        }
        Err(UiCycleStepError::Busy) => return UiCycleStepStatus::Busy,
        Err(UiCycleStepError::NavigationFault) => return UiCycleStepStatus::NavigationFault,
        Err(UiCycleStepError::NoDirty) => return UiCycleStepStatus::NoDirty,
    };
    state.record_dirty(step);
    let Some(dirty) = state.take_dirty() else {
        return UiCycleStepStatus::NoDirty;
    };
    if refresh_panel(context, state, RefreshRequest::from_ui_cycle(dirty)).await {
        console::println!("UI_PROVIDER_FIXTURE_VISIBLE surface={} status=ok", surface);
        UiCycleStepStatus::Applied
    } else {
        UiCycleStepStatus::RefreshFailed
    }
}

async fn process_synthetic_touch_equivalence(
    context: &mut DisplayContext,
    state: &mut PresentationState,
) {
    let now_ms = Instant::now().as_millis();
    let Some((phase, event)) = state.touch_equivalence.take_synthetic_event(now_ms) else {
        return;
    };
    let rendered = state
        .backend
        .as_mut()
        .and_then(|backend| backend.handle_touch(&mut context.inkplate, event));
    let Some(dirty) = rendered else {
        state.touch_equivalence.synthetic_render_missing(phase);
        return;
    };
    state.touch_equivalence.record_synthetic(
        phase,
        context.inkplate.binary_framebuffer_debug_snapshot(),
        dirty,
        now_ms,
    );
    state.record_dirty(dirty);
    let Some(dirty) = state.take_dirty() else {
        return;
    };
    let refresh_phase = match phase {
        SyntheticTouchPhase::Down => "synthetic_pressed",
        SyntheticTouchPhase::Up => "synthetic_released",
    };
    let refreshed = refresh_panel(
        context,
        state,
        RefreshRequest::from_synthetic_touch(dirty, event.time_ms(), refresh_phase),
    )
    .await;
    if phase == SyntheticTouchPhase::Up && state.touch_equivalence.awaiting_physical_equivalence() {
        console::println!(
            "PANEL_EQUIV state=awaiting_physical target=top_test status={} instruction=tap_once",
            if refreshed { "ready" } else { "refresh_failed" }
        );
    }
    if phase == SyntheticTouchPhase::Up && state.touch_equivalence.begin_pipeline_replay(refreshed)
    {
        request_touch_pipeline_replay_probe().await;
    }
}

async fn process_gestures(context: &mut DisplayContext, state: &mut PresentationState) {
    while let Some(event) = take_gesture() {
        match event {
            LvglGestureEvent {
                kind: LvglGestureKind::Pinch { scale },
                state,
            } => console::println!(
                "LVGL_GESTURE kind=pinch state={} scale={:.3}",
                gesture_state_label(state),
                scale
            ),
            LvglGestureEvent {
                kind: LvglGestureKind::Rotation { radians },
                state,
            } => console::println!(
                "LVGL_GESTURE kind=rotation state={} radians={:.3}",
                gesture_state_label(state),
                radians
            ),
            LvglGestureEvent {
                kind:
                    LvglGestureKind::TwoFingerSwipe {
                        direction,
                        distance_px,
                    },
                state,
            } => console::println!(
                "LVGL_GESTURE kind=two_finger_swipe state={} direction={:?} distance_px={:.1}",
                gesture_state_label(state),
                direction,
                distance_px
            ),
        }
        let rendered = state
            .backend
            .as_mut()
            .and_then(|backend| backend.show_gesture(&mut context.inkplate, event));
        if let Some(dirty) = rendered {
            state.record_dirty(dirty);
            state.gesture_page_refresh_pending = true;
        }
    }
}

async fn refresh_gesture_page_when_idle(
    context: &mut DisplayContext,
    state: &mut PresentationState,
) {
    let semantic_update_waiting = state
        .backend
        .as_mut()
        .is_some_and(|backend| backend.pending_screen_update_intent().is_some());
    if !state.gesture_page_refresh_pending
        || TOUCH_CONTROLLER_ACTIVE_SLOTS.load(Ordering::Acquire) != 0
        || semantic_update_waiting
    {
        return;
    }
    state.gesture_page_refresh_pending = false;
    if let Some(dirty) = state.take_dirty() {
        refresh_panel(context, state, RefreshRequest::from_service(dirty)).await;
    }
}

const fn gesture_state_label(state: LvglGestureState) -> &'static str {
    match state {
        LvglGestureState::Ended => "ended",
    }
}

pub(super) async fn force_full_repaint(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    reason: &str,
) -> bool {
    refresh::force_full_repaint(context, state, reason).await
}

/// Pushes an already-rendered Fast clock frame and resolves the deferred
/// semantic action bookkeeping from the result (ADR-0025 item 3). A push
/// failure -- `NotReady` (post-render race) or `Failed` (driver error) --
/// already entered the recovery barrier inside `refresh_panel_strict_fast`;
/// clearing the pending marker here hands presentation to that barrier
/// rather than retrying Fast specifically, since the next Clean recovery
/// will present whatever is already rendered.
async fn apply_strict_fast(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    dirty: crate::firmware::ui::lvgl::DirtyArea,
    source: &str,
) {
    // Every outcome clears the pending marker: `Pushed` presented it, and
    // `NotReady`/`Failed` already recorded their own recovery-barrier
    // bookkeeping inside `refresh_panel_strict_fast` -- the next Clean
    // recovery presents whatever is currently rendered, so there is nothing
    // left for a Fast-specific retry to do.
    let _: StrictFastOutcome = refresh_panel_strict_fast(context, state, dirty, source).await;
    if let Some(backend) = state.backend.as_mut() {
        backend.clear_pending_fast_clock();
    }
}

#[cfg(feature = "cpu-load")]
pub(super) fn update_cpu_load(state: &mut PresentationState, reading: Option<cpu_load::Snapshot>) {
    if let Some(backend) = state.backend.as_mut() {
        backend.update_cpu_load(reading);
    }
}
