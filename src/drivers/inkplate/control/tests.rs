use super::{touch_presence_count, touch_raw_frame_has_contact, touch_raw_point_plausible};

#[test]
fn rejects_zero_axes() {
    assert!(!touch_raw_point_plausible(0, 100, 2048, 2048));
    assert!(!touch_raw_point_plausible(100, 0, 2048, 2048));
}

#[test]
fn rejects_out_of_range() {
    assert!(!touch_raw_point_plausible(2049, 100, 2048, 2048));
    assert!(!touch_raw_point_plausible(100, 3000, 2048, 2048));
}

#[test]
fn accepts_in_range_non_zero_points() {
    assert!(touch_raw_point_plausible(1, 1, 2048, 2048));
    assert!(touch_raw_point_plausible(2048, 2048, 2048, 2048));
}

#[test]
fn presence_requires_bits_and_coords() {
    assert_eq!(touch_presence_count(0, 0), 0);
    // Bit-only presence must be preserved; higher layers debounce and gate it.
    assert_eq!(touch_presence_count(1, 0), 1);
    // Bit count may flicker low while coordinates are still valid.
    assert_eq!(touch_presence_count(0, 1), 1);
    assert_eq!(touch_presence_count(1, 1), 1);
    assert_eq!(touch_presence_count(2, 1), 1);
    assert_eq!(touch_presence_count(1, 2), 2);
    assert_eq!(touch_presence_count(2, 2), 2);
}

#[test]
fn raw_frame_has_contact_when_status_bits_are_set() {
    let mut raw = [0u8; 8];
    raw[7] = 0x01;
    assert!(touch_raw_frame_has_contact(&raw, 2048, 2048));
}

#[test]
fn raw_frame_has_contact_when_decoded_coordinate_is_plausible() {
    let mut raw = [0u8; 8];
    raw[1] = 0x14; // x_high=0x1, y_high=0x4
    raw[2] = 0x23; // x_low
    raw[3] = 0x56; // y_low
    assert!(touch_raw_frame_has_contact(&raw, 2048, 2048));
}

#[test]
fn raw_frame_without_bits_or_coords_is_empty() {
    let raw = [0u8; 8];
    assert!(!touch_raw_frame_has_contact(&raw, 2048, 2048));
}
