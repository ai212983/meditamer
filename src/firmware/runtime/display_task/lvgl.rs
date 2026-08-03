use embassy_time::Instant;

mod panel_power_lease;
mod refresh;
mod refresh_tracking;
mod state;

use refresh::{full_refresh_panel, refresh_panel, service_panel_power_lease, RefreshRequest};
use refresh_tracking::CompletedRefresh;
pub(super) use state::LvglState;

use crate::firmware::{
    touch::{config::TOUCH_PIPELINE_EVENTS, types::TouchEventKind},
    types::DisplayContext,
    ui::lvgl::{Backend, InitError},
};

pub(super) const SERVICE_PERIOD_MS: u64 = 8;

pub(super) async fn initialize(context: &mut DisplayContext, state: &mut LvglState) -> bool {
    let (backend, rendered) = match Backend::initialize(&mut context.inkplate) {
        Ok(initialized) => initialized,
        Err(error) => {
            let reason = match error {
                InitError::MemoryPoolUnavailable => "memory_pool",
                InitError::DisplayCreationFailed => "display_create",
            };
            esp_println::println!("LVGL init=failed reason={}", reason);
            return false;
        }
    };
    state.backend = Some(backend);
    esp_println::println!(
        "LVGL_PANEL_TRANSPORT selected=gpio_reference partial_strategy=gate_neutral_drain cleanup=none touch_window=waveform"
    );
    let lease_policy = state.panel_power_lease.policy();
    esp_println::println!(
        "LVGL_PANEL_POWER_LEASE enabled={} policy=successful_partial idle_ms={}",
        lease_policy.enabled(),
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
    esp_println::println!(
        "LVGL init=ready display=600x600 format=L8_to_I1 loop=app_event startup_rendered={} startup_refresh=full refresh_ms={}",
        rendered.is_some(),
        refresh_ms,
    );
    esp_println::println!("UI_STATE screen=home state=entered");
    true
}

pub(super) async fn process_cycle(context: &mut DisplayContext, state: &mut LvglState) {
    if !state.is_ready() {
        return;
    }
    service_panel_power_lease(context, state).await;

    while let Ok(event) = TOUCH_PIPELINE_EVENTS.try_receive() {
        if matches!(
            event.kind,
            TouchEventKind::Down | TouchEventKind::Up | TouchEventKind::Cancel
        ) {
            let dequeue_latency_ms = Instant::now().as_millis().saturating_sub(event.t_ms);
            esp_println::println!(
                "LVGL_TOUCH phase={:?} x={} y={} queue_ms={}",
                event.kind,
                event.x,
                event.y,
                dequeue_latency_ms
            );
        }
        let rendered = state
            .backend
            .as_mut()
            .and_then(|backend| backend.handle_touch(&mut context.inkplate, event));
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
            state.record_dirty(dirty);
        }
        if let Some(phase) = refresh_phase {
            // Touch sampling is reopened during the GPIO waveform and closed
            // again before panel shutdown, so pressed feedback remains visible
            // without losing the release report on the shared I2C bus.
            if let Some(dirty) = state.take_dirty() {
                refresh_panel(
                    context,
                    state,
                    RefreshRequest::from_touch(dirty, event.t_ms, phase),
                )
                .await;
            }
        }
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
    if let Some(dirty) = rendered {
        state.record_dirty(dirty);
        if let Some(dirty) = state.take_dirty() {
            refresh_panel(context, state, RefreshRequest::from_service(dirty)).await;
        }
    }
    service_panel_power_lease(context, state).await;
}

pub(super) async fn force_full_repaint(
    context: &mut DisplayContext,
    state: &mut LvglState,
    reason: &str,
) -> bool {
    refresh::force_full_repaint(context, state, reason).await
}
