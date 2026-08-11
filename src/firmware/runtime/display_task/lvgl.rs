use core::sync::atomic::Ordering;

use embassy_time::Instant;

mod panel_power_lease;
mod refresh;
mod refresh_tracking;
mod state;
mod touch_equivalence;

use refresh::{full_refresh_panel, refresh_panel, service_panel_power_lease, RefreshRequest};
use refresh_tracking::CompletedRefresh;
pub(super) use state::LvglState;
use touch_equivalence::SyntheticTouchPhase;

use crate::firmware::{
    touch::{
        config::{
            TOUCH_CONTROLLER_ACTIVE_SLOTS, TOUCH_LVGL_MULTITOUCH_FRAMES,
            TOUCH_LVGL_MULTITOUCH_RESET, TOUCH_PIPELINE_EVENTS,
        },
        tasks::request_touch_pipeline_replay_probe,
        types::TouchEventKind,
    },
    types::{DisplayContext, UiCycleStepStatus},
    ui::lvgl::{
        take_full_repaint_request, take_gesture, Backend, InitError, LvglGestureEvent,
        LvglGestureKind, LvglGestureState, UiCycleStepError,
    },
};

pub(super) const SERVICE_PERIOD_MS: u64 = 8;

pub(super) async fn initialize(context: &mut DisplayContext, state: &mut LvglState) -> bool {
    let persisted_settings = context
        .app_state_store
        .load_ui_settings()
        .unwrap_or_default();
    let (backend, rendered) = match Backend::initialize(&mut context.inkplate, persisted_settings) {
        Ok(initialized) => initialized,
        Err(error) => {
            let reason = match error {
                InitError::MemoryPoolUnavailable => "memory_pool",
                InitError::DisplayCreationFailed => "display_create",
                InitError::ShellConfigurationFailed => "shell_configuration",
                InitError::SurfaceCreationFailed => "surface_create",
                InitError::SurfaceActivationFailed => "surface_activate",
                InitError::SurfaceCleanupFailed => "surface_cleanup",
                InitError::CallbackRouteUnavailable => "callback_route",
            };
            esp_println::println!("LVGL init=failed reason={}", reason);
            return false;
        }
    };
    state.backend = Some(backend);
    esp_println::println!(
        "LVGL_PANEL_TRANSPORT selected=gpio_reference partial_strategy=gate_neutral_drain cleanup=none touch_window={}",
        crate::firmware::panel_bus::touch_waveform_window_mode()
    );
    let lease_policy = state.panel_power_lease.policy();
    esp_println::println!(
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
        state.refresh_tracking.record_failure();
        esp_println::println!("LVGL init=failed reason=startup_refresh");
        return false;
    }
    let refresh_ms = Instant::now()
        .as_millis()
        .saturating_sub(refresh_started_ms);
    state
        .refresh_tracking
        .record_success(CompletedRefresh::Full);
    state.startup_refresh_complete = true;
    state.touch_equivalence.arm(Instant::now().as_millis());
    log_framebuffer_debug(context, "startup");
    esp_println::println!(
        "LVGL init=ready display=600x600 format=L8_to_I1 loop=app_event startup_rendered={} startup_refresh=full refresh_ms={}",
        rendered.is_some(),
        refresh_ms,
    );
    let active_surface = state
        .backend
        .as_ref()
        .and_then(Backend::active_surface_label)
        .unwrap_or("unknown");
    esp_println::println!("UI_STATE screen={} state=entered", active_surface);
    true
}

fn log_framebuffer_debug(context: &DisplayContext, phase: &str) {
    if option_env!("MEDITAMER_PANEL_FRAME_DEBUG").is_none() {
        return;
    }
    let snapshot = context.inkplate.binary_framebuffer_debug_snapshot();
    esp_println::println!(
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

pub(super) async fn process_cycle(
    context: &mut DisplayContext,
    state: &mut LvglState,
    allow_full_repaint: bool,
) {
    if !state.is_ready() {
        return;
    }
    service_panel_power_lease(context, state).await;
    process_synthetic_touch_equivalence(context, state).await;
    while let Ok(event) = TOUCH_PIPELINE_EVENTS.try_receive() {
        if matches!(
            event.kind,
            TouchEventKind::Down | TouchEventKind::Up | TouchEventKind::Cancel
        ) {
            let dequeue_latency_ms = Instant::now().as_millis().saturating_sub(event.t_ms);
            esp_println::println!(
                "LVGL_TOUCH phase={:?} x={} y={} queue_ms={} active_slots={}",
                event.kind,
                event.x,
                event.y,
                dequeue_latency_ms,
                TOUCH_CONTROLLER_ACTIVE_SLOTS.load(Ordering::Acquire),
            );
        }
        let rendered = state
            .backend
            .as_mut()
            .and_then(|backend| backend.handle_touch(&mut context.inkplate, event));
        state.touch_equivalence.observe_prime_event(
            event,
            rendered.is_some(),
            Instant::now().as_millis(),
        );
        let refresh_phase = match event.kind {
            TouchEventKind::Down => Some("pressed"),
            TouchEventKind::Up => Some("released"),
            TouchEventKind::Cancel => Some("cancelled"),
            TouchEventKind::Move
            | TouchEventKind::Tap
            | TouchEventKind::LongPress
            | TouchEventKind::Swipe(_) => None,
        };
        if let Some(dirty) = rendered {
            state.touch_equivalence.observe_physical(
                event,
                context.inkplate.binary_framebuffer_debug_snapshot(),
                dirty,
            );
            state.record_dirty(dirty);
        }
        let full_repaint_requested = take_full_repaint_request();
        if full_repaint_requested && allow_full_repaint {
            refresh::force_full_repaint(context, state, "sticky_button").await;
            process_gestures(context, state);
            continue;
        }
        if full_repaint_requested {
            esp_println::println!(
                "LVGL_REPAINT full_repaint=true reason=sticky_button status=rejected upload_active=true"
            );
        }
        if let Some(phase) = refresh_phase {
            if let Some(dirty) = state.take_dirty() {
                refresh_panel(
                    context,
                    state,
                    RefreshRequest::from_touch(dirty, event.t_ms, phase),
                )
                .await;
            }
        }
        process_gestures(context, state);
    }

    if TOUCH_LVGL_MULTITOUCH_RESET.swap(false, Ordering::AcqRel) {
        while TOUCH_LVGL_MULTITOUCH_FRAMES.try_receive().is_ok() {}
        let rendered = state.backend.as_mut().and_then(|backend| {
            backend.reset_multitouch(&mut context.inkplate, Instant::now().as_millis())
        });
        state
            .touch_equivalence
            .observe_pipeline_replay_render(rendered.is_some(), "multitouch_reset");
        if let Some(dirty) = rendered {
            state.record_dirty(dirty);
        }
        if !crate::firmware::firmware_update::transport_quiet() {
            esp_println::println!("LVGL_MULTITOUCH phase=reset reason=delivery_discontinuity");
        }
        process_gestures(context, state);
    }

    while let Ok(frame) = TOUCH_LVGL_MULTITOUCH_FRAMES.try_receive() {
        let rendered = state
            .backend
            .as_mut()
            .and_then(|backend| backend.handle_multitouch(&mut context.inkplate, frame));
        state
            .touch_equivalence
            .observe_pipeline_replay_render(rendered.is_some(), "multitouch_frame");
        if let Some(dirty) = rendered {
            state.record_dirty(dirty);
        }
        process_gestures(context, state);
    }

    refresh_gesture_page_when_idle(context, state).await;

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
        if let Some(dirty) = state.take_dirty() {
            refresh_panel(context, state, RefreshRequest::from_service(dirty)).await;
        }
    }
    flush_ui_settings(context, state, now_ms);
    service_panel_power_lease(context, state).await;
}

#[inline(never)]
fn flush_ui_settings(context: &mut DisplayContext, state: &mut LvglState, now_ms: u64) {
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
    esp_println::println!(
        "UI_SETTINGS_SAVE status={}",
        if saved { "complete" } else { "retry_scheduled" },
    );
}

pub(super) async fn handle_ui_cycle_step(
    context: &mut DisplayContext,
    state: &mut LvglState,
) -> UiCycleStepStatus {
    if !state.is_ready() {
        return UiCycleStepStatus::NotReady;
    }
    let (step, surface) = match state
        .backend
        .as_mut()
        .expect("ready LVGL state has backend")
        .cycle_step(&mut context.inkplate)
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
        esp_println::println!("UI_CYCLE_VISIBLE surface={} status=ok", surface);
        UiCycleStepStatus::Applied
    } else {
        UiCycleStepStatus::RefreshFailed
    }
}

#[cfg(feature = "ui-provider-fixture")]
pub(super) async fn handle_ui_provider_fixture_step(
    context: &mut DisplayContext,
    state: &mut LvglState,
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
        esp_println::println!("UI_PROVIDER_FIXTURE_VISIBLE surface={} status=ok", surface);
        UiCycleStepStatus::Applied
    } else {
        UiCycleStepStatus::RefreshFailed
    }
}

async fn process_synthetic_touch_equivalence(context: &mut DisplayContext, state: &mut LvglState) {
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
        RefreshRequest::from_synthetic_touch(dirty, event.t_ms, refresh_phase),
    )
    .await;
    if phase == SyntheticTouchPhase::Up && state.touch_equivalence.awaiting_physical_equivalence() {
        esp_println::println!(
            "PANEL_EQUIV state=awaiting_physical target=top_test status={} instruction=tap_once",
            if refreshed { "ready" } else { "refresh_failed" }
        );
    }
    if phase == SyntheticTouchPhase::Up && state.touch_equivalence.begin_pipeline_replay(refreshed)
    {
        request_touch_pipeline_replay_probe().await;
    }
}

fn process_gestures(context: &mut DisplayContext, state: &mut LvglState) {
    while let Some(event) = take_gesture() {
        match event {
            LvglGestureEvent {
                kind: LvglGestureKind::Pinch { scale },
                state,
            } => esp_println::println!(
                "LVGL_GESTURE kind=pinch state={} scale={:.3}",
                gesture_state_label(state),
                scale
            ),
            LvglGestureEvent {
                kind: LvglGestureKind::Rotation { radians },
                state,
            } => esp_println::println!(
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
            } => esp_println::println!(
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

async fn refresh_gesture_page_when_idle(context: &mut DisplayContext, state: &mut LvglState) {
    if !state.gesture_page_refresh_pending
        || TOUCH_CONTROLLER_ACTIVE_SLOTS.load(Ordering::Acquire) != 0
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
    state: &mut LvglState,
    reason: &str,
) -> bool {
    refresh::force_full_repaint(context, state, reason).await
}
