#[path = "../../../src/platform/inkplate/touch/protocol.rs"]
mod protocol;

#[test]
fn retained_coordinates_do_not_create_contact_after_release() {
    let raw = [0x5A, 0x32, 0xA5, 0x4E, 0x12, 0x34, 0x56, 0x00];

    assert_eq!(protocol::active_slots(&raw), 0);
}

#[test]
fn only_low_status_bits_are_active_touch_slots() {
    let raw = [0x5A, 0, 0, 0, 0, 0, 0, 0xF2];

    assert_eq!(protocol::active_slots(&raw), 0x02);
}

#[test]
fn non_touch_packets_have_no_active_slots() {
    let raw = [0x55, 0, 0, 0, 0, 0, 0, 0x03];

    assert_eq!(protocol::active_slots(&raw), 0);
}
