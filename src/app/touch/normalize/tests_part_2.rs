    #[test]
    fn single_contact_ignores_implausibly_large_slot_jump() {
        let mut n = TouchPresenceNormalizer::new();

        let (ms0, s0) = sample(0, 1, (260, 260), (0, 0), true);
        let (c0, p0) = n.normalize(ms0, s0);
        assert_eq!(c0, 1);
        assert_eq!(p0, Some(NormalizedTouchPoint { x: 260, y: 260 }));

        // Far candidate exceeds jump cap; keep stable candidate.
        let mut raw = [0u8; 8];
        raw[7] = 0x01;
        let (c1, p1) = n.normalize(
            8,
            NormalizedTouchSample {
                touch_count: 1,
                points: [
                    NormalizedTouchPoint { x: 261, y: 260 },
                    NormalizedTouchPoint { x: 599, y: 0 },
                ],
                raw,
            },
        );
        assert_eq!(c1, 1);
        assert_eq!(p1, Some(NormalizedTouchPoint { x: 260, y: 260 }));
    }

    #[test]
    fn median_filter_reduces_single_frame_spike() {
        let mut n = TouchPresenceNormalizer::new();

        let (ms0, s0) = sample(0, 1, (100, 100), (0, 0), true);
        let (_, p0) = n.normalize(ms0, s0);
        assert_eq!(p0, Some(NormalizedTouchPoint { x: 100, y: 100 }));

        let (ms1, s1) = sample(8, 1, (130, 100), (0, 0), true);
        let (_, p1) = n.normalize(ms1, s1);
        assert_eq!(p1, Some(NormalizedTouchPoint { x: 130, y: 100 }));

        // Small single-frame spike should be damped by temporal median.
        let (ms2, s2) = sample(16, 1, (145, 100), (0, 0), true);
        let (_, p2) = n.normalize(ms2, s2);
        assert_eq!(p2, Some(NormalizedTouchPoint { x: 130, y: 100 }));
    }

    #[test]
    fn outlier_step_is_suppressed() {
        let mut n = TouchPresenceNormalizer::new();

        let _ = n.normalize(
            0,
            NormalizedTouchSample {
                touch_count: 1,
                points: [
                    NormalizedTouchPoint { x: 120, y: 220 },
                    NormalizedTouchPoint { x: 0, y: 0 },
                ],
                raw: [0, 0, 0, 0, 0, 0, 0, 0x01],
            },
        );
        let _ = n.normalize(
            8,
            NormalizedTouchSample {
                touch_count: 1,
                points: [
                    NormalizedTouchPoint { x: 150, y: 220 },
                    NormalizedTouchPoint { x: 0, y: 0 },
                ],
                raw: [0, 0, 0, 0, 0, 0, 0, 0x01],
            },
        );
        let (_, stable) = n.normalize(
            16,
            NormalizedTouchSample {
                touch_count: 1,
                points: [
                    NormalizedTouchPoint { x: 180, y: 220 },
                    NormalizedTouchPoint { x: 0, y: 0 },
                ],
                raw: [0, 0, 0, 0, 0, 0, 0, 0x01],
            },
        );
        assert_eq!(stable, Some(NormalizedTouchPoint { x: 180, y: 220 }));

        // Implausibly large one-frame jump must be rejected.
        let (_, outlier_filtered) = n.normalize(
            24,
            NormalizedTouchSample {
                touch_count: 1,
                points: [
                    NormalizedTouchPoint { x: 900, y: 700 },
                    NormalizedTouchPoint { x: 0, y: 0 },
                ],
                raw: [0, 0, 0, 0, 0, 0, 0, 0x01],
            },
        );
        assert_eq!(
            outlier_filtered,
            Some(NormalizedTouchPoint { x: 180, y: 220 })
        );
    }

    #[test]
    fn repeated_large_step_is_accepted_after_single_suppression() {
        let mut n = TouchPresenceNormalizer::new();

        let _ = n.normalize(
            0,
            NormalizedTouchSample {
                touch_count: 1,
                points: [
                    NormalizedTouchPoint { x: 120, y: 220 },
                    NormalizedTouchPoint { x: 0, y: 0 },
                ],
                raw: [0, 0, 0, 0, 0, 0, 0, 0x01],
            },
        );
        let _ = n.normalize(
            8,
            NormalizedTouchSample {
                touch_count: 1,
                points: [
                    NormalizedTouchPoint { x: 150, y: 220 },
                    NormalizedTouchPoint { x: 0, y: 0 },
                ],
                raw: [0, 0, 0, 0, 0, 0, 0, 0x01],
            },
        );
        let _ = n.normalize(
            16,
            NormalizedTouchSample {
                touch_count: 1,
                points: [
                    NormalizedTouchPoint { x: 180, y: 220 },
                    NormalizedTouchPoint { x: 0, y: 0 },
                ],
                raw: [0, 0, 0, 0, 0, 0, 0, 0x01],
            },
        );

        let (_, first_large_jump) = n.normalize(
            24,
            NormalizedTouchSample {
                touch_count: 1,
                points: [
                    NormalizedTouchPoint { x: 760, y: 220 },
                    NormalizedTouchPoint { x: 0, y: 0 },
                ],
                raw: [0, 0, 0, 0, 0, 0, 0, 0x01],
            },
        );
        assert_eq!(
            first_large_jump,
            Some(NormalizedTouchPoint { x: 180, y: 220 })
        );

        let (_, confirmed_jump) = n.normalize(
            32,
            NormalizedTouchSample {
                touch_count: 1,
                points: [
                    NormalizedTouchPoint { x: 762, y: 221 },
                    NormalizedTouchPoint { x: 0, y: 0 },
                ],
                raw: [0, 0, 0, 0, 0, 0, 0, 0x01],
            },
        );
        assert_eq!(
            confirmed_jump,
            Some(NormalizedTouchPoint { x: 762, y: 221 })
        );
    }

    #[test]
    fn single_contact_dual_slot_prefers_directional_continuity() {
        let mut n = TouchPresenceNormalizer::new();

        let _ = n.normalize(
            0,
            NormalizedTouchSample {
                touch_count: 1,
                points: [
                    NormalizedTouchPoint { x: 250, y: 190 },
                    NormalizedTouchPoint { x: 0, y: 0 },
                ],
                raw: [0, 0, 0, 0, 0, 0, 0, 0x01],
            },
        );
        let (_, p1) = n.normalize(
            8,
            NormalizedTouchSample {
                touch_count: 1,
                points: [
                    NormalizedTouchPoint { x: 250, y: 190 },
                    NormalizedTouchPoint { x: 260, y: 260 },
                ],
                raw: [0, 0, 0, 0, 0, 0, 0, 0x01],
            },
        );
        assert_eq!(p1, Some(NormalizedTouchPoint { x: 260, y: 260 }));

        let (_, p2) = n.normalize(
            16,
            NormalizedTouchSample {
                touch_count: 1,
                points: [
                    NormalizedTouchPoint { x: 250, y: 190 },
                    NormalizedTouchPoint { x: 260, y: 340 },
                ],
                raw: [0, 0, 0, 0, 0, 0, 0, 0x01],
            },
        );
        assert_eq!(p2, Some(NormalizedTouchPoint { x: 260, y: 340 }));
    }

    #[test]
    fn dejitter_holds_subpixel_motion() {
        let mut n = TouchPresenceNormalizer::new();

        let _ = n.normalize(
            0,
            NormalizedTouchSample {
                touch_count: 1,
                points: [
                    NormalizedTouchPoint { x: 400, y: 300 },
                    NormalizedTouchPoint { x: 0, y: 0 },
                ],
                raw: [0, 0, 0, 0, 0, 0, 0, 0x01],
            },
        );
        let _ = n.normalize(
            8,
            NormalizedTouchSample {
                touch_count: 1,
                points: [
                    NormalizedTouchPoint { x: 400, y: 300 },
                    NormalizedTouchPoint { x: 0, y: 0 },
                ],
                raw: [0, 0, 0, 0, 0, 0, 0, 0x01],
            },
        );
        let _ = n.normalize(
            16,
            NormalizedTouchSample {
                touch_count: 1,
                points: [
                    NormalizedTouchPoint { x: 401, y: 301 },
                    NormalizedTouchPoint { x: 0, y: 0 },
                ],
                raw: [0, 0, 0, 0, 0, 0, 0, 0x01],
            },
        );

        // Still within tiny motion radius after filtering.
        let (_, p) = n.normalize(
            24,
            NormalizedTouchSample {
                touch_count: 1,
                points: [
                    NormalizedTouchPoint { x: 402, y: 301 },
                    NormalizedTouchPoint { x: 0, y: 0 },
                ],
                raw: [0, 0, 0, 0, 0, 0, 0, 0x01],
            },
        );
        assert_eq!(p, Some(NormalizedTouchPoint { x: 400, y: 300 }));
    }
