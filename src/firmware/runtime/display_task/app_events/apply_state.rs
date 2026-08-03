async fn handle_apply_app_state_command_event(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
    command: AppStateCommand,
    ack_request_id: Option<u16>,
) {
    let result = state.apply_state_command(context, command).await;
    // A state command is complete only after its display-side effects finish.
    // In particular, upload=off can start an e-paper refresh while this task
    // owns the Inkplate I2C expander. Publishing the ACK before that refresh
    // let callers immediately request SD power and time out waiting for this
    // task to become available.
    if let Some(request_id) = ack_request_id {
        APP_STATE_APPLY_ACKS
            .send(AppStateApplyAck {
                request_id,
                snapshot: result.after,
                status: apply_status_code(result.status),
            })
            .await;
    }
}
