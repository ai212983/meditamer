    fn sample(
        ms: u64,
        touch_count: u8,
        p0: (u16, u16),
        p1: (u16, u16),
        raw_bit7: bool,
    ) -> (u64, NormalizedTouchSample) {
        let mut raw = [0u8; 8];
        if raw_bit7 {
            raw[7] = 0x01;
        }
        (
            ms,
            NormalizedTouchSample {
                touch_count,
                points: [
                    NormalizedTouchPoint { x: p0.0, y: p0.1 },
                    NormalizedTouchPoint { x: p1.0, y: p1.1 },
                ],
                raw,
            },
        )
    }

    #[test]
    fn brief_dropout_keeps_presence_then_expires() {
        let mut n = TouchPresenceNormalizer::new();

        let (ms0, s0) = sample(0, 1, (120, 220), (0, 0), true);
        let (c0, p0) = n.normalize(ms0, s0);
        assert_eq!(c0, 1);
        assert_eq!(p0, Some(NormalizedTouchPoint { x: 120, y: 220 }));

        // Decoded coordinates drop out, but raw still says touch -> keep presence.
        let (ms1, s1) = sample(8, 0, (0, 0), (0, 0), true);
        let (c1, p1) = n.normalize(ms1, s1);
        assert_eq!(c1, 1);
        assert_eq!(p1, Some(NormalizedTouchPoint { x: 120, y: 220 }));

        // Even if raw clears briefly, decoded grace should preserve touch.
        let (ms2, s2) = sample(16, 0, (0, 0), (0, 0), false);
        let (c2, p2) = n.normalize(ms2, s2);
        assert_eq!(c2, 1);
        assert_eq!(p2, Some(NormalizedTouchPoint { x: 120, y: 220 }));

        // After raw-assist expiry with no decoded presence, touch must end.
        let (ms3, s3) = sample(80, 0, (0, 0), (0, 0), false);
        let (c3, p3) = n.normalize(ms3, s3);
        assert_eq!(c3, 0);
        assert_eq!(p3, None);
    }

    #[test]
    fn grace_window_does_not_self_latch_forever() {
        let mut n = TouchPresenceNormalizer::new();

        let (ms0, s0) = sample(0, 1, (10, 10), (0, 0), true);
        let (c0, _) = n.normalize(ms0, s0);
        assert_eq!(c0, 1);

        let (ms1, s1) = sample(8, 0, (0, 0), (0, 0), false);
        let (c1, _) = n.normalize(ms1, s1);
        assert_eq!(c1, 1);

        let (ms2, s2) = sample(64, 0, (0, 0), (0, 0), false);
        let (c2, _) = n.normalize(ms2, s2);
        assert_eq!(c2, 0);

        // Must stay released (no self-latching through "recent present" feedback).
        let (ms3, s3) = sample(64, 0, (0, 0), (0, 0), false);
        let (c3, _) = n.normalize(ms3, s3);
        assert_eq!(c3, 0);
    }

    #[test]
    fn decoded_gap_of_32ms_keeps_touch_continuity() {
        let mut n = TouchPresenceNormalizer::new();

        let (ms0, s0) = sample(0, 1, (180, 260), (0, 0), true);
        let (c0, p0) = n.normalize(ms0, s0);
        assert_eq!(c0, 1);
        assert_eq!(p0, Some(NormalizedTouchPoint { x: 180, y: 260 }));

        // Some panels emit one decoded frame, then multiple all-zero reads.
        // A 32 ms hole should still keep the same touch alive.
        let (ms1, s1) = sample(32, 0, (0, 0), (0, 0), false);
        let (c1, p1) = n.normalize(ms1, s1);
        assert_eq!(c1, 1);
        assert_eq!(p1, Some(NormalizedTouchPoint { x: 180, y: 260 }));

        // But continuity must still expire shortly after.
        let (ms2, s2) = sample(72, 0, (0, 0), (0, 0), false);
        let (c2, p2) = n.normalize(ms2, s2);
        assert_eq!(c2, 0);
        assert_eq!(p2, None);
    }

    #[test]
    fn raw_noise_without_decoded_touch_never_creates_presence() {
        let mut n = TouchPresenceNormalizer::new();

        let (ms0, s0) = sample(0, 0, (0, 0), (0, 0), true);
        let (c0, p0) = n.normalize(ms0, s0);
        assert_eq!(c0, 0);
        assert_eq!(p0, None);

        let (ms1, s1) = sample(24, 0, (0, 0), (0, 0), true);
        let (c1, p1) = n.normalize(ms1, s1);
        assert_eq!(c1, 0);
        assert_eq!(p1, None);
    }

    #[test]
    fn bit_only_frame_without_recent_coordinate_presence_does_not_latch() {
        let mut n = TouchPresenceNormalizer::new();

        // Status bit only, no decoded coordinate.
        let mut raw = [0u8; 8];
        raw[7] = 0x01;
        let (c0, p0) = n.normalize(
            0,
            NormalizedTouchSample {
                touch_count: 1,
                points: [
                    NormalizedTouchPoint { x: 0, y: 0 },
                    NormalizedTouchPoint { x: 0, y: 0 },
                ],
                raw,
            },
        );
        assert_eq!(c0, 0);
        assert_eq!(p0, None);
    }

    #[test]
    fn bit_only_frame_after_real_touch_keeps_short_continuity_then_releases() {
        let mut n = TouchPresenceNormalizer::new();

        let (ms0, s0) = sample(0, 1, (210, 310), (0, 0), true);
        let (c0, p0) = n.normalize(ms0, s0);
        assert_eq!(c0, 1);
        assert_eq!(p0, Some(NormalizedTouchPoint { x: 210, y: 310 }));

        let mut raw = [0u8; 8];
        raw[7] = 0x01;
        let (c1, p1) = n.normalize(
            40,
            NormalizedTouchSample {
                touch_count: 1,
                points: [
                    NormalizedTouchPoint { x: 0, y: 0 },
                    NormalizedTouchPoint { x: 0, y: 0 },
                ],
                raw,
            },
        );
        assert_eq!(c1, 1);
        assert_eq!(p1, Some(NormalizedTouchPoint { x: 210, y: 310 }));

        // Once recent decoded-coordinate window expires, bit-only frames must not
        // keep latching the previous touch forever.
        let (c2, p2) = n.normalize(
            160,
            NormalizedTouchSample {
                touch_count: 1,
                points: [
                    NormalizedTouchPoint { x: 0, y: 0 },
                    NormalizedTouchPoint { x: 0, y: 0 },
                ],
                raw,
            },
        );
        assert_eq!(c2, 0);
        assert_eq!(p2, None);
    }

    #[test]
    fn raw_assist_extends_recent_decoded_touch_only_temporarily() {
        let mut n = TouchPresenceNormalizer::new();

        let (ms0, s0) = sample(0, 1, (200, 300), (0, 0), true);
        let (c0, p0) = n.normalize(ms0, s0);
        assert_eq!(c0, 1);
        assert_eq!(p0, Some(NormalizedTouchPoint { x: 200, y: 300 }));

        // Beyond short decoded grace, raw assist still keeps the touch alive.
        let (ms1, s1) = sample(24, 0, (0, 0), (0, 0), true);
        let (c1, p1) = n.normalize(ms1, s1);
        assert_eq!(c1, 1);
        assert_eq!(p1, Some(NormalizedTouchPoint { x: 200, y: 300 }));

        // Beyond raw-assist window, it must release even if raw remains noisy.
        let (ms2, s2) = sample(120, 0, (0, 0), (0, 0), true);
        let (c2, p2) = n.normalize(ms2, s2);
        assert_eq!(c2, 0);
        assert_eq!(p2, None);
    }

    #[test]
    fn idle_raw_noise_does_not_poison_next_decoded_primary() {
        let mut n = TouchPresenceNormalizer::new();

        // This mirrors the observed phantom point pattern: garbage (0,599) while
        // decoded touch_count is zero and raw status bit flickers.
        let (ms0, s0) = sample(0, 0, (0, 599), (0, 0), true);
        let (c0, p0) = n.normalize(ms0, s0);
        assert_eq!(c0, 0);
        assert_eq!(p0, None);

        // First valid decoded touch must use the real coordinate, not the stale
        // phantom corner point.
        let (ms1, s1) = sample(16, 1, (431, 353), (0, 599), true);
        let (c1, p1) = n.normalize(ms1, s1);
        assert_eq!(c1, 1);
        assert_eq!(p1, Some(NormalizedTouchPoint { x: 431, y: 353 }));
    }

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

