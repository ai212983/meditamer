pub(crate) const TOUCH_REPORT_HEADER: u8 = 0x5A;
pub(crate) const TOUCH_ACTIVE_MASK: u8 = 0x03;

pub(crate) const fn is_touch_report(raw: &[u8; 8]) -> bool {
    raw[0] == TOUCH_REPORT_HEADER
}

pub(crate) const fn active_slots(raw: &[u8; 8]) -> u8 {
    if is_touch_report(raw) {
        raw[7] & TOUCH_ACTIVE_MASK
    } else {
        0
    }
}
