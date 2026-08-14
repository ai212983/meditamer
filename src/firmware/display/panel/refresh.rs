use embassy_futures::yield_now;
use embassy_time::{Instant, Timer};

use super::super::presentation::PresentationState;
use super::lease::{should_shutdown_parked_panel, LeaseMaintenance, LeaseRefreshKind};
use super::refresh_tracking::CompletedRefresh;
use crate::firmware::{
    config::AUTOMATIC_FULL_REFRESH_AFTER_PARTIAL_UPDATES, panel_bus, types::DisplayContext,
    ui::lvgl::DirtyArea,
};
use crate::platform::inkplate::{PartialGateDrainTiming, E_INK_HEIGHT};

const FORCE_FULL_REFRESH_DIAGNOSTIC: bool =
    option_env!("MEDITAMER_TOUCH_FORCE_FULL_REFRESH").is_some();
const POST_RELEASE_SETTLE_DIAGNOSTIC: bool =
    option_env!("MEDITAMER_TOUCH_POST_RELEASE_SETTLE").is_some();

pub(in crate::firmware::display) struct RefreshRequest<'a> {
    dirty: DirtyArea,
    input_ms: Option<u64>,
    phase: &'a str,
    source: &'a str,
}

impl<'a> RefreshRequest<'a> {
    pub(in crate::firmware::display) const fn from_touch(
        dirty: DirtyArea,
        input_ms: u64,
        phase: &'a str,
    ) -> Self {
        Self {
            dirty,
            input_ms: Some(input_ms),
            phase,
            source: "physical_touch",
        }
    }

    pub(in crate::firmware::display) const fn from_synthetic_touch(
        dirty: DirtyArea,
        input_ms: u64,
        phase: &'a str,
    ) -> Self {
        Self {
            dirty,
            input_ms: Some(input_ms),
            phase,
            source: "synthetic_touch",
        }
    }

    pub(in crate::firmware::display) const fn from_service(dirty: DirtyArea) -> Self {
        Self {
            dirty,
            input_ms: None,
            phase: "service",
            source: "service",
        }
    }

    pub(in crate::firmware::display) const fn from_ui_cycle(dirty: DirtyArea) -> Self {
        Self {
            dirty,
            input_ms: None,
            phase: "ui_cycle_step",
            source: "serial_ui_step",
        }
    }
}

#[derive(Clone, Copy)]
struct RefreshPlan {
    recovery_requested: bool,
    full_refresh: bool,
    hold_terminal_state: bool,
}

impl RefreshPlan {
    fn for_state(state: &PresentationState) -> Self {
        let recovery_requested = state.refresh_tracking.recovery_required();
        let full_refresh = FORCE_FULL_REFRESH_DIAGNOSTIC
            || state
                .refresh_tracking
                .should_request_full(AUTOMATIC_FULL_REFRESH_AFTER_PARTIAL_UPDATES);
        let hold_terminal_state = !full_refresh
            && state
                .panel_power_lease
                .policy()
                .should_hold_terminal_state_for_partial();
        Self {
            recovery_requested,
            full_refresh,
            hold_terminal_state,
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

pub(in crate::firmware::display) async fn service_panel_power_lease(
    context: &mut DisplayContext,
    state: &mut PresentationState,
) {
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
                    "LVGL_PANEL_LEASE event=shutdown mode=terminal_hold reason=idle_timeout status=ok active_ms={} shutdown_ms={}",
                    active_ms,
                    Instant::now()
                        .as_millis()
                        .saturating_sub(shutdown_started_ms)
                ),
                Err(error) => {
                    state.refresh_tracking.record_failure();
                    esp_println::println!(
                        "LVGL_PANEL_LEASE event=shutdown mode=terminal_hold reason=idle_timeout status=error error={:?} recovery=full_next active_ms={} shutdown_ms={}",
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

pub(in crate::firmware::display) async fn force_full_repaint(
    context: &mut DisplayContext,
    state: &mut PresentationState,
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

pub(in crate::firmware::display) async fn refresh_panel(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    request: RefreshRequest<'_>,
) -> bool {
    let refresh_started_ms = Instant::now().as_millis();
    let plan = RefreshPlan::for_state(state);

    if POST_RELEASE_SETTLE_DIAGNOSTIC && request.phase == "released" {
        esp_println::println!("LVGL_REFRESH phase=released settle_ms=5000");
        Timer::after_millis(5_000).await;
    }

    if option_env!("MEDITAMER_PANEL_FRAME_DEBUG").is_some() {
        let snapshot = context.inkplate.binary_framebuffer_debug_snapshot();
        esp_println::println!(
            "PANEL_FRAME phase=pre_refresh current_hash={:#010x} previous_hash={:?} changed_bytes={} changed_pixels={} rows={:?}..{:?} byte_columns={:?}..{:?}",
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

    // Keep an executor boundary between an LVGL flush into the binary
    // framebuffer and the interrupt-locked panel scan. Hardware A/B testing
    // showed identical framebuffer bytes corrupt without this handoff and
    // remain stable with a single yield; no timed delay is required.
    yield_now().await;

    let bus_quiet_started_ms = Instant::now().as_millis();
    panel_bus::suspend_clients().await;
    let bus_quiet_wait_ms = Instant::now()
        .as_millis()
        .saturating_sub(bus_quiet_started_ms);

    let mut refresh_result = if plan.full_refresh {
        context
            .inkplate
            .display_bw_cooperative_async(
                false,
                panel_bus::open_touch_waveform_window,
                panel_bus::close_touch_waveform_window,
            )
            .await
            .map(|()| PanelRefreshOutcome {
                completed: CompletedRefresh::Full,
                timing: None,
            })
    } else {
        context
            .inkplate
            .display_bw_partial_gate_drain_no_cleanup_cooperative_async(
                plan.hold_terminal_state,
                panel_bus::open_touch_waveform_window,
                panel_bus::close_touch_waveform_window,
            )
            .await
            .map(|timing| PanelRefreshOutcome {
                completed: if timing.full_fallback {
                    CompletedRefresh::Full
                } else if timing.scan_rows == 0 {
                    CompletedRefresh::NoChange
                } else {
                    CompletedRefresh::Partial
                },
                timing: Some(timing),
            })
    };

    // A requested partial may use the full-refresh fallback when its baseline
    // is unavailable. The driver honored the requested terminal hold, so shut
    // the panel down while shared clients are still suspended. Treat a failed
    // shutdown as a failed refresh so recovery is forced on the next request.
    let parked_panel_needs_shutdown = match &refresh_result {
        Ok(outcome) => should_shutdown_parked_panel(
            plan.hold_terminal_state,
            lease_refresh_kind(outcome.completed),
            outcome.timing.is_some_and(|timing| timing.full_fallback),
        ),
        Err(_) => false,
    };
    if parked_panel_needs_shutdown {
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
                "LVGL_REFRESH phase={} source={} status=error requested={} error={:?} recovery=full_next dirty={},{},{},{} terminal_hold_requested={} bus_quiet_wait_ms={} refresh_ms={} input_to_visible_ms={} partial_count={}",
                request.phase,
                request.source,
                if plan.full_refresh { "full" } else { "partial" },
                error,
                request.dirty.x1,
                request.dirty.y1,
                request.dirty.x2,
                request.dirty.y2,
                plan.hold_terminal_state,
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
    state: &mut PresentationState,
    request: RefreshRequest<'_>,
    plan: RefreshPlan,
    measurements: RefreshMeasurements,
    outcome: PanelRefreshOutcome,
) -> bool {
    state.refresh_tracking.record_success(outcome.completed);
    let lease_renewed = state.panel_power_lease.record_refresh_success(
        lease_refresh_kind(outcome.completed),
        measurements.finished_ms,
    );
    let (kind, partial_strategy) = match outcome.completed {
        CompletedRefresh::Full => ("full", "not_applicable"),
        CompletedRefresh::Partial => ("partial", "gate_neutral_drain"),
        CompletedRefresh::NoChange => ("no_change", "not_applicable"),
    };
    let partial = PartialRefreshMetrics::from_timing(outcome.timing);
    esp_println::println!(
        "LVGL_REFRESH phase={} source={} status=ok kind={} recovery_requested={} dirty={},{},{},{} terminal_hold_requested={} lease_renewed={} bus_quiet_wait_ms={} refresh_ms={} input_to_visible_ms={} partial_count={} partial_strategy={} transition_scan_rows={} neutral_drain_rows={} cleanup_zero_passes=0 cleanup_neutral_passes=0 waveform_us={} full_fallback={}",
        request.phase,
        request.source,
        kind,
        plan.recovery_requested,
        request.dirty.x1,
        request.dirty.y1,
        request.dirty.x2,
        request.dirty.y2,
        plan.hold_terminal_state,
        lease_renewed,
        measurements.bus_quiet_wait_ms,
        measurements.refresh_ms,
        measurements.input_to_visible_ms,
        state.refresh_tracking.partial_count(),
        partial_strategy,
        partial.transition_scan_rows,
        partial.neutral_drain_rows,
        partial.waveform_us,
        partial.full_fallback,
    );
    true
}

const fn lease_refresh_kind(completed: CompletedRefresh) -> LeaseRefreshKind {
    match completed {
        CompletedRefresh::Full => LeaseRefreshKind::Full,
        CompletedRefresh::Partial => LeaseRefreshKind::Partial,
        CompletedRefresh::NoChange => LeaseRefreshKind::NoChange,
    }
}

pub(in crate::firmware::display) async fn full_refresh_panel(
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
