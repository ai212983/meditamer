#[test]
fn decoded_dropout_with_plausible_coords_updates_continuity_primary() {
    let mut n = TouchPresenceNormalizer::new();

    let (ms0, s0) = sample(0, 1, (320, 320), (0, 0), true);
    let (c0, p0) = n.normalize(ms0, s0);
    assert_eq!(c0, 1);
    assert_eq!(p0, Some(NormalizedTouchPoint { x: 320, y: 320 }));

    // Decoded drops to zero, but raw still indicates touch and coordinates
    // stay plausible near the previous position; continuity should track it.
    let mut raw = [0u8; 8];
    raw[7] = 0x01;
    let (c1, p1) = n.normalize(
        24,
        NormalizedTouchSample {
            touch_count: 0,
            points: [
                NormalizedTouchPoint { x: 352, y: 322 },
                NormalizedTouchPoint { x: 0, y: 0 },
            ],
            raw,
        },
    );
    assert_eq!(c1, 1);
    assert_eq!(p1, Some(NormalizedTouchPoint { x: 352, y: 322 }));
}

#[test]
fn decoded_dropout_with_implausible_coords_keeps_previous_primary() {
    let mut n = TouchPresenceNormalizer::new();

    let (ms0, s0) = sample(0, 1, (360, 360), (0, 0), true);
    let (c0, p0) = n.normalize(ms0, s0);
    assert_eq!(c0, 1);
    assert_eq!(p0, Some(NormalizedTouchPoint { x: 360, y: 360 }));

    // Very far coordinate during decoded dropout should be treated as noise.
    let mut raw = [0u8; 8];
    raw[7] = 0x01;
    let (c1, p1) = n.normalize(
        24,
        NormalizedTouchSample {
            touch_count: 0,
            points: [
                NormalizedTouchPoint { x: 0, y: 599 },
                NormalizedTouchPoint { x: 0, y: 0 },
            ],
            raw,
        },
    );
    assert_eq!(c1, 1);
    assert_eq!(p1, Some(NormalizedTouchPoint { x: 360, y: 360 }));
}

#[test]
fn single_contact_dual_slot_prefers_moved_candidate_when_other_is_sticky() {
    let mut n = TouchPresenceNormalizer::new();

    // Seed previous primary at (100,100).
    let (ms0, s0) = sample(0, 1, (100, 100), (0, 0), true);
    let (c0, p0) = n.normalize(ms0, s0);
    assert_eq!(c0, 1);
    assert_eq!(p0, Some(NormalizedTouchPoint { x: 100, y: 100 }));

    // Two coordinates appear, but raw says one contact. Slot A is stuck on
    // previous point while slot B moved significantly.
    let mut raw = [0u8; 8];
    raw[7] = 0x01;
    let (c1, p1) = n.normalize(
        8,
        NormalizedTouchSample {
            touch_count: 1,
            points: [
                NormalizedTouchPoint { x: 100, y: 100 },
                NormalizedTouchPoint { x: 156, y: 103 },
            ],
            raw,
        },
    );
    assert_eq!(c1, 1);
    assert_eq!(p1, Some(NormalizedTouchPoint { x: 156, y: 103 }));
}

#[test]
fn dual_contact_keeps_continuity_and_does_not_force_slot_switch() {
    let mut n = TouchPresenceNormalizer::new();

    let (ms0, s0) = sample(0, 1, (220, 220), (0, 0), true);
    let (c0, p0) = n.normalize(ms0, s0);
    assert_eq!(c0, 1);
    assert_eq!(p0, Some(NormalizedTouchPoint { x: 220, y: 220 }));

    // Two valid contacts reported: keep continuity with nearest point.
    let mut raw = [0u8; 8];
    raw[7] = 0x03;
    let (c1, p1) = n.normalize(
        8,
        NormalizedTouchSample {
            touch_count: 1,
            points: [
                NormalizedTouchPoint { x: 222, y: 221 },
                NormalizedTouchPoint { x: 300, y: 320 },
            ],
            raw,
        },
    );
    assert_eq!(c1, 1);
    assert_eq!(p1, Some(NormalizedTouchPoint { x: 222, y: 221 }));
}
use super::*;
