async fn handle_refresh_event(
    uptime_seconds: u32,
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
    upload_enabled: bool,
) {
    state.last_uptime_seconds = uptime_seconds;
    if upload_enabled || state.in_touch_wizard_mode() {
        return;
    }
    if should_do_full_refresh(state) {
        render_active_mode_from_state(context, state, uptime_seconds).await;
    } else {
        render_visual_update_from_state(context, state, uptime_seconds).await;
    }
    state.update_count = state.update_count.wrapping_add(1);
    state.screen_initialized = true;
}

async fn handle_battery_tick_event(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
    upload_enabled: bool,
) {
    if upload_enabled {
        return;
    }
    if let Some(sampled_percent) = sample_battery_percent(&mut context.inkplate).await {
        state.battery_percent = Some(sampled_percent);
        crate::firmware::imu::publish_trace_context(crate::firmware::imu::ImuTraceContext {
            battery_percent: i16::from(sampled_percent),
        });
    }
    if state.in_touch_wizard_mode() {
        return;
    }
    if state.screen_initialized {
        if matches!(state.overlay_mode(), OverlayMode::Clock) {
            render_clock_overlay(
                &mut context.inkplate,
                state.last_uptime_seconds,
                state.time_sync,
                state.battery_percent,
            )
            .await;
        }
        return;
    }
    render_active_mode_from_state(context, state, state.last_uptime_seconds).await;
    state.screen_initialized = true;
}

async fn handle_time_sync_event(
    cmd: crate::firmware::types::TimeSyncCommand,
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
    upload_enabled: bool,
) {
    let uptime_now = Instant::now().as_secs().min(u32::MAX as u64) as u32;
    state.last_uptime_seconds = state.last_uptime_seconds.max(uptime_now);
    state.time_sync = Some(TimeSyncState {
        unix_epoch_utc_seconds: cmd.unix_epoch_utc_seconds,
        tz_offset_minutes: cmd.tz_offset_minutes,
        sync_instant: Instant::now(),
    });
    state.update_count = 0;
    if upload_enabled || state.in_touch_wizard_mode() {
        return;
    }
    render_active_mode_from_state(context, state, state.last_uptime_seconds).await;
    state.screen_initialized = true;
}
