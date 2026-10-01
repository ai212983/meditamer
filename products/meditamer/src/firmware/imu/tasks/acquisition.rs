use embassy_futures::select::{select, select5, Either, Either5};
use embassy_time::Instant;

use crate::firmware::{
    config::SERIAL_STATUS_EVENTS,
    event_engine::{config::active_config, SensorFrame},
    touch::config::{TOUCH_IMU_ACTIVITY, TOUCH_IMU_QUIET_WINDOW_MS, TOUCH_IMU_STATUS},
    touch::types::TouchActivitySnapshot,
    types::{ImuSampleTimer, InkplateImuDriver, SerialStatusEvent},
};

use super::super::{
    config::{IMU_PIPELINE_INPUTS, IMU_SAMPLING_DEMAND},
    metrics,
    scheduler::{AdaptiveImuScheduler, SamplingMode},
    timing,
    types::{ImuFaultStage, ImuPipelineInput},
};
use super::acquisition_control::{handle_control_command, receive_command};

#[path = "acquisition_cycle.rs"]
mod acquisition_cycle;

use acquisition_cycle::{CycleState, SampleDurations, SuppressionPlan};

#[embassy_executor::task]
pub async fn imu_acquisition_task(mut imu: InkplateImuDriver, mut sample_timer: ImuSampleTimer) {
    let config = &active_config().imu_sampling;
    let mut scheduler = AdaptiveImuScheduler::new(config.idle_hz, config.active_hz);
    let mut state = CycleState::new(Instant::now());

    log_status(SerialStatusEvent::Scheduler {
        sensor_odr_hz: config.sensor_odr_hz,
        idle_hz: config.idle_hz,
        active_hz: config.active_hz,
        active_hold_ms: config.active_hold_ms,
    });

    loop {
        let remaining_us = state
            .next_sample_at
            .as_micros()
            .saturating_sub(Instant::now().as_micros());
        // An already-due sample must not arm a zero-duration hardware timer.
        // The timer is re-armed after an earlier select loses to a control signal.
        if remaining_us != 0 {
            match select5(
                sample_timer.delay_micros_async(remaining_us.min(u32::MAX as u64) as u32),
                IMU_SAMPLING_DEMAND.wait(),
                TOUCH_IMU_ACTIVITY.wait(),
                TOUCH_IMU_STATUS.wait(),
                receive_command(),
            )
            .await
            {
                Either5::First(_) => {}
                Either5::Second(demand) => {
                    state.on_demand(&mut scheduler, demand.active_until_ms);
                    continue;
                }
                Either5::Third(snapshot) => {
                    state.on_touch_snapshot(snapshot);
                    continue;
                }
                Either5::Fourth(status) => {
                    state.on_touch_status(status);
                    continue;
                }
                Either5::Fifth(command) => {
                    handle_control_command(command).await;
                    state.on_control_command(command);
                    continue;
                }
            }
        }

        let now = Instant::now();
        let mut rebase_deadline = state.pending_discontinuity;
        let now_ms = now.as_millis();
        state.note_missed_deadline(&scheduler, now, now_ms);
        let current_mode = state.refresh_mode(&scheduler, now_ms);

        match state.suppression_plan(now_ms) {
            SuppressionPlan::Hold => {
                state.hold_suppressed(&scheduler, now);
                continue;
            }
            SuppressionPlan::Proceed => {}
            SuppressionPlan::Suppress(reason) => {
                if !publish_input(
                    ImuPipelineInput::Suppressed { now_ms, reason },
                    &mut state.pending_discontinuity,
                )
                .await
                {
                    continue;
                }
                state.commit_suppressed(reason);
                state.hold_suppressed(&scheduler, now);
                continue;
            }
            SuppressionPlan::Resume => {
                if !publish_input(
                    ImuPipelineInput::Resumed { now_ms },
                    &mut state.pending_discontinuity,
                )
                .await
                {
                    continue;
                }
                state.commit_resumed(&mut scheduler, config.active_hold_ms, now_ms);
                timing::note_resumed();
            }
        }

        if state.init_due(now) {
            match imu.init(config.sensor_odr_hz).await {
                Ok(true) => {
                    state.commit_init_success(&mut scheduler, config.active_hold_ms, now_ms);
                    if !publish_input(
                        ImuPipelineInput::Recovered { now_ms },
                        &mut state.pending_discontinuity,
                    )
                    .await
                    {
                        continue;
                    }
                    metrics::record_recovery();
                    log_status(SerialStatusEvent::Ready);
                }
                Ok(false) | Err(_) => {
                    metrics::record_init_failure();
                    if state.should_report_init_fault() {
                        if !publish_input(
                            ImuPipelineInput::Fault {
                                now_ms,
                                stage: ImuFaultStage::Initialization,
                            },
                            &mut state.pending_discontinuity,
                        )
                        .await
                        {
                            continue;
                        }
                        state.commit_init_fault_reported();
                    }
                    state.defer_retry();
                    log_status(SerialStatusEvent::InitFailed);
                }
            }
        }

        if state.ready {
            let expected_period_us = scheduler.period_us(now_ms);
            let gap_discontinuity = state.gap_discontinuity(now, expected_period_us);
            let service_started = Instant::now();
            crate::firmware::types::i2c::reset_imu();
            match imu.read_latest().await {
                Ok(sample) => {
                    let bus_timing = crate::firmware::types::i2c::imu_snapshot();
                    // The PMIC value is diagnostic only. Never extend an active
                    // 8 ms sample with this extra shared-bus transaction, even
                    // if the active window expired during read_latest().
                    let aux_now = Instant::now();
                    let aux_us = if current_mode == SamplingMode::Idle
                        && scheduler.mode(aux_now.as_millis()) == SamplingMode::Idle
                        && state.aux_due(aux_now)
                    {
                        let aux_started = Instant::now();
                        let power_good = imu
                            .read_power_good()
                            .await
                            .ok()
                            .map(i16::from)
                            .unwrap_or(-1);
                        let elapsed = aux_started.elapsed().as_micros().min(u32::MAX as u64) as u32;
                        state.commit_aux(power_good);
                        elapsed
                    } else {
                        0
                    };
                    let service_us =
                        service_started.elapsed().as_micros().min(u32::MAX as u64) as u32;
                    let publish_started = Instant::now();
                    if !publish_input(
                        ImuPipelineInput::Sample {
                            frame: SensorFrame {
                                now_ms,
                                tap_src: sample.tap_src,
                                int1: sample.int1,
                                gx: sample.gx,
                                gy: sample.gy,
                                gz: sample.gz,
                                ax: sample.ax,
                                ay: sample.ay,
                                az: sample.az,
                            },
                            int2: sample.int2,
                            power_good: state.power_good_for_trace(Instant::now()),
                            discontinuity: state.sample_discontinuity(gap_discontinuity),
                        },
                        &mut state.pending_discontinuity,
                    )
                    .await
                    {
                        state.next_sample_at = Instant::now();
                        continue;
                    }
                    let publish_us =
                        publish_started.elapsed().as_micros().min(u32::MAX as u64) as u32;
                    state.commit_sample(
                        &scheduler,
                        now,
                        now_ms,
                        SampleDurations {
                            service_us,
                            publish_us,
                            read_timing: sample.read_timing,
                            bus_timing,
                            aux_us,
                        },
                        gap_discontinuity,
                        &mut rebase_deadline,
                    );
                }
                Err(_) => {
                    metrics::record_sample_failure();
                    state.commit_sample_failure();
                    if !publish_input(
                        ImuPipelineInput::Fault {
                            now_ms,
                            stage: ImuFaultStage::Sampling,
                        },
                        &mut state.pending_discontinuity,
                    )
                    .await
                    {
                        continue;
                    }
                    log_status(SerialStatusEvent::ReadError);
                }
            }
        }

        state.finish_cycle(
            &mut scheduler,
            current_mode,
            state.cycle_rebase(rebase_deadline),
        );
    }
}

fn log_status(event: SerialStatusEvent) {
    let _ = SERIAL_STATUS_EVENTS.try_send(event);
}

fn touch_bus_quiet(snapshot: TouchActivitySnapshot, now_ms: u64) -> bool {
    snapshot.active
        || snapshot
            .last_nonzero_ms
            .is_some_and(|last| now_ms.saturating_sub(last) <= TOUCH_IMU_QUIET_WINDOW_MS)
}

fn promote_scheduler(
    scheduler: &mut AdaptiveImuScheduler,
    last_mode: &mut SamplingMode,
    active_until_ms: u64,
) {
    let now_ms = Instant::now().as_millis();
    let before = scheduler.mode(now_ms);
    scheduler.promote_until(active_until_ms);
    let after = scheduler.mode(now_ms);
    metrics::record_mode_change(before, after);
    *last_mode = after;
}

// Publication cannot prevent a full-queue producer from acknowledging control.
// Drop only a sample crossing a suspension boundary; state transitions retain order.
async fn publish_input(input: ImuPipelineInput, discontinuity: &mut bool) -> bool {
    loop {
        match select(receive_command(), IMU_PIPELINE_INPUTS.send(input)).await {
            Either::First(command) => {
                handle_control_command(command).await;
                *discontinuity = true;
                if matches!(input, ImuPipelineInput::Sample { .. }) {
                    return false;
                }
            }
            Either::Second(()) => return true,
        }
    }
}
