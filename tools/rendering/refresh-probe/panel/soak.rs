//! Soak workloads: one deterministic scenario per probe mode.
//!
//! The embassy task boots the panel, emits the `event=ready` line describing
//! the compiled-in candidate, runs a baseline refresh, then dispatches to the
//! selected soak or benchmark loop and ends with `event=complete`.

use embassy_time::{Instant, Timer};
use inkplate_tempera::{
    adapters::BusyDelay, InkplateHal, TestPattern, PANEL_FULL_CLEAN_HARDWARE_LOOP,
    PANEL_FULL_CLEAN_INLINE_HOLD_CYCLES, PANEL_FULL_CLEAN_REFERENCE_EDGE_SEQUENCE,
    PANEL_FULL_CL_HIGH_HOLD_CYCLES, PANEL_FULL_FIXED_HOLD_NOPS, PANEL_FULL_FIXED_HOLD_SELECTED,
    PANEL_FULL_HARDWARE_LOOP, PANEL_FULL_INTER_PASS_DELAY_US, PANEL_FULL_OPTIMIZED_CLEAN_LOOP,
    PANEL_FULL_PIPELINED_HOLD, PANEL_FULL_REFERENCE_ROW_BOUNDARY, PANEL_FULL_REFERENCE_SEQUENCE,
    PANEL_FULL_ROW_START_REFERENCE_SEQUENCE, PANEL_FULL_ROW_START_REGISTER_CCOUNT,
    PANEL_FULL_SCAN_PHASE_TIMING, PANEL_FULL_SOURCE_FIXED_HOLD_NOPS,
    PANEL_FULL_SOURCE_FIXED_HOLD_SELECTED, PANEL_PARTIAL_BOUNDED_TRANSITION_PREP,
    PANEL_PARTIAL_CL_HIGH_HOLD_CYCLES, PANEL_PARTIAL_FIXED_HOLD_NOPS, PANEL_PARTIAL_HARDWARE_LOOP,
    PANEL_PARTIAL_HOISTED_GPIO_LOOP, PANEL_PARTIAL_REFERENCE_ROW_BOUNDARY,
    PANEL_PARTIAL_REFERENCE_SEQUENCE, PANEL_PARTIAL_SCAN_PHASE_TIMING,
};
use meditamer_product::firmware::psram::ExternalValue;

use super::{
    draw::{draw_partial_span_pattern, draw_pattern, validate_sparse_center_counter_delta},
    gray_pattern::{prepare_gray3_framebuffer, prepare_numbered_gray3_pattern},
    marker::{alternating_pattern, visible_marker},
    probe_mode::{
        benchmark_name, probe_purpose, should_emit_cycle, ProbeMode, COMPACT_LOG,
        INSPECTION_CHECKPOINT_EVERY, INSPECTION_HOLD_MS, PANEL_I2C_KHZ, PROBE_MODE,
        REFRESH_INTERVAL_MS, RUN_CYCLES,
    },
    refresh::{run_full, run_gray3_full, run_partial, Gray3FullSample},
    resources::{halt, ProbeI2cDevice, SoakResources},
};
use crate::probe_config;

const FULL_CLEAN_SOURCE_DRIVER: &str = "gpio";

#[embassy_executor::task]
pub(crate) async fn panel_soak_task(mut resources: ExternalValue<SoakResources>) {
    let driver = &mut resources.inkplate;
    if let Err(error) = driver.init_core().await {
        console::println!("PANEL_SOAK event=halt stage=init_core error={:?}", error);
        halt()
    }
    let _ = driver.set_wakeup(true).await;
    let _ = driver.frontlight_off().await;

    console::println!(
        "PANEL_SOAK event=ready mode={:?} purpose={} benchmark={} cycles={} refresh_end_to_start_interval_ms={} panel_i2c_khz={} checkpoint_every={} checkpoint_hold_ms={} log_mode={} partial_timing={} partial_cl_high_hold_cycles={} partial_fixed_hold_nops={} partial_row_boundary={} partial_scan_loop={} partial_scan_phase_timing={} partial_transition_prep={} full_timing={} full_cl_high_hold_cycles={} full_fixed_hold_nops={} full_row_boundary={} full_row_start_hold={} full_scan_loop={} full_clean_edge_sequence={} full_framebuffer_loop={} full_clean_inline_hold_cycles={} full_scan_phase_timing={} full_inter_pass_delay_us={} full_clean_source_driver={} visual=unverified",
        PROBE_MODE,
        probe_purpose(PROBE_MODE),
        benchmark_name(PROBE_MODE),
        RUN_CYCLES,
        REFRESH_INTERVAL_MS,
        PANEL_I2C_KHZ,
        INSPECTION_CHECKPOINT_EVERY,
        INSPECTION_HOLD_MS,
        if COMPACT_LOG { "compact" } else { "full" },
        if PANEL_PARTIAL_FIXED_HOLD_NOPS != 0 {
            "fixed_nops"
        } else if PANEL_PARTIAL_REFERENCE_SEQUENCE {
            "official_ordered_set_clear"
        } else {
            "ccount_hold"
        },
        PANEL_PARTIAL_CL_HIGH_HOLD_CYCLES,
        PANEL_PARTIAL_FIXED_HOLD_NOPS,
        if PANEL_PARTIAL_REFERENCE_ROW_BOUNDARY {
            "official_ordered_ckv_le"
        } else {
            "margin_1us"
        },
        if PANEL_PARTIAL_HARDWARE_LOOP {
            "hardware_loop"
        } else if PANEL_PARTIAL_HOISTED_GPIO_LOOP {
            "hoisted_gpio"
        } else {
            "baseline"
        },
        PANEL_PARTIAL_SCAN_PHASE_TIMING,
        if PANEL_PARTIAL_BOUNDED_TRANSITION_PREP {
            "bounded"
        } else {
            "current"
        },
        if PANEL_FULL_FIXED_HOLD_SELECTED {
            "fixed_nops"
        } else if PANEL_FULL_REFERENCE_SEQUENCE {
            "official_ordered_set_clear"
        } else {
            "ccount_hold"
        },
        PANEL_FULL_CL_HIGH_HOLD_CYCLES,
        PANEL_FULL_FIXED_HOLD_NOPS,
        if PANEL_FULL_REFERENCE_ROW_BOUNDARY {
            "official_ordered_ckv_le"
        } else {
            "margin_1us"
        },
        if PANEL_FULL_ROW_START_REFERENCE_SEQUENCE {
            "official_ordered_set_clear"
        } else if PANEL_FULL_ROW_START_REGISTER_CCOUNT {
            "register_ccount"
        } else {
            "default"
        },
        if PANEL_FULL_CLEAN_HARDWARE_LOOP {
            "reference_hardware_loop_74x2"
        } else if PANEL_FULL_OPTIMIZED_CLEAN_LOOP {
            "optimized_clean_gpio_inline_hold"
        } else {
            "legacy_clean"
        },
        if PANEL_FULL_CLEAN_REFERENCE_EDGE_SEQUENCE {
            "official_ordered_set_clear"
        } else {
            "ccount_hold"
        },
        if PANEL_FULL_PIPELINED_HOLD {
            "hardware_loop_pipelined_ccount"
        } else if PANEL_FULL_SOURCE_FIXED_HOLD_SELECTED {
            match PANEL_FULL_SOURCE_FIXED_HOLD_NOPS {
                0 => "hardware_loop_fixed_0_nops",
                1 => "hardware_loop_fixed_1_nops",
                2 => "hardware_loop_fixed_2_nops",
                3 => "hardware_loop_fixed_3_nops",
                4 => "hardware_loop_fixed_4_nops",
                5 => "hardware_loop_fixed_5_nops",
                6 => "hardware_loop_fixed_6_nops",
                7 => "hardware_loop_fixed_7_nops",
                8 => "hardware_loop_fixed_8_nops",
                9 => "hardware_loop_fixed_9_nops",
                10 => "hardware_loop_fixed_10_nops",
                11 => "hardware_loop_fixed_11_nops",
                12 => "hardware_loop_fixed_12_nops",
                13 => "hardware_loop_fixed_13_nops",
                14 => "hardware_loop_fixed_14_nops",
                15 => "hardware_loop_fixed_15_nops",
                16 => "hardware_loop_fixed_16_nops",
                _ => "hardware_loop_fixed_invalid",
            }
        } else if PANEL_FULL_HARDWARE_LOOP {
            "hardware_loop_inline_ccount"
        } else {
            "baseline"
        },
        PANEL_FULL_CLEAN_INLINE_HOLD_CYCLES,
        PANEL_FULL_SCAN_PHASE_TIMING,
        PANEL_FULL_INTER_PASS_DELAY_US,
        FULL_CLEAN_SOURCE_DRIVER,
    );

    let mut gray_framebuffer = if matches!(
        PROBE_MODE,
        ProbeMode::FullPerformanceGray3 | ProbeMode::FullProfileGray3
    ) {
        Some(prepare_gray3_framebuffer())
    } else {
        None
    };

    if let Some(framebuffer) = gray_framebuffer.as_deref_mut() {
        let pattern_hash = prepare_numbered_gray3_pattern(framebuffer, "baseline", 0, 0);
        run_gray3_full(
            driver,
            framebuffer,
            Gray3FullSample {
                sample: 0,
                marker: 0,
                pattern_hash,
                phase: "baseline",
                start_interval_us: 0,
                profile: false,
            },
        )
        .await;
    } else {
        if PROBE_MODE == ProbeMode::PartialSpans {
            driver.clear_bw();
        } else if matches!(
            PROBE_MODE,
            ProbeMode::PartialPerformance | ProbeMode::PartialProfile
        ) {
            draw_pattern(driver, TestPattern::HorizontalBars, 0);
        } else {
            draw_pattern(driver, TestPattern::VerticalBars, 0);
        }
        run_full(driver, 0, 0, "baseline", false, 0).await;
    }
    let baseline_finished = Instant::now();

    match PROBE_MODE {
        ProbeMode::Partial => soak_partial(driver).await,
        ProbeMode::PartialPerformance | ProbeMode::PartialProfile => {
            benchmark_partial(driver, PROBE_MODE, baseline_finished).await
        }
        ProbeMode::PartialSpans => soak_partial_spans(driver).await,
        ProbeMode::FullPerformanceBinary
        | ProbeMode::FullPerformanceGray3
        | ProbeMode::FullProfileBinary
        | ProbeMode::FullProfileGray3 => {
            benchmark_full(driver, PROBE_MODE, gray_framebuffer, baseline_finished).await
        }
        ProbeMode::FullSoak => soak_full(driver).await,
        ProbeMode::PartialThenFull => soak_partial_then_full(driver).await,
        ProbeMode::Ghosting => soak_ghosting(driver).await,
    }

    console::println!(
        "PANEL_SOAK event=complete mode={:?} purpose={} benchmark={} cycles={} protocol=passed visual=unverified",
        PROBE_MODE,
        probe_purpose(PROBE_MODE),
        benchmark_name(PROBE_MODE),
        RUN_CYCLES
    );
    halt()
}

async fn soak_partial(driver: &mut InkplateHal<ProbeI2cDevice, BusyDelay>) {
    if let Err(error) = driver.eink_on_async().await {
        console::println!(
            "PANEL_SOAK event=halt mode=Partial stage=warm_partial_setup error={:?}",
            error
        );
        halt()
    }
    console::println!("PANEL_SOAK event=power phase=warm_partial_setup panel_power=held status=ok");

    let mut previous_refresh_finished = Instant::now();
    for cycle in 1..=RUN_CYCLES {
        let marker = visible_marker(cycle);
        draw_pattern(driver, alternating_pattern(cycle), marker);
        let start_interval_us = wait_for_refresh_interval(previous_refresh_finished).await;
        run_partial(
            driver,
            cycle,
            marker,
            "partial",
            true,
            None,
            start_interval_us,
        )
        .await;
        previous_refresh_finished = Instant::now();
        if should_emit_cycle(cycle) {
            console::println!(
                "PANEL_SOAK event=cycle_complete mode=Partial cycle={} panel_power=held next_interval_ms={} protocol=passed visual=unverified",
                cycle,
                REFRESH_INTERVAL_MS
            );
        }
    }
    if let Err(error) = driver.eink_off_async().await {
        console::println!(
            "PANEL_SOAK event=halt mode=Partial cycle={} stage=final_power_off error={:?}",
            RUN_CYCLES,
            error
        );
        halt()
    }
}

async fn benchmark_partial(
    driver: &mut InkplateHal<ProbeI2cDevice, BusyDelay>,
    mode: ProbeMode,
    baseline_finished: Instant,
) {
    if let Err(error) = driver.eink_on_async().await {
        console::println!(
            "PANEL_SOAK event=halt mode={:?} stage=warm_partial_setup error={:?}",
            mode,
            error
        );
        halt()
    }
    console::println!("PANEL_SOAK event=power phase=warm_partial_setup panel_power=held status=ok");

    draw_pattern(driver, TestPattern::VerticalBars, 0);
    let baseline_interval_us = wait_for_refresh_interval(baseline_finished).await;
    run_partial(driver, 0, 0, "baseline", true, None, baseline_interval_us).await;

    let (purpose, phase, instrumentation) = if mode == ProbeMode::PartialProfile {
        ("phase_profile", "partial_1bit_profile", "phase_timestamps")
    } else {
        ("performance", "partial_1bit_performance", "none")
    };
    console::println!(
        "PANEL_SOAK event=benchmark_start benchmark=partial_1bit purpose={} samples={} warmup_samples=1 power_state=warm_held_on refresh_end_to_start_interval_ms={} draw_timed=0 api=display_bw_partial_gate_drain_strict_no_cleanup_cooperative_async instrumentation={}",
        purpose,
        RUN_CYCLES,
        REFRESH_INTERVAL_MS,
        instrumentation,
    );

    let mut previous_refresh_finished = Instant::now();
    for sample in 1..=RUN_CYCLES {
        let marker = visible_marker(sample);
        draw_pattern(driver, alternating_pattern(sample), marker);
        let start_interval_us = wait_for_refresh_interval(previous_refresh_finished).await;
        run_partial(driver, sample, marker, phase, true, None, start_interval_us).await;
        previous_refresh_finished = Instant::now();
    }

    if let Err(error) = driver.eink_off_async().await {
        console::println!(
            "PANEL_SOAK event=halt mode={:?} stage=partial_final_power_off error={:?}",
            mode,
            error
        );
        halt()
    }
}

async fn soak_partial_spans(driver: &mut InkplateHal<ProbeI2cDevice, BusyDelay>) {
    if let Err(error) = driver.eink_on_async().await {
        console::println!(
            "PANEL_SOAK event=halt mode=PartialSpans stage=warm_partial_setup error={:?}",
            error
        );
        halt()
    }
    console::println!("PANEL_SOAK event=power phase=warm_partial_setup panel_power=held status=ok");

    let mut previous_refresh_finished = Instant::now();
    for (span_index, scan_rows) in probe_config::PARTIAL_SCAN_ROWS.into_iter().enumerate() {
        console::println!(
            "PANEL_SOAK event=workload_start mode=PartialSpans scan_rows={} samples={} panel_power=held visual=unverified",
            scan_rows,
            RUN_CYCLES,
        );
        for sample in 1..=RUN_CYCLES {
            let marker = visible_marker((span_index as u32 + 1) * 100 + sample % 100);
            draw_partial_span_pattern(driver, scan_rows, sample, marker);
            let start_interval_us = wait_for_refresh_interval(previous_refresh_finished).await;
            run_partial(
                driver,
                sample,
                marker,
                "partial_span",
                true,
                Some(scan_rows),
                start_interval_us,
            )
            .await;
            previous_refresh_finished = Instant::now();
            if should_emit_cycle(sample) {
                console::println!(
                    "PANEL_SOAK event=cycle_complete mode=PartialSpans scan_rows={} sample={} marker={:03} panel_power=held next_interval_ms={} protocol=passed visual=unverified",
                    scan_rows,
                    sample,
                    marker,
                    REFRESH_INTERVAL_MS,
                );
            }
        }
        console::println!(
            "PANEL_SOAK event=workload_complete mode=PartialSpans scan_rows={} samples={} protocol=passed visual=unverified",
            scan_rows,
            RUN_CYCLES,
        );
    }

    if let Err(error) = driver.eink_off_async().await {
        console::println!(
            "PANEL_SOAK event=halt mode=PartialSpans stage=final_power_off error={:?}",
            error
        );
        halt()
    }
}

async fn wait_for_refresh_interval(previous_refresh_finished: Instant) -> u64 {
    let target_us = REFRESH_INTERVAL_MS.saturating_mul(1_000);
    let elapsed_us = previous_refresh_finished.elapsed().as_micros();
    if elapsed_us < target_us {
        Timer::after_micros(target_us - elapsed_us).await;
    }
    previous_refresh_finished.elapsed().as_micros()
}

async fn hold_for_inspection_checkpoint(cycle: u32, phase: &str) {
    if INSPECTION_CHECKPOINT_EVERY == 0
        || !cycle.is_multiple_of(INSPECTION_CHECKPOINT_EVERY)
        || cycle == RUN_CYCLES
    {
        return;
    }
    console::println!(
        "PANEL_SOAK event=inspection_checkpoint cycle={} phase={} hold_ms={} visual=unverified",
        cycle,
        phase,
        INSPECTION_HOLD_MS
    );
    Timer::after_millis(INSPECTION_HOLD_MS).await;
}

async fn benchmark_full(
    driver: &mut InkplateHal<ProbeI2cDevice, BusyDelay>,
    mode: ProbeMode,
    mut gray_framebuffer: Option<&mut [u8]>,
    baseline_finished: Instant,
) {
    match mode {
        ProbeMode::FullPerformanceBinary => console::println!(
            "PANEL_SOAK event=benchmark_start benchmark=full_1bit purpose=performance samples={} warmup_samples=1 power_state=cold_per_sample refresh_end_to_start_interval_ms={} draw_timed=0 api=display_bw_async_false instrumentation=none",
            RUN_CYCLES,
            REFRESH_INTERVAL_MS,
        ),
        ProbeMode::FullPerformanceGray3 => console::println!(
            "PANEL_SOAK event=benchmark_start benchmark=full_3bit purpose=performance samples={} warmup_samples=1 power_state=cold_per_sample refresh_end_to_start_interval_ms={} draw_timed=0 api=display_gray4_async_false fixture=levels_ramp_boundaries_numbered marker_scheme=mod1000 instrumentation=none",
            RUN_CYCLES,
            REFRESH_INTERVAL_MS,
        ),
        ProbeMode::FullProfileBinary => console::println!(
            "PANEL_SOAK event=benchmark_start benchmark=full_1bit purpose=phase_profile samples={} warmup_samples=1 power_state=cold_per_sample refresh_end_to_start_interval_ms={} draw_timed=0 api=display_bw_timed_async_false instrumentation=phase_timestamps",
            RUN_CYCLES,
            REFRESH_INTERVAL_MS,
        ),
        ProbeMode::FullProfileGray3 => console::println!(
            "PANEL_SOAK event=benchmark_start benchmark=full_3bit purpose=phase_profile samples={} warmup_samples=1 power_state=cold_per_sample refresh_end_to_start_interval_ms={} draw_timed=0 api=display_gray4_timed_async_false fixture=levels_ramp_boundaries_numbered marker_scheme=mod1000 instrumentation=phase_timestamps",
            RUN_CYCLES,
            REFRESH_INTERVAL_MS,
        ),
        _ => unreachable!("full performance benchmark requires a full mode"),
    }

    let mut previous_refresh_finished = baseline_finished;
    for sample in 1..=RUN_CYCLES {
        match mode {
            ProbeMode::FullPerformanceBinary | ProbeMode::FullProfileBinary => {
                let marker = visible_marker(sample);
                draw_pattern(driver, alternating_pattern(sample), marker);
                let start_interval_us = wait_for_refresh_interval(previous_refresh_finished).await;
                run_full(
                    driver,
                    sample,
                    marker,
                    if mode == ProbeMode::FullProfileBinary {
                        "full_1bit_profile"
                    } else {
                        "full_1bit_performance"
                    },
                    false,
                    start_interval_us,
                )
                .await;
            }
            ProbeMode::FullPerformanceGray3 | ProbeMode::FullProfileGray3 => {
                let marker = visible_marker(sample);
                let framebuffer = gray_framebuffer
                    .as_deref_mut()
                    .expect("gray benchmark buffer");
                let phase = if mode == ProbeMode::FullProfileGray3 {
                    "full_3bit_profile"
                } else {
                    "full_3bit_performance"
                };
                let pattern_hash =
                    prepare_numbered_gray3_pattern(framebuffer, phase, sample, marker);
                let start_interval_us = wait_for_refresh_interval(previous_refresh_finished).await;
                run_gray3_full(
                    driver,
                    framebuffer,
                    Gray3FullSample {
                        sample,
                        marker,
                        pattern_hash,
                        phase,
                        start_interval_us,
                        profile: mode == ProbeMode::FullProfileGray3,
                    },
                )
                .await;
            }
            _ => unreachable!("full performance benchmark requires a full mode"),
        }
        previous_refresh_finished = Instant::now();
    }
}

async fn soak_full(driver: &mut InkplateHal<ProbeI2cDevice, BusyDelay>) {
    let mut previous_refresh_finished = Instant::now();
    for cycle in 1..=RUN_CYCLES {
        let marker = visible_marker(cycle);
        draw_pattern(driver, alternating_pattern(cycle), marker);
        let start_interval_us = wait_for_refresh_interval(previous_refresh_finished).await;
        run_full(driver, cycle, marker, "full_soak", false, start_interval_us).await;
        previous_refresh_finished = Instant::now();
        console::println!(
            "PANEL_SOAK event=cycle_complete mode=Full cycle={} next_interval_ms={} protocol=passed visual=unverified",
            cycle,
            REFRESH_INTERVAL_MS
        );
        hold_for_inspection_checkpoint(cycle, "full").await;
    }
}

async fn soak_partial_then_full(driver: &mut InkplateHal<ProbeI2cDevice, BusyDelay>) {
    console::println!(
        "PANEL_SOAK event=workload_start mode=PartialThenFull fixture=sparse_center_counter base_pattern=vertical_bars samples={} panel_power=partial_held_full_off visual=unverified",
        RUN_CYCLES,
    );
    let mut previous_refresh_finished = Instant::now();
    for cycle in 1..=RUN_CYCLES {
        // Keep the visible number equal to the pair count. Counting individual
        // updates wraps a 500-pair soak from 999 to 000 and makes the final
        // screen indistinguishable from a restart. Redrawing the invariant
        // vertical-bar base changes only pixels inside the centered counter;
        // the debug snapshot below enforces that sparse-delta contract.
        let partial_marker = visible_marker(cycle);
        let full_marker = partial_marker;
        draw_pattern(driver, TestPattern::VerticalBars, partial_marker);
        validate_sparse_center_counter_delta(driver, cycle, partial_marker);
        let start_interval_us = wait_for_refresh_interval(previous_refresh_finished).await;
        run_partial(
            driver,
            cycle,
            partial_marker,
            "partial_predecessor",
            true,
            None,
            start_interval_us,
        )
        .await;
        previous_refresh_finished = Instant::now();
        // Reconstruct exactly the sparse partial's resulting framebuffer. No
        // second draw occurs here, so the full successor cannot silently widen
        // the logical update before exercising the candidate full waveform.
        let start_interval_us = wait_for_refresh_interval(previous_refresh_finished).await;
        run_full(
            driver,
            cycle,
            full_marker,
            "full_successor",
            false,
            start_interval_us,
        )
        .await;
        previous_refresh_finished = Instant::now();
        console::println!(
            "PANEL_SOAK event=cycle_complete mode=PartialThenFull cycle={} next_interval_ms={} protocol=passed visual=unverified",
            cycle,
            REFRESH_INTERVAL_MS
        );
        hold_for_inspection_checkpoint(cycle, "full_successor").await;
    }
}

async fn soak_ghosting(driver: &mut InkplateHal<ProbeI2cDevice, BusyDelay>) {
    let mut previous_refresh_finished = Instant::now();
    for cycle in 1..=RUN_CYCLES {
        let marker = visible_marker(cycle);
        draw_pattern(driver, alternating_pattern(cycle), marker);
        let start_interval_us = wait_for_refresh_interval(previous_refresh_finished).await;
        run_full(
            driver,
            cycle,
            marker,
            "ghosting_condition",
            false,
            start_interval_us,
        )
        .await;
        previous_refresh_finished = Instant::now();
        console::println!(
            "PANEL_SOAK event=cycle_complete mode=Ghosting cycle={} phase=ghosting_condition next_interval_ms={} protocol=passed visual=unverified",
            cycle,
            REFRESH_INTERVAL_MS
        );
    }

    let reveal_cycle = RUN_CYCLES.saturating_add(1);
    let reveal_marker = visible_marker(PANEL_FULL_INTER_PASS_DELAY_US);
    draw_pattern(driver, TestPattern::SolidWhite, reveal_marker);
    let start_interval_us = wait_for_refresh_interval(previous_refresh_finished).await;
    run_full(
        driver,
        reveal_cycle,
        reveal_marker,
        "ghosting_reveal",
        false,
        start_interval_us,
    )
    .await;
    console::println!(
        "PANEL_SOAK event=reveal_complete mode=Ghosting cycle={} marker={:03} protocol=passed visual=unverified",
        reveal_cycle,
        reveal_marker
    );
    console::println!(
        "PANEL_SOAK event=ghosting_observation settle_s=0 marker={:03} screen_updates=stopped visual=unverified",
        reveal_marker
    );

    let mut elapsed_s = 0u32;
    for wait_s in [5u32, 10, 15, 30] {
        Timer::after_secs(u64::from(wait_s)).await;
        elapsed_s += wait_s;
        console::println!(
            "PANEL_SOAK event=ghosting_observation settle_s={} marker={:03} screen_updates=stopped visual=unverified",
            elapsed_s,
            reveal_marker
        );
    }
}
