fn should_do_full_refresh(state: &DisplayLoopState) -> bool {
    !state.screen_initialized
        || state
            .update_count
            .is_multiple_of(FULL_REFRESH_EVERY_N_UPDATES)
}

async fn render_active_mode_from_state(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
    uptime_seconds: u32,
) {
    render_active_mode(
        &mut context.inkplate,
        RenderActiveParams {
            base_mode: state.base_mode(),
            day_background: state.day_background(),
            overlay_mode: state.overlay_mode(),
            uptime_seconds,
            time_sync: state.time_sync,
            battery_percent: state.battery_percent,
            pattern_nonce: &mut state.pattern_nonce,
            first_visual_seed_pending: &mut state.first_visual_seed_pending,
        },
    )
    .await;
}

async fn render_visual_update_from_state(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
    uptime_seconds: u32,
) {
    render_visual_update(
        &mut context.inkplate,
        RenderVisualParams {
            day_background: state.day_background(),
            overlay_mode: state.overlay_mode(),
            uptime_seconds,
            time_sync: state.time_sync,
            battery_percent: state.battery_percent,
            pattern_nonce: &mut state.pattern_nonce,
            first_visual_seed_pending: &mut state.first_visual_seed_pending,
        },
    )
    .await;
}
