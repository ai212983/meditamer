use crate::firmware::app_state::AppStateSnapshot;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct AppStateApplyAck {
    pub(crate) request_id: u16,
    pub(crate) snapshot: AppStateSnapshot,
    pub(crate) status: u8,
}
