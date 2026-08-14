use super::elan::{
    decode_sample, decode_xy, raw_frame_is_touch_report, raw_point_plausible, transform_point,
};

#[test]
fn decodes_elan_packet_coordinates() {
    let raw = [0x5a, 0x32, 0xa5, 0x4e, 0, 0, 0, 1];
    assert_eq!(decode_xy(&raw, 0), (933, 590));
}

#[test]
fn validates_touch_report_header_and_edge_coordinates() {
    assert!(raw_point_plausible(0, 10, 1152, 1152));
    assert!(raw_point_plausible(10, 0, 1152, 1152));
    assert!(!raw_point_plausible(1153, 10, 1152, 1152));
    assert!(raw_frame_is_touch_report(&[0x5A, 0, 0, 0, 0, 0, 0, 0]));
    assert!(!raw_frame_is_touch_report(&[0; 8]));
}

#[test]
fn tempera_rotation_zero_matches_reference_mapping() {
    let point = transform_point(933, 590, 0, 1152, 1152);
    assert_eq!((point.x, point.y), (307, 114));
}

#[test]
fn release_status_ignores_retained_coordinate_bytes() {
    let raw = [0x5A, 0x32, 0xA5, 0x4E, 0, 0, 0, 0];
    let sample = decode_sample(raw, 0, 1152, 1152);

    assert_eq!(sample.touch_count, 0);
    assert_eq!((sample.points[0].x, sample.points[0].y), (0, 0));
}

#[test]
fn inactive_slot_coordinates_cannot_create_multitouch() {
    let raw = [0x5A, 0x32, 0xA5, 0x4E, 0x12, 0x34, 0x56, 0x01];
    let sample = decode_sample(raw, 0, 1152, 1152);

    assert_eq!(sample.touch_count, 1);
    assert_eq!((sample.points[0].x, sample.points[0].y), (307, 114));
    assert_eq!((sample.points[1].x, sample.points[1].y), (0, 0));
}

#[test]
fn second_active_slot_remains_in_its_protocol_slot() {
    let raw = [0x5A, 0, 0, 0, 0x32, 0xA5, 0x4E, 0x02];
    let sample = decode_sample(raw, 0, 1152, 1152);

    assert_eq!(sample.touch_count, 1);
    assert_eq!((sample.points[0].x, sample.points[0].y), (0, 0));
    assert_eq!((sample.points[1].x, sample.points[1].y), (307, 114));
}

#[test]
fn non_touch_packet_is_not_decoded_as_contact() {
    let raw = [0x55, 0x55, 0x55, 0x55, 0, 0, 0, 0x03];
    let sample = decode_sample(raw, 0, 1152, 1152);

    assert_eq!(sample.touch_count, 0);
    assert_eq!((sample.points[0].x, sample.points[0].y), (0, 0));
    assert_eq!((sample.points[1].x, sample.points[1].y), (0, 0));
}
