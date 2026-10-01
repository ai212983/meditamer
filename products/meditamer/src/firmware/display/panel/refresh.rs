use embassy_futures::yield_now;
use embassy_time::{Instant, Timer};

use super::super::presentation::PresentationState;
use super::lease::{should_shutdown_parked_panel, LeaseMaintenance, LeaseRefreshKind};
use super::refresh_tracking::CompletedRefresh;
use crate::firmware::{panel_bus, types::DisplayContext, ui::lvgl::DirtyArea};
use inkplate_tempera::{PartialGateDrainError, PartialGateDrainTiming, E_INK_HEIGHT};

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
        let full_refresh =
            FORCE_FULL_REFRESH_DIAGNOSTIC || state.refresh_tracking.should_request_full();
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
    first_changed_row: usize,
    last_changed_row: usize,
    changed_span_rows: usize,
    source_skip_candidate_rows: usize,
    waveform_us: u64,
}

impl PartialRefreshMetrics {
    fn from_timing(timing: Option<PartialGateDrainTiming>) -> Self {
        let Some(timing) = timing else {
            return Self {
                full_fallback: false,
                transition_scan_rows: 0,
                neutral_drain_rows: 0,
                first_changed_row: 0,
                last_changed_row: 0,
                changed_span_rows: 0,
                source_skip_candidate_rows: 0,
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
            first_changed_row: timing.first_changed_row,
            last_changed_row: timing.last_changed_row,
            changed_span_rows: timing.changed_span_rows,
            source_skip_candidate_rows: timing.source_skip_candidate_rows,
            waveform_us: timing.waveform_us(),
        }
    }
}

pub(in crate::firmware::display) async fn service_panel_power_lease(
    context: &mut DisplayContext,
    state: &mut PresentationState,
) {
    let now_ms = Instant::now().as_millis();
    if context.inkplate.panel_cleanup_pending() {
        state.panel_power_lease.schedule_cleanup_retry(now_ms);
    }
    match state.panel_power_lease.take_maintenance(now_ms) {
        LeaseMaintenance::None => {}
        LeaseMaintenance::ShutDown { active_ms } => {
            let shutdown_started_ms = Instant::now().as_millis();
            let reason = if context.inkplate.panel_cleanup_pending() {
                "recovery"
            } else {
                "idle_timeout"
            };
            let clients_quiet = panel_bus::suspend_clients().await;
            // A client's cleanup that could not confirm the shared bus
            // quiescent means touching the PMIC now risks colliding with
            // whatever transaction it could not finish -- skip the shutdown
            // attempt entirely rather than proceed anyway ("A failed
            // acknowledgement prevents the protected sleep or panel
            // operation").
            let shutdown_ms = || {
                Instant::now()
                    .as_millis()
                    .saturating_sub(shutdown_started_ms)
            };
            if !clients_quiet {
                context.inkplate.isolate_panel_for_cleanup();
                state
                    .refresh_tracking
                    .record_failure(Instant::now().as_millis());
                console::println!(
                    "LVGL_PANEL_LEASE event=shutdown mode=terminal_hold reason={} status=error error=clients_not_quiesced recovery=full_next active_ms={} shutdown_ms={}",
                    reason,
                    active_ms,
                    shutdown_ms()
                );
            } else {
                match context.inkplate.eink_off_async().await {
                    Ok(()) => console::println!(
                        "LVGL_PANEL_LEASE event=shutdown mode=terminal_hold reason={} status=ok active_ms={} shutdown_ms={}",
                        reason,
                        active_ms,
                        shutdown_ms()
                    ),
                    Err(error) => {
                        state.refresh_tracking.record_failure(Instant::now().as_millis());
                        console::println!(
                            "LVGL_PANEL_LEASE event=shutdown mode=terminal_hold reason={} status=error error={:?} recovery=full_next active_ms={} shutdown_ms={}",
                            reason,
                            error,
                            active_ms,
                            shutdown_ms()
                        );
                    }
                }
            }
            panel_bus::resume_clients(false).await;
            track_panel_cleanup(context, state);
        }
    }
}

fn track_panel_cleanup(context: &DisplayContext, state: &mut PresentationState) {
    if context.inkplate.panel_cleanup_pending() {
        state
            .panel_power_lease
            .schedule_cleanup_retry(Instant::now().as_millis());
    } else {
        state.panel_power_lease.mark_panel_off();
    }
}

pub(in crate::firmware::display) async fn force_full_repaint(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    reason: &str,
) -> bool {
    force_full_repaint_traced(context, state, reason, None).await
}

pub(in crate::firmware::display) async fn force_full_repaint_traced(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    reason: &str,
    request_id: Option<u64>,
) -> bool {
    let render = !state.refresh_tracking.recovery_required();
    force_full_repaint_impl(context, state, reason, request_id, render).await
}

/// Retry a failed rendered transaction from the framebuffer already retained
/// behind the recovery barrier. No LVGL callback, navigation, timer, or cached
/// observation is allowed to mutate that frame before the Clean scan.
pub(in crate::firmware::display) async fn recover_retained_frame(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    reason: &str,
) -> bool {
    force_full_repaint_impl(context, state, reason, None, false).await
}

async fn force_full_repaint_impl(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    reason: &str,
    request_id: Option<u64>,
    render: bool,
) -> bool {
    if let Some(id) = request_id {
        super::observation_trace::begin(id);
    }
    if render {
        let _ = state
            .backend
            .as_mut()
            .and_then(|backend| backend.invalidate(&mut context.inkplate));
    }
    let started_ms = Instant::now().as_millis();
    let (scan_succeeded, resumed) = full_refresh_panel_traced(context, true, request_id).await;
    let refresh_succeeded = scan_succeeded && resumed;
    track_panel_cleanup(context, state);
    if refresh_succeeded {
        state.record_refresh_success(CompletedRefresh::Full, Instant::now().as_millis());
        // A successful Clean scan is the sole commit point for a staged
        // clock-overlay removal (ADR-0025): the overlay was hidden -- but
        // its shell ownership and modal capture retained -- ahead of this
        // scan, and only now does teardown actually run. A scan failure
        // leaves the staging in place behind the recovery barrier below.
        if let Some(backend) = state.backend.as_mut() {
            if backend.clock_removal_pending().is_some() {
                backend.commit_clock_removal();
            }
        }
    } else {
        note_refresh_failure(state, Instant::now().as_millis());
    }
    state.pending_dirty = None;
    state.last_service_ms = Instant::now().as_millis();
    console::println!(
        "LVGL_REPAINT full_repaint=true reason={} status={} refresh_ms={}",
        reason,
        if refresh_succeeded { "ok" } else { "error" },
        Instant::now().as_millis().saturating_sub(started_ms)
    );
    if let Some(id) = request_id {
        super::observation_trace::end(id, refresh_succeeded, resumed);
    }
    refresh_succeeded
}

pub(in crate::firmware::display) async fn refresh_panel(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    request: RefreshRequest<'_>,
) -> bool {
    let refresh_started_ms = Instant::now().as_millis();
    #[cfg(feature = "ui-interaction-trace")]
    if request.input_ms.is_none() {
        // This refresh was scheduled by service/timer/fixture work. Its input
        // predecessor is intentionally unknown, rather than the last touch.
        crate::firmware::interaction_trace::record(0, 35, 0, 0, 0);
    }
    let plan = RefreshPlan::for_state(state);

    if POST_RELEASE_SETTLE_DIAGNOSTIC && request.phase == "released" {
        console::println!("LVGL_REFRESH phase=released settle_ms=5000");
        Timer::after_millis(5_000).await;
    }

    if option_env!("MEDITAMER_PANEL_FRAME_DEBUG").is_some() {
        let snapshot = context.inkplate.binary_framebuffer_debug_snapshot();
        console::println!(
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
    let clients_quiet = panel_bus::suspend_clients().await;
    let bus_quiet_wait_ms = Instant::now()
        .as_millis()
        .saturating_sub(bus_quiet_started_ms);

    if !clients_quiet {
        // A client's cleanup could not confirm the shared bus quiescent --
        // the panel scan and its GPIO/PMIC/expander sequencing must not
        // touch that bus now. Skip the transaction entirely rather than
        // proceed anyway ("A failed acknowledgement prevents the protected
        // sleep or panel operation"), and force full-refresh recovery on the
        // next request, the same as any other refresh failure.
        context.inkplate.isolate_panel_for_cleanup();
        panel_bus::resume_clients(false).await;
        track_panel_cleanup(context, state);
        note_refresh_failure(state, Instant::now().as_millis());
        let measurements =
            RefreshMeasurements::finish(refresh_started_ms, bus_quiet_wait_ms, request.input_ms);
        console::println!(
            "LVGL_REFRESH phase={} source={} status=error requested={} error=clients_not_quiesced recovery=full_next dirty={},{},{},{} terminal_hold_requested={} bus_quiet_wait_ms={} refresh_ms={} input_to_visible_ms={}",
            request.phase,
            request.source,
            if plan.full_refresh { "full" } else { "partial" },
            request.dirty.x1,
            request.dirty.y1,
            request.dirty.x2,
            request.dirty.y2,
            plan.hold_terminal_state,
            measurements.bus_quiet_wait_ms,
            measurements.refresh_ms,
            measurements.input_to_visible_ms,
        );
        return false;
    }

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
    let clients_resumed = panel_bus::resume_clients(false).await;

    let measurements =
        RefreshMeasurements::finish(refresh_started_ms, bus_quiet_wait_ms, request.input_ms);
    if !clients_resumed {
        context.inkplate.isolate_panel_for_cleanup();
        note_refresh_failure(state, Instant::now().as_millis());
        track_panel_cleanup(context, state);
        console::println!(
            "LVGL_REFRESH phase={} source={} status=error requested={} error=clients_not_resumed recovery=full_next dirty={},{},{},{} terminal_hold_requested={} bus_quiet_wait_ms={} refresh_ms={} input_to_visible_ms={}",
            request.phase,
            request.source,
            if plan.full_refresh { "full" } else { "partial" },
            request.dirty.x1,
            request.dirty.y1,
            request.dirty.x2,
            request.dirty.y2,
            plan.hold_terminal_state,
            measurements.bus_quiet_wait_ms,
            measurements.refresh_ms,
            measurements.input_to_visible_ms,
        );
        return false;
    }
    match refresh_result {
        Ok(outcome) => record_refresh_success(state, request, plan, measurements, outcome),
        Err(error) => {
            note_refresh_failure(state, Instant::now().as_millis());
            track_panel_cleanup(context, state);
            console::println!(
                "LVGL_REFRESH phase={} source={} status=error requested={} error={:?} recovery=full_next dirty={},{},{},{} terminal_hold_requested={} bus_quiet_wait_ms={} refresh_ms={} input_to_visible_ms={}",
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
    state.record_refresh_success(outcome.completed, measurements.finished_ms);
    let lease_renewed = state.panel_power_lease.record_refresh_success(
        lease_refresh_kind(outcome.completed),
        measurements.finished_ms,
    );
    if cfg!(feature = "firmware-trace") {
        let (kind, partial_strategy) = match outcome.completed {
            CompletedRefresh::Full => ("full", "not_applicable"),
            CompletedRefresh::Partial => ("partial", "gate_neutral_drain"),
            CompletedRefresh::NoChange => ("no_change", "not_applicable"),
        };
        let partial = PartialRefreshMetrics::from_timing(outcome.timing);
        console::println!(
        "LVGL_REFRESH phase={} source={} status=ok kind={} recovery_requested={} dirty={},{},{},{} terminal_hold_requested={} lease_renewed={} bus_quiet_wait_ms={} refresh_ms={} input_to_visible_ms={} partial_strategy={} transition_scan_rows={} neutral_drain_rows={} framebuffer_changed_rows={}..{} changed_span_rows={} source_skip_candidate_rows={} cleanup_zero_passes=0 cleanup_neutral_passes=0 waveform_us={} full_fallback={}",
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
        partial_strategy,
        partial.transition_scan_rows,
        partial.neutral_drain_rows,
        partial.first_changed_row,
        partial.last_changed_row,
        partial.changed_span_rows,
        partial.source_skip_candidate_rows,
        partial.waveform_us,
        partial.full_fallback,
    );
    }
    true
}

const fn lease_refresh_kind(completed: CompletedRefresh) -> LeaseRefreshKind {
    match completed {
        CompletedRefresh::Full => LeaseRefreshKind::Full,
        CompletedRefresh::Partial => LeaseRefreshKind::Partial,
        CompletedRefresh::NoChange => LeaseRefreshKind::NoChange,
    }
}

/// Records a failed rendered panel transaction and resets the one-second
/// recovery-retry cadence (ADR-0025 item 6): every failure -- Fast or Clean,
/// startup or later -- retries Clean from a stable, event-independent
/// deadline rather than only the pre-startup case `RefreshTracking` itself
/// still tracks.
fn note_refresh_failure(state: &mut PresentationState, now_ms: u64) {
    state.refresh_tracking.record_failure(now_ms);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::firmware::display) enum StrictFastOutcome {
    /// Pushed to the panel via the strict binary partial waveform.
    Pushed,
    /// The transaction reached the driver but it reported the partial
    /// baseline not ready -- a race against this function's own preflight
    /// check, since the caller only reaches here after that check passed.
    /// Already-rendered LVGL content is retained and presented by whatever
    /// Clean recovery runs next.
    NotReady,
    /// A driver error. Same recovery handling as `NotReady`.
    Failed,
}

/// Pushes already-rendered LVGL content via the strict, non-fallback binary
/// partial waveform (ADR-0025 item 5's Fast path). Callers must preflight
/// with `context.inkplate.is_partial_refresh_ready()` *before* rendering --
/// this function does not render and does not fall back to a clean refresh;
/// it only reports whether the panel push itself succeeded.
pub(in crate::firmware::display) async fn refresh_panel_strict_fast(
    context: &mut DisplayContext,
    state: &mut PresentationState,
    dirty: DirtyArea,
    source: &str,
) -> StrictFastOutcome {
    yield_now().await;
    let bus_quiet_started_ms = Instant::now().as_millis();
    let clients_quiet = panel_bus::suspend_clients().await;
    let bus_quiet_wait_ms = Instant::now()
        .as_millis()
        .saturating_sub(bus_quiet_started_ms);

    if !clients_quiet {
        context.inkplate.isolate_panel_for_cleanup();
        panel_bus::resume_clients(false).await;
        track_panel_cleanup(context, state);
        note_refresh_failure(state, Instant::now().as_millis());
        console::println!(
            "LVGL_REFRESH phase=ambient_fast source={} status=error error=clients_not_quiesced recovery=full_next dirty={},{},{},{} bus_quiet_wait_ms={}",
            source, dirty.x1, dirty.y1, dirty.x2, dirty.y2, bus_quiet_wait_ms,
        );
        return StrictFastOutcome::Failed;
    }

    // The strict transaction never falls back to a full refresh (it errors
    // instead, see `display_bw_partial_gate_drain_strict_no_cleanup_
    // cooperative_async`'s doc), so the parked-panel-needs-shutdown case
    // `refresh_panel` handles for the fallback-capable path cannot arise
    // here: a successful result is always a partial or no-change lease kind.
    let hold_terminal_state = state
        .panel_power_lease
        .policy()
        .should_hold_terminal_state_for_partial();
    let result = context
        .inkplate
        .display_bw_partial_gate_drain_strict_no_cleanup_cooperative_async(
            hold_terminal_state,
            panel_bus::open_touch_waveform_window,
            panel_bus::close_touch_waveform_window,
        )
        .await;
    let clients_resumed = panel_bus::resume_clients(false).await;
    if !clients_resumed || result.as_ref().is_ok_and(|timing| timing.full_fallback) {
        context.inkplate.isolate_panel_for_cleanup();
        track_panel_cleanup(context, state);
        note_refresh_failure(state, Instant::now().as_millis());
        console::println!(
            "LVGL_REFRESH phase=ambient_fast source={} status=error error={} recovery=full_next dirty={},{},{},{} bus_quiet_wait_ms={}",
            source,
            if clients_resumed {
                "strict_full_fallback"
            } else {
                "clients_not_resumed"
            },
            dirty.x1,
            dirty.y1,
            dirty.x2,
            dirty.y2,
            bus_quiet_wait_ms,
        );
        return StrictFastOutcome::Failed;
    }

    match result {
        Ok(timing) => {
            let completed = if timing.scan_rows == 0 {
                CompletedRefresh::NoChange
            } else {
                CompletedRefresh::Partial
            };
            state.record_refresh_success(completed, Instant::now().as_millis());
            state
                .panel_power_lease
                .record_refresh_success(lease_refresh_kind(completed), Instant::now().as_millis());
            if cfg!(feature = "firmware-trace") {
                console::println!(
                "LVGL_REFRESH phase=ambient_fast source={} status=ok kind={:?} dirty={},{},{},{} terminal_hold_requested={} bus_quiet_wait_ms={} waveform_us={}",
                source,
                completed,
                dirty.x1,
                dirty.y1,
                dirty.x2,
                dirty.y2,
                hold_terminal_state,
                bus_quiet_wait_ms,
                timing.waveform_us(),
            );
            }
            StrictFastOutcome::Pushed
        }
        Err(PartialGateDrainError::NotReady) => {
            track_panel_cleanup(context, state);
            note_refresh_failure(state, Instant::now().as_millis());
            console::println!(
                "LVGL_REFRESH phase=ambient_fast source={} status=not_ready recovery=full_next dirty={},{},{},{} bus_quiet_wait_ms={}",
                source, dirty.x1, dirty.y1, dirty.x2, dirty.y2, bus_quiet_wait_ms,
            );
            StrictFastOutcome::NotReady
        }
        Err(PartialGateDrainError::Driver(error)) => {
            track_panel_cleanup(context, state);
            note_refresh_failure(state, Instant::now().as_millis());
            console::println!(
                "LVGL_REFRESH phase=ambient_fast source={} status=error error={:?} recovery=full_next dirty={},{},{},{} bus_quiet_wait_ms={}",
                source, error, dirty.x1, dirty.y1, dirty.x2, dirty.y2, bus_quiet_wait_ms,
            );
            StrictFastOutcome::Failed
        }
    }
}

/// Full refresh driving the panel's eight physical grey levels from
/// `context.gray4_framebuffer`, instead of the binary framebuffer the LVGL
/// path fills.
///
/// This is the direct-panel seam for product apps and targets outside the
/// display module: call it to push Gray4 content straight to the panel.
///
/// Same bus choreography as [`full_refresh_panel`] -- clients are suspended
/// for the scan and resumed afterwards -- because the constraint is the shared
/// bus, not the waveform. Two differences worth knowing:
///
/// - a grayscale waveform never establishes a valid source image for the
///   binary partial-transition engine, so the driver clears `partial_ready`
///   and the next binary update is a full refresh;
/// - it is a no-op returning `false` when no Gray4 framebuffer was allocated.
pub async fn full_refresh_panel_gray4(
    context: &mut DisplayContext,
    reset_touch_pipeline: bool,
) -> bool {
    let Some(framebuffer) = context.gray4_framebuffer.as_deref() else {
        console::println!("LVGL_GRAY4_REFRESH status=error error=no_framebuffer");
        return false;
    };
    let clients_quiet = panel_bus::suspend_clients().await;
    let succeeded = clients_quiet
        && match context
            .inkplate
            .display_gray4_async(framebuffer, false)
            .await
        {
            Ok(()) => true,
            Err(error) => {
                console::println!("LVGL_GRAY4_REFRESH status=error error={:?}", error);
                false
            }
        };
    if !clients_quiet {
        context.inkplate.isolate_panel_for_cleanup();
        console::println!("LVGL_GRAY4_REFRESH status=error error=clients_not_quiesced");
    }
    let resumed = panel_bus::resume_clients(reset_touch_pipeline).await;
    succeeded && resumed
}

pub(in crate::firmware::display) async fn full_refresh_panel(
    context: &mut DisplayContext,
    reset_touch_pipeline: bool,
) -> bool {
    let (scan_succeeded, resumed) =
        full_refresh_panel_traced(context, reset_touch_pipeline, None).await;
    scan_succeeded && resumed
}

async fn full_refresh_panel_traced(
    context: &mut DisplayContext,
    reset_touch_pipeline: bool,
    request_id: Option<u64>,
) -> (bool, bool) {
    // The same executor handoff `refresh_panel` keeps between its LVGL flush
    // and its scan: the invalidate() render the caller just ran must have
    // fully landed before clients suspend and the interrupt-locked full scan
    // starts.
    yield_now().await;
    let clients_quiet = panel_bus::suspend_clients().await;
    if let Some(id) = request_id {
        super::observation_trace::control(id, "suspend", clients_quiet);
    }
    // A client's cleanup could not confirm the shared bus quiescent -- skip
    // the scan entirely rather than proceed anyway (see `refresh_panel`'s
    // matching comment).
    let succeeded = clients_quiet
        && match context.inkplate.display_bw_async(false).await {
            Ok(()) => true,
            Err(error) => {
                console::println!("LVGL_FULL_REFRESH status=error error={:?}", error);
                false
            }
        };
    if !clients_quiet {
        context.inkplate.isolate_panel_for_cleanup();
        console::println!("LVGL_FULL_REFRESH status=error error=clients_not_quiesced");
    }
    let resumed = panel_bus::resume_clients(reset_touch_pipeline).await;
    if let Some(id) = request_id {
        super::observation_trace::control(id, "resume", resumed);
    }
    (succeeded, resumed)
}
