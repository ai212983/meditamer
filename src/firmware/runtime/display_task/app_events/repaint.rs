async fn handle_force_repaint_event(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
    upload_enabled: bool,
) {
    if upload_enabled {
        return;
    }
    super::lvgl::force_full_repaint(context, &mut state.lvgl, "serial_repaint").await;
}
