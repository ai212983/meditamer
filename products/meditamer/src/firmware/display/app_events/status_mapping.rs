pub(super) fn apply_status_code(
    status: crate::firmware::app_state::actions::AppStateApplyStatus,
) -> u8 {
    match status {
        crate::firmware::app_state::actions::AppStateApplyStatus::Applied => 0,
        crate::firmware::app_state::actions::AppStateApplyStatus::Unchanged => 1,
        crate::firmware::app_state::actions::AppStateApplyStatus::InvalidTransition => 2,
    }
}
