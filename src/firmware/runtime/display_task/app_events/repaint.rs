async fn handle_force_repaint_event(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
    upload_enabled: bool,
) {
    if upload_enabled || state.in_touch_wizard_mode() {
        return;
    }
    state.update_count = 0;
    render_active_mode_from_state(context, state, state.last_uptime_seconds).await;
    state.screen_initialized = true;
}

async fn handle_force_marble_repaint_event(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
    upload_enabled: bool,
) {
    if upload_enabled || state.in_touch_wizard_mode() {
        return;
    }
    let last_uptime_seconds = state.last_uptime_seconds;
    let time_sync = state.time_sync;
    let seed = next_visual_seed(
        last_uptime_seconds,
        time_sync,
        &mut state.pattern_nonce,
        &mut state.first_visual_seed_pending,
    );
    if matches!(
        state.day_background(),
        crate::firmware::app_state::DayBackground::Shanshui
    ) {
        render_shanshui_update(&mut context.inkplate, seed, last_uptime_seconds, time_sync).await;
    } else {
        render_suminagashi_update(&mut context.inkplate, seed, last_uptime_seconds, time_sync)
            .await;
    }
    if matches!(state.overlay_mode(), OverlayMode::Clock) {
        render_clock_overlay(
            &mut context.inkplate,
            last_uptime_seconds,
            time_sync,
            state.battery_percent,
        )
        .await;
    }
    state.screen_initialized = true;
}
