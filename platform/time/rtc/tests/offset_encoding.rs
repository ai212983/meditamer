//! Fixed local-offset encoding into the RTC free RAM byte: every supported
//! offset, rejection of bad granularity, and the reserved-marker exclusions.

use rtc::offset::{self, LEGACY_ARDUINO_MARKER, UNSET, VALID_RANGE_MAX, VALID_RANGE_MIN};

#[test]
fn round_trips_every_supported_offset() {
    // UTC-12:00 through UTC+14:00 in 15-minute steps.
    let mut minutes = -12 * 60;
    while minutes <= 14 * 60 {
        let encoded = offset::encode(minutes)
            .unwrap_or_else(|| panic!("offset {minutes} minutes should encode"));
        assert_eq!(offset::decode(encoded), Some(minutes), "offset {minutes}");
        minutes += 15;
    }
}

#[test]
fn rejects_offsets_outside_the_supported_range() {
    assert_eq!(offset::encode(-12 * 60 - 15), None);
    assert_eq!(offset::encode(14 * 60 + 15), None);
}

#[test]
fn rejects_offsets_not_on_a_15_minute_boundary() {
    assert_eq!(offset::encode(5), None);
    assert_eq!(offset::encode(-7), None);
    assert_eq!(offset::encode(1), None);
}

#[test]
fn every_valid_encoded_byte_falls_within_the_documented_range() {
    let mut minutes = -12 * 60;
    while minutes <= 14 * 60 {
        let encoded = offset::encode(minutes).expect("valid offset");
        assert!((VALID_RANGE_MIN..=VALID_RANGE_MAX).contains(&encoded));
        minutes += 15;
    }
}

#[test]
fn decode_rejects_the_unset_sentinel() {
    assert_eq!(offset::decode(UNSET), None);
}

#[test]
fn decode_rejects_the_legacy_arduino_marker() {
    // The old Arduino library's "time was set" sentinel must never be
    // mistaken for a valid offset.
    assert_eq!(offset::decode(LEGACY_ARDUINO_MARKER), None);
    assert!(!(VALID_RANGE_MIN..=VALID_RANGE_MAX).contains(&LEGACY_ARDUINO_MARKER));
}

#[test]
fn decode_rejects_unused_encodings_outside_the_valid_range() {
    assert_eq!(offset::decode(0x00), None);
    assert_eq!(offset::decode(VALID_RANGE_MIN - 1), None);
    assert_eq!(offset::decode(VALID_RANGE_MAX + 1), None);
}
