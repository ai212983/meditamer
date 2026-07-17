use super::elan::{decode_xy, raw_frame_has_contact, raw_point_plausible, transform_point};

#[test]
fn decodes_elan_packet_coordinates() {
    let raw = [0x5a, 0x32, 0xa5, 0x4e, 0, 0, 0, 1];
    assert_eq!(decode_xy(&raw, 0), (933, 590));
}

#[test]
fn validates_presence_from_bits_or_coordinates() {
    assert!(!raw_point_plausible(0, 10, 1152, 1152));
    let mut raw = [0u8; 8];
    raw[7] = 1;
    assert!(raw_frame_has_contact(&raw, 1152, 1152));
}

#[test]
fn tempera_rotation_zero_matches_reference_mapping() {
    let point = transform_point(933, 590, 0, 1152, 1152);
    assert_eq!((point.x, point.y), (307, 114));
}
