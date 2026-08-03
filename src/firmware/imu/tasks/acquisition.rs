use embassy_futures::select::{select5, Either5};
use embassy_time::{Duration, Instant, Timer};

use crate::firmware::{
    app_state::read_app_state_snapshot,
    config::SERIAL_STATUS_EVENTS,
    event_engine::{config::active_config, SensorFrame},
    touch::config::{TOUCH_IMU_ACTIVITY, TOUCH_IMU_QUIET_WINDOW_MS, TOUCH_IMU_STATUS},
    touch::types::{TouchActivitySnapshot, TouchStatus},
    types::{InkplateImuDriver, SerialStatusEvent},
};

use super::super::{
    config::{
        IMU_INIT_RETRY_MS, IMU_PIPELINE_INPUTS, IMU_SAMPLING_DEMAND, TAP_TRACE_AUX_SAMPLE_MS,
        TAP_TRACE_ENABLED,
    },
    metrics,
    scheduler::{AdaptiveImuScheduler, SamplingMode},
    types::{ImuFaultStage, ImuPipelineInput, ImuSuppressionReason},
};
use super::acquisition_control::{handle_control_command, receive_command};

#[embassy_executor::task]
pub(crate) async fn imu_acquisition_task(mut imu: InkplateImuDriver) {
    let config = &active_config().imu_sampling;
    let mut scheduler = AdaptiveImuScheduler::new(config.idle_hz, config.active_hz);
    let mut last_mode = SamplingMode::Idle;
    let mut touch = TouchActivitySnapshot::default();
    let mut touch_initializing = true;
    let mut suppression = None;
    let mut ready = false;
    let mut fault_notified = false;
    let mut retry_at = Instant::now();
    let mut next_sample_at = Instant::now();
    let mut last_sample_at: Option<Instant> = None;
    let mut pending_discontinuity = true;
    let mut power_good = -1i16;
    let mut next_aux_at = Instant::now();

    log_status(SerialStatusEvent::Scheduler {
        sensor_odr_hz: config.sensor_odr_hz,
        idle_hz: config.idle_hz,
        active_hz: config.active_hz,
        active_hold_ms: config.active_hold_ms,
    });

    loop {
        match select5(
            Timer::at(next_sample_at),
            IMU_SAMPLING_DEMAND.wait(),
            TOUCH_IMU_ACTIVITY.wait(),
            TOUCH_IMU_STATUS.wait(),
            receive_command(),
        )
        .await
        {
            Either5::First(_) => {}
            Either5::Second(demand) => {
                promote_scheduler(&mut scheduler, &mut last_mode, demand.active_until_ms);
                let active_deadline =
                    Instant::now() + Duration::from_micros(scheduler.active_period_us());
                if active_deadline < next_sample_at {
                    next_sample_at = active_deadline;
                }
                continue;
            }
            Either5::Third(snapshot) => {
                touch = snapshot;
                next_sample_at = Instant::now();
                continue;
            }
            Either5::Fourth(status) => {
                touch_initializing = matches!(status, TouchStatus::Initializing);
                next_sample_at = Instant::now();
                continue;
            }
            Either5::Fifth(command) => {
                handle_control_command(command).await;
                pending_discontinuity = true;
                next_sample_at = Instant::now();
                continue;
            }
        }

        let now = Instant::now();
        let now_ms = now.as_millis();
        if now.as_micros()
            > next_sample_at
                .as_micros()
                .saturating_add(scheduler.period_us(now_ms))
        {
            metrics::record_missed_deadline();
        }
        let current_mode = scheduler.mode(now_ms);
        metrics::record_mode_change(last_mode, current_mode);
        last_mode = current_mode;
        let current_suppression = if read_app_state_snapshot().services.upload_enabled {
            Some(ImuSuppressionReason::Upload)
        } else if touch_initializing || touch_bus_quiet(touch, now_ms) {
            Some(ImuSuppressionReason::Touch)
        } else {
            None
        };

        if current_suppression != suppression {
            match current_suppression {
                Some(reason) => {
                    IMU_PIPELINE_INPUTS
                        .send(ImuPipelineInput::Suppressed { now_ms, reason })
                        .await;
                    pending_discontinuity = true;
                }
                None => {
                    IMU_PIPELINE_INPUTS
                        .send(ImuPipelineInput::Resumed { now_ms })
                        .await;
                    promote_scheduler(
                        &mut scheduler,
                        &mut last_mode,
                        now_ms.saturating_add(config.active_hold_ms),
                    );
                    pending_discontinuity = true;
                }
            }
            suppression = current_suppression;
        }

        if suppression.is_some() {
            metrics::record_suppressed(suppression.unwrap_or(ImuSuppressionReason::Touch));
            next_sample_at = now + Duration::from_micros(scheduler.idle_period_us());
            continue;
        }

        if !ready && now >= retry_at {
            match imu.init(config.sensor_odr_hz).await {
                Ok(true) => {
                    ready = true;
                    fault_notified = false;
                    pending_discontinuity = true;
                    promote_scheduler(
                        &mut scheduler,
                        &mut last_mode,
                        now_ms.saturating_add(config.active_hold_ms),
                    );
                    IMU_PIPELINE_INPUTS
                        .send(ImuPipelineInput::Recovered { now_ms })
                        .await;
                    metrics::record_recovery();
                    log_status(SerialStatusEvent::Ready);
                }
                Ok(false) | Err(_) => {
                    metrics::record_init_failure();
                    if !fault_notified {
                        IMU_PIPELINE_INPUTS
                            .send(ImuPipelineInput::Fault {
                                now_ms,
                                stage: ImuFaultStage::Initialization,
                            })
                            .await;
                        fault_notified = true;
                    }
                    retry_at = Instant::now() + Duration::from_millis(IMU_INIT_RETRY_MS);
                    log_status(SerialStatusEvent::InitFailed);
                }
            }
        }

        if ready {
            let expected_period_us = scheduler.period_us(now_ms);
            let gap_discontinuity = last_sample_at.is_some_and(|last| {
                now.saturating_duration_since(last).as_micros()
                    > expected_period_us.saturating_mul(2)
            });
            match imu.read_latest().await {
                Ok(sample) => {
                    if TAP_TRACE_ENABLED && now >= next_aux_at {
                        power_good = imu
                            .read_power_good()
                            .await
                            .ok()
                            .map(i16::from)
                            .unwrap_or(-1);
                        next_aux_at =
                            Instant::now() + Duration::from_millis(TAP_TRACE_AUX_SAMPLE_MS);
                    }
                    IMU_PIPELINE_INPUTS
                        .send(ImuPipelineInput::Sample {
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
                            power_good,
                            discontinuity: pending_discontinuity || gap_discontinuity,
                        })
                        .await;
                    metrics::record_sample(now_ms, scheduler.mode(now_ms));
                    if pending_discontinuity || gap_discontinuity {
                        metrics::record_discontinuity();
                    }
                    pending_discontinuity = false;
                    last_sample_at = Some(now);
                }
                Err(_) => {
                    metrics::record_sample_failure();
                    ready = false;
                    fault_notified = true;
                    pending_discontinuity = true;
                    retry_at = Instant::now() + Duration::from_millis(IMU_INIT_RETRY_MS);
                    IMU_PIPELINE_INPUTS
                        .send(ImuPipelineInput::Fault {
                            now_ms,
                            stage: ImuFaultStage::Sampling,
                        })
                        .await;
                    log_status(SerialStatusEvent::ReadError);
                }
            }
        }

        let completed = Instant::now();
        next_sample_at =
            completed + Duration::from_micros(scheduler.period_us(completed.as_millis()));
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
