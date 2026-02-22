use embassy_time::{with_timeout, Duration, Instant};
use meditamer::event_engine::{EngineTraceSample, EventEngine, SensorFrame};

use crate::sd_probe;

use super::{
    config::{
        APP_EVENTS, IMU_INIT_RETRY_MS, TAP_TRACE_AUX_SAMPLE_MS, TAP_TRACE_ENABLED,
        TAP_TRACE_SAMPLES, TAP_TRACE_SAMPLE_MS, TOUCH_EVENTS, TOUCH_INIT_RETRY_MS, TOUCH_SAMPLE_MS,
        TOUCH_TRACE_ENABLED, TOUCH_TRACE_SAMPLES, UI_TICK_MS,
    },
    render::{
        next_visual_seed, render_active_mode, render_battery_update, render_shanshui_update,
        render_suminagashi_update, render_visual_update, sample_battery_percent,
    },
    runtime::{
        run_backlight_timeline, trigger_backlight_cycle, update_face_down_toggle,
        FaceDownToggleState,
    },
    touch::TouchEngine,
    types::{
        AppEvent, DisplayContext, DisplayMode, TapTraceSample, TimeSyncState, TouchEventKind,
        TouchSwipeDirection, TouchTraceSample,
    },
};

const TOUCH_FEEDBACK_ENABLED: bool = true;
const TOUCH_FEEDBACK_RADIUS_PX: i32 = 3;
const TOUCH_FEEDBACK_MIN_REFRESH_MS: u64 = 90;

#[embassy_executor::task]
pub(crate) async fn display_task(mut context: DisplayContext) {
    let mut update_count = 0u32;
    let mut last_uptime_seconds = 0u32;
    let mut time_sync: Option<TimeSyncState> = None;
    let mut battery_percent: Option<u8> = None;
    let mut display_mode = DisplayMode::Shanshui;
    let mut screen_initialized = false;
    let mut pattern_nonce = 0u32;
    let mut first_visual_seed_pending = true;
    let mut face_down_toggle = FaceDownToggleState::new();
    let mut imu_double_tap_ready = false;
    let mut imu_retry_at = Instant::now();
    let mut event_engine = EventEngine::default();
    let mut last_engine_trace = EngineTraceSample::default();
    let mut last_detect_tap_src = 0u8;
    let mut last_detect_int1 = 0u8;
    let trace_epoch = Instant::now();
    let mut tap_trace_next_sample_at = Instant::now();
    let mut tap_trace_aux_next_sample_at = Instant::now();
    let mut tap_trace_power_good: i16 = -1;
    let mut backlight_cycle_start: Option<Instant> = None;
    let mut backlight_level = 0u8;
    let mut touch_ready = context.inkplate.touch_init().unwrap_or(false);
    let mut touch_retry_at = Instant::now();
    let mut touch_next_sample_at = Instant::now();
    let mut touch_engine = TouchEngine::default();
    let mut touch_feedback_dirty = false;
    let mut touch_feedback_next_flush_at = Instant::now();

    if touch_ready {
        esp_println::println!("touch: ready");
    } else {
        esp_println::println!("touch: init_failed");
        touch_retry_at = Instant::now() + Duration::from_millis(TOUCH_INIT_RETRY_MS);
    }

    run_sd_probe("boot", &mut context.inkplate, &mut context.sd_probe);

    loop {
        while let Ok(touch_event) = TOUCH_EVENTS.try_receive() {
            let _ = APP_EVENTS.try_send(AppEvent::Touch(touch_event));
        }

        let app_wait_ms = next_loop_wait_ms(
            touch_ready,
            touch_retry_at,
            touch_next_sample_at,
            imu_double_tap_ready,
            imu_retry_at,
            touch_feedback_dirty,
            touch_feedback_next_flush_at,
            TAP_TRACE_ENABLED && imu_double_tap_ready,
            tap_trace_next_sample_at,
            tap_trace_aux_next_sample_at,
        );

        match with_timeout(Duration::from_millis(app_wait_ms), APP_EVENTS.receive()).await {
            Ok(event) => match event {
                AppEvent::Refresh { uptime_seconds } => {
                    last_uptime_seconds = uptime_seconds;
                    if display_mode == DisplayMode::Clock {
                        let do_full_refresh = !screen_initialized
                            || update_count % super::config::FULL_REFRESH_EVERY_N_UPDATES == 0;
                        super::render::render_clock_update(
                            &mut context.inkplate,
                            uptime_seconds,
                            time_sync,
                            battery_percent,
                            do_full_refresh,
                        );
                        update_count = update_count.wrapping_add(1);
                    } else {
                        render_visual_update(
                            &mut context.inkplate,
                            display_mode,
                            uptime_seconds,
                            time_sync,
                            &mut pattern_nonce,
                            &mut first_visual_seed_pending,
                        )
                        .await;
                        update_count = 0;
                    }
                    screen_initialized = true;
                }
                AppEvent::BatteryTick => {
                    if let Some(sampled_percent) = sample_battery_percent(&mut context.inkplate) {
                        battery_percent = Some(sampled_percent);
                    }

                    if screen_initialized {
                        if display_mode == DisplayMode::Clock {
                            render_battery_update(&mut context.inkplate, battery_percent);
                        }
                    } else if display_mode == DisplayMode::Clock {
                        render_active_mode(
                            &mut context.inkplate,
                            display_mode,
                            last_uptime_seconds,
                            time_sync,
                            battery_percent,
                            &mut pattern_nonce,
                            &mut first_visual_seed_pending,
                            true,
                        )
                        .await;
                        screen_initialized = true;
                    }
                }
                AppEvent::TimeSync(cmd) => {
                    let uptime_now = Instant::now().as_secs().min(u32::MAX as u64) as u32;
                    last_uptime_seconds = last_uptime_seconds.max(uptime_now);
                    time_sync = Some(TimeSyncState {
                        unix_epoch_utc_seconds: cmd.unix_epoch_utc_seconds,
                        tz_offset_minutes: cmd.tz_offset_minutes,
                        sync_instant: Instant::now(),
                    });
                    update_count = 0;
                    render_active_mode(
                        &mut context.inkplate,
                        display_mode,
                        last_uptime_seconds,
                        time_sync,
                        battery_percent,
                        &mut pattern_nonce,
                        &mut first_visual_seed_pending,
                        true,
                    )
                    .await;
                    screen_initialized = true;
                }
                AppEvent::ForceRepaint => {
                    update_count = 0;
                    render_active_mode(
                        &mut context.inkplate,
                        display_mode,
                        last_uptime_seconds,
                        time_sync,
                        battery_percent,
                        &mut pattern_nonce,
                        &mut first_visual_seed_pending,
                        true,
                    )
                    .await;
                    screen_initialized = true;
                }
                AppEvent::ForceMarbleRepaint => {
                    let seed = next_visual_seed(
                        last_uptime_seconds,
                        time_sync,
                        &mut pattern_nonce,
                        &mut first_visual_seed_pending,
                    );
                    if display_mode == DisplayMode::Shanshui {
                        render_shanshui_update(
                            &mut context.inkplate,
                            seed,
                            last_uptime_seconds,
                            time_sync,
                        )
                        .await;
                    } else {
                        render_suminagashi_update(
                            &mut context.inkplate,
                            seed,
                            last_uptime_seconds,
                            time_sync,
                        )
                        .await;
                    }
                    screen_initialized = true;
                }
                AppEvent::SdProbe => {
                    run_sd_probe("manual", &mut context.inkplate, &mut context.sd_probe);
                }
                AppEvent::Touch(event) => match event.kind {
                    TouchEventKind::Down | TouchEventKind::Move if TOUCH_FEEDBACK_ENABLED => {
                        draw_touch_feedback_dot(&mut context.inkplate, event.x, event.y);
                        touch_feedback_dirty = true;
                    }
                    TouchEventKind::Tap => {
                        trigger_backlight_cycle(
                            &mut context.inkplate,
                            &mut backlight_cycle_start,
                            &mut backlight_level,
                        );
                    }
                    TouchEventKind::LongPress => {
                        update_count = 0;
                        render_active_mode(
                            &mut context.inkplate,
                            display_mode,
                            last_uptime_seconds,
                            time_sync,
                            battery_percent,
                            &mut pattern_nonce,
                            &mut first_visual_seed_pending,
                            true,
                        )
                        .await;
                        screen_initialized = true;
                    }
                    TouchEventKind::Swipe(direction) => {
                        display_mode = match direction {
                            TouchSwipeDirection::Right | TouchSwipeDirection::Down => {
                                display_mode.toggled()
                            }
                            TouchSwipeDirection::Left | TouchSwipeDirection::Up => {
                                display_mode.toggled_reverse()
                            }
                        };
                        context.mode_store.save_mode(display_mode);
                        update_count = 0;
                        render_active_mode(
                            &mut context.inkplate,
                            display_mode,
                            last_uptime_seconds,
                            time_sync,
                            battery_percent,
                            &mut pattern_nonce,
                            &mut first_visual_seed_pending,
                            true,
                        )
                        .await;
                        screen_initialized = true;
                    }
                    _ => {}
                },
            },
            Err(_) => {}
        }

        if touch_feedback_dirty && Instant::now() >= touch_feedback_next_flush_at {
            let _ = context.inkplate.display_bw_partial(false);
            touch_feedback_dirty = false;
            touch_feedback_next_flush_at =
                Instant::now() + Duration::from_millis(TOUCH_FEEDBACK_MIN_REFRESH_MS);
        }

        if !imu_double_tap_ready && Instant::now() >= imu_retry_at {
            imu_double_tap_ready = context.inkplate.lsm6ds3_init_double_tap().unwrap_or(false);
            if imu_double_tap_ready {
                let now_ms = Instant::now()
                    .saturating_duration_since(trace_epoch)
                    .as_millis();
                last_engine_trace = event_engine.imu_recovered(now_ms).trace;
            }
            imu_retry_at = Instant::now() + Duration::from_millis(IMU_INIT_RETRY_MS);
        }

        if imu_double_tap_ready {
            match (
                context.inkplate.lsm6ds3_read_tap_src(),
                context.inkplate.lsm6ds3_int1_level(),
                context.inkplate.lsm6ds3_read_motion_raw(),
            ) {
                (Ok(tap_src), Ok(int1), Ok((gx, gy, gz, ax, ay, az))) => {
                    let now = Instant::now();
                    let now_ms = now.saturating_duration_since(trace_epoch).as_millis();
                    last_detect_tap_src = tap_src;
                    last_detect_int1 = if int1 { 1 } else { 0 };

                    let output = event_engine.tick(SensorFrame {
                        now_ms,
                        tap_src,
                        int1,
                        gx,
                        gy,
                        gz,
                        ax,
                        ay,
                        az,
                    });
                    last_engine_trace = output.trace;

                    if output.actions.contains_backlight_trigger() {
                        trigger_backlight_cycle(
                            &mut context.inkplate,
                            &mut backlight_cycle_start,
                            &mut backlight_level,
                        );
                    }

                    if update_face_down_toggle(&mut face_down_toggle, now, ax, ay, az) {
                        display_mode = display_mode.toggled();
                        context.mode_store.save_mode(display_mode);
                        update_count = 0;
                        render_active_mode(
                            &mut context.inkplate,
                            display_mode,
                            last_uptime_seconds,
                            time_sync,
                            battery_percent,
                            &mut pattern_nonce,
                            &mut first_visual_seed_pending,
                            true,
                        )
                        .await;
                        screen_initialized = true;
                    }
                }
                _ => {
                    imu_double_tap_ready = false;
                    let now_ms = Instant::now()
                        .saturating_duration_since(trace_epoch)
                        .as_millis();
                    last_engine_trace = event_engine.imu_fault(now_ms).trace;
                    last_detect_tap_src = 0;
                    last_detect_int1 = 0;
                    imu_retry_at = Instant::now() + Duration::from_millis(IMU_INIT_RETRY_MS);
                }
            }
        }

        if TAP_TRACE_ENABLED && imu_double_tap_ready {
            let now = Instant::now();

            if now >= tap_trace_aux_next_sample_at {
                tap_trace_power_good = context
                    .inkplate
                    .read_power_good()
                    .ok()
                    .map(|v| v as i16)
                    .unwrap_or(-1);
                tap_trace_aux_next_sample_at = now + Duration::from_millis(TAP_TRACE_AUX_SAMPLE_MS);
            }

            if now >= tap_trace_next_sample_at {
                match (
                    context.inkplate.lsm6ds3_int2_level(),
                    context.inkplate.lsm6ds3_read_motion_raw(),
                ) {
                    (Ok(int2), Ok((gx, gy, gz, ax, ay, az))) => {
                        let battery_percent_i16 = battery_percent.map_or(-1, i16::from);
                        let t_ms = now.saturating_duration_since(trace_epoch).as_millis();
                        let sample = TapTraceSample {
                            t_ms,
                            tap_src: last_detect_tap_src,
                            seq_count: last_engine_trace.seq_count,
                            tap_candidate: last_engine_trace.tap_candidate,
                            cand_src: last_engine_trace.candidate_source_mask,
                            state_id: last_engine_trace.state_id.as_u8(),
                            reject_reason: last_engine_trace.reject_reason.as_u8(),
                            candidate_score: last_engine_trace.candidate_score.0,
                            window_ms: last_engine_trace.window_ms,
                            cooldown_active: last_engine_trace.cooldown_active,
                            jerk_l1: last_engine_trace.jerk_l1,
                            motion_veto: last_engine_trace.motion_veto,
                            gyro_l1: last_engine_trace.gyro_l1,
                            int1: last_detect_int1,
                            int2: if int2 { 1 } else { 0 },
                            power_good: tap_trace_power_good,
                            battery_percent: battery_percent_i16,
                            gx,
                            gy,
                            gz,
                            ax,
                            ay,
                            az,
                        };
                        let _ = TAP_TRACE_SAMPLES.try_send(sample);
                    }
                    _ => {}
                }
                tap_trace_next_sample_at = now + Duration::from_millis(TAP_TRACE_SAMPLE_MS);
            }
        }

        if !touch_ready && Instant::now() >= touch_retry_at {
            touch_ready = context.inkplate.touch_init().unwrap_or(false);
            if touch_ready {
                esp_println::println!("touch: recovered");
                touch_next_sample_at = Instant::now();
            } else {
                touch_retry_at = Instant::now() + Duration::from_millis(TOUCH_INIT_RETRY_MS);
            }
        }

        if touch_ready && Instant::now() >= touch_next_sample_at {
            let now = Instant::now();
            match context.inkplate.touch_read_sample(0) {
                Ok(sample) => {
                    let t_ms = now.saturating_duration_since(trace_epoch).as_millis();
                    let output = touch_engine.tick(t_ms, sample);
                    for touch_event in output.events.into_iter().flatten() {
                        let _ = TOUCH_EVENTS.try_send(touch_event);
                    }

                    if TOUCH_TRACE_ENABLED && sample.touch_count > 0 {
                        let _ = TOUCH_TRACE_SAMPLES
                            .try_send(TouchTraceSample::from_sample(t_ms, sample));
                    }
                }
                Err(_) => {
                    touch_ready = false;
                    let _ = context.inkplate.touch_shutdown();
                    touch_retry_at = now + Duration::from_millis(TOUCH_INIT_RETRY_MS);
                    esp_println::println!("touch: read_error; retrying");
                }
            }
            touch_next_sample_at = now + Duration::from_millis(TOUCH_SAMPLE_MS);
        }

        run_backlight_timeline(
            &mut context.inkplate,
            &mut backlight_cycle_start,
            &mut backlight_level,
        );
    }
}

fn next_loop_wait_ms(
    touch_ready: bool,
    touch_retry_at: Instant,
    touch_next_sample_at: Instant,
    imu_ready: bool,
    imu_retry_at: Instant,
    touch_feedback_dirty: bool,
    touch_feedback_next_flush_at: Instant,
    tap_trace_active: bool,
    tap_trace_next_sample_at: Instant,
    tap_trace_aux_next_sample_at: Instant,
) -> u64 {
    let now = Instant::now();
    let mut wait_ms = UI_TICK_MS;

    if touch_ready {
        wait_ms = wait_ms.min(ms_until(now, touch_next_sample_at));
    } else {
        wait_ms = wait_ms.min(ms_until(now, touch_retry_at));
    }

    if !imu_ready {
        wait_ms = wait_ms.min(ms_until(now, imu_retry_at));
    }

    if touch_feedback_dirty {
        wait_ms = wait_ms.min(ms_until(now, touch_feedback_next_flush_at));
    }

    if tap_trace_active {
        wait_ms = wait_ms.min(ms_until(now, tap_trace_next_sample_at));
        wait_ms = wait_ms.min(ms_until(now, tap_trace_aux_next_sample_at));
    }

    wait_ms
}

fn ms_until(now: Instant, deadline: Instant) -> u64 {
    if deadline <= now {
        0
    } else {
        deadline.saturating_duration_since(now).as_millis()
    }
}

fn run_sd_probe(
    reason: &str,
    inkplate: &mut super::types::InkplateDriver,
    sd_probe: &mut super::types::SdProbeDriver,
) {
    if inkplate.sd_card_power_on().is_err() {
        esp_println::println!("sdprobe[{}]: power_on_error", reason);
        return;
    }

    let result = sd_probe.probe();

    match result {
        Ok(status) => {
            let version = match status.version {
                sd_probe::SdCardVersion::V1 => "v1.x",
                sd_probe::SdCardVersion::V2 => "v2+",
            };
            let capacity = if status.high_capacity {
                "sdhc_or_sdxc"
            } else {
                "sdsc"
            };
            let filesystem = match status.filesystem {
                sd_probe::SdFilesystem::ExFat => "exfat",
                sd_probe::SdFilesystem::Fat32 => "fat32",
                sd_probe::SdFilesystem::Fat16 => "fat16",
                sd_probe::SdFilesystem::Fat12 => "fat12",
                sd_probe::SdFilesystem::Ntfs => "ntfs",
                sd_probe::SdFilesystem::Unknown => "unknown",
            };
            let gib_x100 = status
                .capacity_bytes
                .saturating_mul(100)
                .saturating_div(1024 * 1024 * 1024);
            let gib_int = gib_x100 / 100;
            let gib_frac = gib_x100 % 100;
            esp_println::println!(
                "sdprobe[{}]: card_detected version={} capacity={} fs={} bytes={} size_gib={}.{:02}",
                reason,
                version,
                capacity,
                filesystem,
                status.capacity_bytes,
                gib_int,
                gib_frac
            );
        }
        Err(err) => match err {
            sd_probe::SdProbeError::Spi(spi_err) => {
                esp_println::println!("sdprobe[{}]: not_detected spi_error={:?}", reason, spi_err);
            }
            sd_probe::SdProbeError::Cmd0Failed(r1) => {
                esp_println::println!("sdprobe[{}]: not_detected cmd0_r1=0x{:02x}", reason, r1);
            }
            sd_probe::SdProbeError::Cmd8Unexpected(r1) => {
                esp_println::println!("sdprobe[{}]: not_detected cmd8_r1=0x{:02x}", reason, r1);
            }
            sd_probe::SdProbeError::Cmd8EchoMismatch(r7) => {
                esp_println::println!(
                    "sdprobe[{}]: not_detected cmd8_echo={:02x}{:02x}{:02x}{:02x}",
                    reason,
                    r7[0],
                    r7[1],
                    r7[2],
                    r7[3]
                );
            }
            sd_probe::SdProbeError::Acmd41Timeout(r1) => {
                esp_println::println!(
                    "sdprobe[{}]: not_detected acmd41_last_r1=0x{:02x}",
                    reason,
                    r1
                );
            }
            sd_probe::SdProbeError::Cmd58Unexpected(r1) => {
                esp_println::println!("sdprobe[{}]: not_detected cmd58_r1=0x{:02x}", reason, r1);
            }
            sd_probe::SdProbeError::Cmd9Unexpected(r1) => {
                esp_println::println!("sdprobe[{}]: not_detected cmd9_r1=0x{:02x}", reason, r1);
            }
            sd_probe::SdProbeError::Cmd17Unexpected(r1) => {
                esp_println::println!("sdprobe[{}]: not_detected cmd17_r1=0x{:02x}", reason, r1);
            }
            sd_probe::SdProbeError::NoResponse(cmd) => {
                esp_println::println!("sdprobe[{}]: not_detected cmd{}_no_response", reason, cmd);
            }
            sd_probe::SdProbeError::DataTokenTimeout(cmd) => {
                esp_println::println!(
                    "sdprobe[{}]: not_detected cmd{}_data_token_timeout",
                    reason,
                    cmd
                );
            }
            sd_probe::SdProbeError::DataTokenUnexpected(cmd, token) => {
                esp_println::println!(
                    "sdprobe[{}]: not_detected cmd{}_data_token=0x{:02x}",
                    reason,
                    cmd,
                    token
                );
            }
            sd_probe::SdProbeError::CapacityDecodeFailed => {
                esp_println::println!("sdprobe[{}]: not_detected capacity_decode_failed", reason);
            }
        },
    }

    if inkplate.sd_card_power_off().is_err() {
        esp_println::println!("sdprobe[{}]: power_off_error", reason);
    }
}

fn draw_touch_feedback_dot(display: &mut super::types::InkplateDriver, x: u16, y: u16) {
    let cx = x as i32;
    let cy = y as i32;
    let radius = TOUCH_FEEDBACK_RADIUS_PX.max(1);
    let radius_sq = radius * radius;
    let width = display.width() as i32;
    let height = display.height() as i32;

    for dy in -radius..=radius {
        for dx in -radius..=radius {
            if dx * dx + dy * dy > radius_sq {
                continue;
            }

            let px = cx + dx;
            let py = cy + dy;
            if px < 0 || py < 0 || px >= width || py >= height {
                continue;
            }

            display.set_pixel_bw(px as usize, py as usize, true);
        }
    }
}
