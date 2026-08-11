#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum UiCycleStepStatus {
    Applied,
    NotReady,
    Busy,
    NavigationFault,
    NoDirty,
    RefreshFailed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct UiCycleStepAck {
    pub(crate) request_id: u16,
    pub(crate) status: UiCycleStepStatus,
}
