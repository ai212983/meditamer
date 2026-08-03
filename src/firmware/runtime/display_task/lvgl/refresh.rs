use embassy_time::Instant;

use super::{
    panel_power_lease::{LeaseMaintenance, LeaseRefreshKind},
    refresh_tracking::CompletedRefresh,
    LvglState,
};
use crate::drivers::inkplate::{PartialGateDrainTiming, E_INK_HEIGHT};
use crate::firmware::{
    config::AUTOMATIC_FULL_REFRESH_AFTER_PARTIAL_UPDATES, panel_bus, types::DisplayContext,
    ui::lvgl::DirtyArea,
};

pub(super) struct RefreshRequest<'a> {
    dirty: DirtyArea,
    input_ms: Option<u64>,
    phase: &'a str,
}

impl<'a> RefreshRequest<'a> {
    pub(super) const fn from_touch(dirty: DirtyArea, input_ms: u64, phase: &'a str) -> Self {
        Self {
            dirty,
            input_ms: Some(input_ms),
            phase,
        }
    }

    pub(super) const fn from_service(dirty: DirtyArea) -> Self {
        Self {
            dirty,
            input_ms: None,
            phase: "service",
        }
    }
}

#[derive(Clone, Copy)]
struct RefreshPlan {
    recovery_requested: bool,
    full_refresh: bool,
    leave_panel_on: bool,
}

impl RefreshPlan {
    fn for_state(state: &LvglState) -> Self {
        let recovery_requested = state.refresh_tracking.recovery_required();
        let full_refresh = state
            .refresh_tracking
            .should_request_full(AUTOMATIC_FULL_REFRESH_AFTER_PARTIAL_UPDATES);
        let leave_panel_on = !full_refresh
            && state
                .panel_power_lease
                .policy()
                .should_leave_on_for_partial();
        Self {
            recovery_requested,
            full_refresh,
            leave_panel_on,
        }
    }
}

#[derive(Clone, Copy)]
struct PanelRefreshOutcome {
    completed: CompletedRefresh,
    timing: Option<PartialGateDrainTiming>,
}

#[derive(Clone, Copy)]
struct RefreshMeasurements {
    finished_ms: u64,
    bus_quiet_wait_ms: u64,
    refresh_ms: u64,
    input_to_visible_ms: u64,
}

impl RefreshMeasurements {
    fn finish(refresh_started_ms: u64, bus_quiet_wait_ms: u64, input_ms: Option<u64>) -> Self {
        let refresh_finished_ms = Instant::now().as_millis();
        let input_to_visible_ms = match input_ms {
            Some(touch_ms) => refresh_finished_ms.saturating_sub(touch_ms),
            None => 0,
        };
        Self {
            finished_ms: refresh_finished_ms,
            bus_quiet_wait_ms,
            refresh_ms: refresh_finished_ms.saturating_sub(refresh_started_ms),
            input_to_visible_ms,
        }
    }
}

#[derive(Clone, Copy)]
struct PartialRefreshMetrics {
    full_fallback: bool,
    transition_scan_rows: usize,
    neutral_drain_rows: usize,
    waveform_us: u64,
}

impl PartialRefreshMetrics {
    fn from_timing(timing: Option<PartialGateDrainTiming>) -> Self {
        let Some(timing) = timing else {
            return Self {
                full_fallback: false,
                transition_scan_rows: 0,
                neutral_drain_rows: 0,
                waveform_us: 0,
            };
        };
        let transition_scan_rows = if timing.full_fallback {
            0
        } else {
            timing.scan_rows
        };
        Self {
            full_fallback: timing.full_fallback,
            transition_scan_rows,
            neutral_drain_rows: if transition_scan_rows == 0 {
                0
            } else {
                E_INK_HEIGHT - transition_scan_rows
            },
            waveform_us: timing.waveform_us(),
        }
    }
}

pub(super) async fn service_panel_power_lease(context: &mut DisplayContext, state: &mut LvglState) {
    let now_ms = Instant::now().as_millis();
    match state.panel_power_lease.take_maintenance(now_ms) {
        LeaseMaintenance::None => {}
        LeaseMaintenance::ShutDown { active_ms } => {
            let shutdown_started_ms = Instant::now().as_millis();
            panel_bus::suspend_clients().await;
            let shutdown = context.inkplate.eink_off_async().await;
            panel_bus::resume_clients(false).await;
            match shutdown {
                Ok(()) => esp_println::println!(
                    "LVGL_PANEL_POWER_LEASE event=shutdown reason=idle_timeout status=ok active_ms={} shutdown_ms={}",
                    active_ms,
                    Instant::now()
                        .as_millis()
                        .saturating_sub(shutdown_started_ms)
                ),
                Err(error) => {
                    state.refresh_tracking.record_failure();
                    esp_println::println!(
                        "LVGL_PANEL_POWER_LEASE event=shutdown reason=idle_timeout status=error error={:?} recovery=full_next active_ms={} shutdown_ms={}",
                        error,
                        active_ms,
                        Instant::now()
                            .as_millis()
                            .saturating_sub(shutdown_started_ms)
                    );
                }
            }
        }
    }
}

pub(super) async fn force_full_repaint(
    context: &mut DisplayContext,
    state: &mut LvglState,
    reason: &str,
) -> bool {
    let _ = state
        .backend
        .as_mut()
        .and_then(|backend| backend.invalidate(&mut context.inkplate));
    let started_ms = Instant::now().as_millis();
    let refresh_succeeded = full_refresh_panel(context, true).await;
    state.panel_power_lease.mark_panel_off();
    if refresh_succeeded {
        state.startup_refresh_complete = true;
        state
            .refresh_tracking
            .record_success(CompletedRefresh::Full);
    } else {
        state.refresh_tracking.record_failure();
    }
    state.pending_dirty = None;
    state.last_service_ms = Instant::now().as_millis();
    esp_println::println!(
        "LVGL_REPAINT full_repaint=true reason={} status={} refresh_ms={}",
        reason,
        if refresh_succeeded { "ok" } else { "error" },
        Instant::now().as_millis().saturating_sub(started_ms)
    );
    refresh_succeeded
}

pub(super) async fn refresh_panel(
    context: &mut DisplayContext,
    state: &mut LvglState,
    request: RefreshRequest<'_>,
) -> bool {
    let refresh_started_ms = Instant::now().as_millis();
    let plan = RefreshPlan::for_state(state);
    // The opt-in lease parks every successful partial transaction. Full
    // refreshes and recovery transactions retain the mandatory shutdown
    // boundary, while the idle timeout owns the eventual partial shutdown.
    let bus_quiet_started_ms = Instant::now().as_millis();
    panel_bus::suspend_clients().await;
    let bus_quiet_wait_ms = Instant::now()
        .as_millis()
        .saturating_sub(bus_quiet_started_ms);
    let mut refresh_result = if plan.full_refresh {
        match context
            .inkplate
            .display_bw_cooperative_async(
                plan.leave_panel_on,
                panel_bus::open_touch_waveform_window,
                panel_bus::close_touch_waveform_window,
            )
            .await
        {
            Ok(()) => Ok(PanelRefreshOutcome {
                completed: CompletedRefresh::Full,
                timing: None,
            }),
            Err(error) => Err(error),
        }
    } else {
        match context
            .inkplate
            .display_bw_partial_gate_drain_no_cleanup_cooperative_async(
                plan.leave_panel_on,
                panel_bus::open_touch_waveform_window,
                panel_bus::close_touch_waveform_window,
            )
            .await
        {
            Ok(timing) => {
                let completed = if timing.full_fallback {
                    CompletedRefresh::Full
                } else if timing.scan_rows == 0 {
                    CompletedRefresh::NoChange
                } else {
                    CompletedRefresh::Partial
                };
                Ok(PanelRefreshOutcome {
                    completed,
                    timing: Some(timing),
                })
            }
            Err(error) => Err(error),
        }
    };
    // The partial API can fall back to a full waveform when its baseline is
    // unavailable. That is not a successful partial, so close the parked
    // transaction before releasing shared I2C ownership.
    let parked_full_fallback = match &refresh_result {
        Ok(outcome) => {
            plan.leave_panel_on
                && matches!(outcome.completed, CompletedRefresh::Full)
                && matches!(outcome.timing, Some(timing) if timing.full_fallback)
        }
        Err(_) => false,
    };
    if parked_full_fallback {
        if let Err(error) = context.inkplate.eink_off_async().await {
            refresh_result = Err(error);
        }
    }
    panel_bus::resume_clients(false).await;
    let measurements =
        RefreshMeasurements::finish(refresh_started_ms, bus_quiet_wait_ms, request.input_ms);
    match refresh_result {
        Ok(outcome) => record_refresh_success(state, request, plan, measurements, outcome),
        Err(error) => {
            state.refresh_tracking.record_failure();
            state.panel_power_lease.mark_panel_off();
            esp_println::println!(
                "LVGL_REFRESH phase={} status=error requested={} error={:?} recovery=full_next dirty={},{},{},{} leave_on_requested={} bus_quiet_wait_ms={} refresh_ms={} input_to_visible_ms={} partial_count={}",
                request.phase,
                if plan.full_refresh { "full" } else { "partial" },
                error,
                request.dirty.x1,
                request.dirty.y1,
                request.dirty.x2,
                request.dirty.y2,
                plan.leave_panel_on,
                measurements.bus_quiet_wait_ms,
                measurements.refresh_ms,
                measurements.input_to_visible_ms,
                state.refresh_tracking.partial_count()
            );
            false
        }
    }
}

fn record_refresh_success(
    state: &mut LvglState,
    request: RefreshRequest<'_>,
    plan: RefreshPlan,
    measurements: RefreshMeasurements,
    outcome: PanelRefreshOutcome,
) -> bool {
    state.refresh_tracking.record_success(outcome.completed);
    let (lease_kind, kind, partial_strategy) = match outcome.completed {
        CompletedRefresh::Full => (LeaseRefreshKind::Full, "full", "not_applicable"),
        CompletedRefresh::Partial => (LeaseRefreshKind::Partial, "partial", "gate_neutral_drain"),
        CompletedRefresh::NoChange => (LeaseRefreshKind::NoChange, "no_change", "not_applicable"),
    };
    let lease_renewed = state
        .panel_power_lease
        .record_refresh_success(lease_kind, measurements.finished_ms);
    let partial = PartialRefreshMetrics::from_timing(outcome.timing);
    esp_println::println!(
        "LVGL_REFRESH phase={} status=ok kind={} recovery_requested={} dirty={},{},{},{} leave_on_requested={} power_lease_renewed={} bus_quiet_wait_ms={} refresh_ms={} input_to_visible_ms={} partial_count={} partial_strategy={} transition_scan_rows={} neutral_drain_rows={} cleanup_zero_passes=0 cleanup_neutral_passes=0 waveform_us={} full_fallback={}",
        request.phase,
        kind,
        plan.recovery_requested,
        request.dirty.x1,
        request.dirty.y1,
        request.dirty.x2,
        request.dirty.y2,
        plan.leave_panel_on,
        lease_renewed,
        measurements.bus_quiet_wait_ms,
        measurements.refresh_ms,
        measurements.input_to_visible_ms,
        state.refresh_tracking.partial_count(),
        partial_strategy,
        partial.transition_scan_rows,
        partial.neutral_drain_rows,
        partial.waveform_us,
        partial.full_fallback
    );
    true
}

pub(super) async fn full_refresh_panel(
    context: &mut DisplayContext,
    reset_touch_pipeline: bool,
) -> bool {
    panel_bus::suspend_clients().await;
    let refresh_result = context.inkplate.display_bw_async(false).await;
    panel_bus::resume_clients(reset_touch_pipeline).await;
    match refresh_result {
        Ok(()) => true,
        Err(error) => {
            esp_println::println!("LVGL_FULL_REFRESH status=error error={:?}", error);
            false
        }
    }
}
