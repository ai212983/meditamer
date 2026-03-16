use super::WifiEvent;

pub(crate) unsafe fn dispatch_event_handler(
    _event: WifiEvent,
    _event_data: *mut crate::binary::c_types::c_void,
    _event_data_size: usize,
) -> bool {
    false
}
