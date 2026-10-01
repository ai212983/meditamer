//! Single timed refresh transactions and their machine-readable reports.
//!
//! Each `run_*` helper performs exactly one panel update and logs one
//! `event=refresh` line with the waveform timing breakdown. Driver errors
//! halt with `event=halt` so a failed sample can never look like a fast one.

use core::cell::Cell;

use embassy_time::Instant;
use inkplate_tempera::{
    adapters::BusyDelay, InkplateHal, PartialGateDrainError, E_INK_HEIGHT,
    PANEL_FULL_SCAN_PHASE_TIMING,
};

use super::{
    probe_mode::{should_emit_cycle, ProbeMode, PROBE_MODE},
    resources::{halt, ProbeI2cDevice},
};

pub(crate) async fn run_partial(
    driver: &mut InkplateHal<ProbeI2cDevice, BusyDelay>,
    cycle: u32,
    marker: u32,
    phase: &str,
    leave_on: bool,
    expected_scan_rows: Option<usize>,
    start_interval_us: u64,
) {
    let started = Instant::now();
    let waveform_opened_us = Cell::new(0u64);
    let waveform_closed_us = Cell::new(0u64);
    let result = driver
        .display_bw_partial_gate_drain_strict_no_cleanup_cooperative_async(
            leave_on,
            || {
                waveform_opened_us.set(Instant::now().as_micros());
                async {}
            },
            || {
                waveform_closed_us.set(Instant::now().as_micros());
                async { true }
            },
        )
        .await;
    let finished = Instant::now();
    let elapsed = finished - started;
    match result {
        Ok(timing) => {
            if let Some(expected_scan_rows) = expected_scan_rows {
                let expected_first_changed_row = E_INK_HEIGHT - expected_scan_rows;
                if timing.scan_rows != expected_scan_rows
                    || timing.first_changed_row != expected_first_changed_row
                    || timing.last_changed_row != E_INK_HEIGHT - 1
                    || timing.changed_span_rows != expected_scan_rows
                    || timing.source_skip_candidate_rows != 0
                {
                    console::println!(
                        "PANEL_SOAK event=halt mode={:?} cycle={} phase={} stage=row_span_mismatch expected_scan_rows={} actual_scan_rows={} expected_changed_rows={}..{} actual_changed_rows={}..{} actual_changed_span_rows={} actual_source_skip_candidate_rows={}",
                        PROBE_MODE,
                        cycle,
                        phase,
                        expected_scan_rows,
                        timing.scan_rows,
                        expected_first_changed_row,
                        E_INK_HEIGHT - 1,
                        timing.first_changed_row,
                        timing.last_changed_row,
                        timing.changed_span_rows,
                        timing.source_skip_candidate_rows,
                    );
                    halt()
                }
            }
            let opened_us = waveform_opened_us.get();
            let closed_us = waveform_closed_us.get();
            let preparation_us = timing
                .row_discovery_us
                .saturating_add(timing.transition_prepare_us);
            let instrumented_total_us = preparation_us
                .saturating_add(timing.power_on_us)
                .saturating_add(timing.waveform_us())
                .saturating_add(timing.finalization_us)
                .saturating_add(timing.previous_copy_us);
            if should_emit_cycle(cycle) {
                if PROBE_MODE == ProbeMode::PartialPerformance {
                    console::println!(
                        "PANEL_SOAK event=refresh mode={:?} sample={} cycle={} marker={:03} phase={} kind=partial status=ok start_interval_us={} elapsed_us={} elapsed_ms={} leave_on={} visual=unverified",
                        PROBE_MODE,
                        cycle,
                        cycle,
                        marker,
                        phase,
                        start_interval_us,
                        elapsed.as_micros(),
                        elapsed.as_millis(),
                        leave_on,
                    )
                } else {
                    console::println!(
                        "PANEL_SOAK event=refresh mode={:?} sample={} cycle={} marker={:03} phase={} kind=partial status=ok start_interval_us={} elapsed_us={} elapsed_ms={} preparation_us={} row_discovery_us={} transition_prepare_us={} power_on_us={} waveform_us={} transition_scan_us={} cleanup_us={} vscan_start_us={} source_scan_us={} pass_delay_us={} finalization_us={} power_off_us=0 previous_copy_us={} instrumented_total_us={} pre_waveform_us={} waveform_window_us={} post_waveform_us={} scan_rows={} changed_rows={}..{} changed_span_rows={} source_skip_candidate_rows={} leave_on={} visual=unverified",
                        PROBE_MODE,
                        cycle,
                        cycle,
                        marker,
                        phase,
                        start_interval_us,
                        elapsed.as_micros(),
                        elapsed.as_millis(),
                        preparation_us,
                        timing.row_discovery_us,
                        timing.transition_prepare_us,
                        timing.power_on_us,
                        timing.waveform_us(),
                        timing.transition_us,
                        timing.cleanup_neutral_finalize_us,
                        timing.vscan_start_us,
                        timing.source_scan_us,
                        timing.pass_delay_us,
                        timing.finalization_us,
                        timing.previous_copy_us,
                        instrumented_total_us,
                        opened_us.saturating_sub(started.as_micros()),
                        closed_us.saturating_sub(opened_us),
                        finished.as_micros().saturating_sub(closed_us),
                        timing.scan_rows,
                        timing.first_changed_row,
                        timing.last_changed_row,
                        timing.changed_span_rows,
                        timing.source_skip_candidate_rows,
                        leave_on,
                    )
                }
            }
        }
        Err(PartialGateDrainError::NotReady) => {
            console::println!(
                "PANEL_SOAK event=halt mode={:?} cycle={} phase={} stage=partial_not_ready",
                PROBE_MODE,
                cycle,
                phase
            );
            halt()
        }
        Err(PartialGateDrainError::Driver(error)) => {
            console::println!(
                "PANEL_SOAK event=halt mode={:?} cycle={} phase={} stage=partial_driver error={:?}",
                PROBE_MODE,
                cycle,
                phase,
                error
            );
            halt()
        }
    }
}

pub(crate) async fn run_full(
    driver: &mut InkplateHal<ProbeI2cDevice, BusyDelay>,
    sample: u32,
    marker: u32,
    phase: &str,
    leave_on: bool,
    start_interval_us: u64,
) {
    let started = Instant::now();
    if PANEL_FULL_SCAN_PHASE_TIMING {
        let result = driver.display_bw_timed_async(leave_on).await;
        let elapsed = started.elapsed();
        match result {
            Ok(timing) => console::println!(
                "PANEL_SOAK event=refresh mode={:?} sample={} cycle={} marker={:03} phase={} kind=full status=ok start_interval_us={} elapsed_us={} elapsed_ms={} preparation_us={} power_on_us={} waveform_us={} initial_clean_us={} framebuffer_us={} settle_us={} final_clean_us={} terminal_vscan_us={} vscan_start_us={} source_scan_us={} pass_delay_us={} finalization_us={} previous_copy_us={} instrumented_total_us={} leave_on={} visual=unverified",
                PROBE_MODE,
                sample,
                sample,
                marker,
                phase,
                start_interval_us,
                elapsed.as_micros(),
                elapsed.as_millis(),
                timing.preparation_us,
                timing.power_on_us,
                timing.waveform_us(),
                timing.initial_clean_us,
                timing.framebuffer_us,
                timing.settle_us,
                timing.final_clean_us,
                timing.terminal_vscan_us,
                timing.vscan_start_us,
                timing.source_scan_us,
                timing.pass_delay_us,
                timing.finalization_us,
                timing.previous_copy_us,
                timing.total_us,
                leave_on,
            ),
            Err(error) => {
                console::println!(
                    "PANEL_SOAK event=halt mode={:?} sample={} phase={} stage=full_driver error={:?}",
                    PROBE_MODE,
                    sample,
                    phase,
                    error
                );
                halt()
            }
        }
        return;
    }
    let result = driver.display_bw_async(leave_on).await;
    let elapsed = started.elapsed();
    match result {
        Ok(()) => console::println!(
            "PANEL_SOAK event=refresh mode={:?} sample={} cycle={} marker={:03} phase={} kind=full status=ok start_interval_us={} elapsed_us={} elapsed_ms={} leave_on={} visual=unverified",
            PROBE_MODE,
            sample,
            sample,
            marker,
            phase,
            start_interval_us,
            elapsed.as_micros(),
            elapsed.as_millis(),
            leave_on
        ),
        Err(error) => {
            console::println!(
                "PANEL_SOAK event=halt mode={:?} sample={} phase={} stage=full_driver error={:?}",
                PROBE_MODE,
                sample,
                phase,
                error
            );
            halt()
        }
    }
}

pub(crate) struct Gray3FullSample<'a> {
    pub(crate) sample: u32,
    pub(crate) marker: u32,
    pub(crate) pattern_hash: u32,
    pub(crate) phase: &'a str,
    pub(crate) start_interval_us: u64,
    pub(crate) profile: bool,
}

pub(crate) async fn run_gray3_full(
    driver: &mut InkplateHal<ProbeI2cDevice, BusyDelay>,
    framebuffer: &[u8],
    refresh: Gray3FullSample<'_>,
) {
    let started = Instant::now();
    if refresh.profile {
        let result = driver.display_gray4_timed_async(framebuffer, false).await;
        let elapsed = started.elapsed();
        match result {
            Ok(timing) => console::println!(
                "PANEL_SOAK event=refresh mode={:?} sample={} cycle={} marker={:03} phase={} kind=full display_mode=3bit status=ok pattern=levels_ramp_boundaries_numbered pattern_hash=0x{:08x} start_interval_us={} elapsed_us={} elapsed_ms={} preparation_us={} power_on_us={} waveform_us={} initial_clean_us={} framebuffer_us={} settle_us={} final_clean_us={} terminal_vscan_us={} finalization_us={} previous_copy_us={} instrumented_total_us={} leave_on=false visual=unverified",
                PROBE_MODE,
                refresh.sample,
                refresh.sample,
                refresh.marker,
                refresh.phase,
                refresh.pattern_hash,
                refresh.start_interval_us,
                elapsed.as_micros(),
                elapsed.as_millis(),
                timing.preparation_us,
                timing.power_on_us,
                timing.waveform_us(),
                timing.initial_clean_us,
                timing.framebuffer_us,
                timing.settle_us,
                timing.final_clean_us,
                timing.terminal_vscan_us,
                timing.finalization_us,
                timing.previous_copy_us,
                timing.total_us,
            ),
            Err(error) => {
                console::println!(
                    "PANEL_SOAK event=halt mode={:?} sample={} phase={} stage=gray3_full_driver error={:?}",
                    PROBE_MODE,
                    refresh.sample,
                    refresh.phase,
                    error
                );
                halt()
            }
        }
        return;
    }
    let result = driver.display_gray4_async(framebuffer, false).await;
    let elapsed = started.elapsed();
    match result {
        Ok(()) => console::println!(
            "PANEL_SOAK event=refresh mode={:?} sample={} cycle={} marker={:03} phase={} kind=full display_mode=3bit status=ok pattern=levels_ramp_boundaries_numbered pattern_hash=0x{:08x} start_interval_us={} elapsed_us={} elapsed_ms={} leave_on=false visual=unverified",
            PROBE_MODE,
            refresh.sample,
            refresh.sample,
            refresh.marker,
            refresh.phase,
            refresh.pattern_hash,
            refresh.start_interval_us,
            elapsed.as_micros(),
            elapsed.as_millis(),
        ),
        Err(error) => {
            console::println!(
                "PANEL_SOAK event=halt mode={:?} sample={} phase={} stage=gray3_full_driver error={:?}",
                PROBE_MODE,
                refresh.sample,
                refresh.phase,
                error
            );
            halt()
        }
    }
}
