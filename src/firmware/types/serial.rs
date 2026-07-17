#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SerialStatusEvent {
    ImuScheduler {
        sensor_odr_hz: u16,
        idle_hz: u16,
        active_hz: u16,
        active_hold_ms: u64,
    },
    ImuReady,
    ImuInitFailed,
    ImuReadError,
}
