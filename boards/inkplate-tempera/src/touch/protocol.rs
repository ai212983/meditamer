pub(crate) const TOUCH_REPORT_HEADER: u8 = 0x5A;
pub(crate) const TOUCH_ACTIVE_MASK: u8 = 0x03;

pub const fn is_touch_report(raw: &[u8; 8]) -> bool {
    raw[0] == TOUCH_REPORT_HEADER
}

pub const fn active_slots(raw: &[u8; 8]) -> u8 {
    if is_touch_report(raw) {
        raw[7] & TOUCH_ACTIVE_MASK
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retained_coordinates_do_not_create_contact_after_release() {
        let raw = [0x5A, 0x32, 0xA5, 0x4E, 0x12, 0x34, 0x56, 0x00];

        assert_eq!(active_slots(&raw), 0);
    }

    #[test]
    fn only_low_status_bits_are_active_touch_slots() {
        let raw = [0x5A, 0, 0, 0, 0, 0, 0, 0xF2];

        assert_eq!(active_slots(&raw), 0x02);
    }

    #[test]
    fn non_touch_packets_have_no_active_slots() {
        let raw = [0x55, 0, 0, 0, 0, 0, 0, 0x03];

        assert_eq!(active_slots(&raw), 0);
    }
}
