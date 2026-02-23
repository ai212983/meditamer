const TOUCH_DECODED_GRACE_MS: u64 = 16;
const TOUCH_RAW_ASSIST_GRACE_MS: u64 = 48;
const TOUCH_SLOT_STICKY_RADIUS_PX: i32 = 4;
const TOUCH_SLOT_SWITCH_MIN_TRAVEL_PX: i32 = 8;
const TOUCH_SLOT_SWITCH_MAX_TRAVEL_PX: i32 = 220;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct NormalizedTouchPoint {
    pub(crate) x: u16,
    pub(crate) y: u16,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NormalizedTouchSample {
    pub(crate) touch_count: u8,
    pub(crate) points: [NormalizedTouchPoint; 2],
    pub(crate) raw: [u8; 8],
}

pub(crate) struct TouchPresenceNormalizer {
    last_primary: Option<NormalizedTouchPoint>,
    last_decoded_present_ms: Option<u64>,
}

impl Default for TouchPresenceNormalizer {
    fn default() -> Self {
        Self::new()
    }
}

impl TouchPresenceNormalizer {
    pub(crate) fn new() -> Self {
        Self {
            last_primary: None,
            last_decoded_present_ms: None,
        }
    }

    pub(crate) fn normalize(
        &mut self,
        now_ms: u64,
        sample: NormalizedTouchSample,
    ) -> (u8, Option<NormalizedTouchPoint>) {
        let decoded_present = sample.touch_count > 0;
        let raw_present = sample.raw[7].count_ones() > 0;
        let recent_decoded_short = self.last_decoded_present_ms.map_or(false, |t_ms| {
            now_ms.saturating_sub(t_ms) <= TOUCH_DECODED_GRACE_MS
        });
        let recent_decoded_long = self.last_decoded_present_ms.map_or(false, |t_ms| {
            now_ms.saturating_sub(t_ms) <= TOUCH_RAW_ASSIST_GRACE_MS
        });
        // Raw bits are noisy when idle on some panels. They are only trusted as
        // an extension signal after a recently decoded touch sample.
        let continuity_present = recent_decoded_short || (raw_present && recent_decoded_long);

        let allow_continuity_fallback = decoded_present || continuity_present;
        let primary = self.select_primary(sample, decoded_present, allow_continuity_fallback);
        let normalized_present = (decoded_present || continuity_present) && primary.is_some();

        if decoded_present {
            self.last_decoded_present_ms = Some(now_ms);
        } else if !recent_decoded_long {
            self.last_decoded_present_ms = None;
        }

        if normalized_present {
            if let Some(point) = primary {
                self.last_primary = Some(point);
            }
        } else {
            self.last_primary = None;
        }

        (
            if normalized_present { 1 } else { 0 },
            if normalized_present { primary } else { None },
        )
    }

    fn select_primary(
        &self,
        sample: NormalizedTouchSample,
        decoded_present: bool,
        allow_continuity_fallback: bool,
    ) -> Option<NormalizedTouchPoint> {
        // Without a decoded touch in this sample, never accept incoming
        // coordinates as a new primary. Only preserve continuity.
        if !decoded_present {
            return if allow_continuity_fallback {
                self.last_primary
            } else {
                None
            };
        }

        let mut candidates = [NormalizedTouchPoint::default(); 2];
        let mut candidate_count = 0usize;

        for point in sample.points {
            if point.x == 0 && point.y == 0 {
                continue;
            }
            if candidate_count < candidates.len() {
                candidates[candidate_count] = point;
                candidate_count += 1;
            }
        }

        match candidate_count {
            0 => {
                if allow_continuity_fallback {
                    self.last_primary
                } else {
                    None
                }
            }
            1 => Some(candidates[0]),
            _ => {
                let a = candidates[0];
                let b = candidates[1];
                let prev = self.last_primary.unwrap_or(a);
                let dist_a = squared_distance(a, prev);
                let dist_b = squared_distance(b, prev);
                let raw_bit_count = (sample.raw[7].count_ones() as u8).min(2);
                let sticky_sq = squared_i32(TOUCH_SLOT_STICKY_RADIUS_PX);
                let switch_min_sq = squared_i32(TOUCH_SLOT_SWITCH_MIN_TRAVEL_PX);
                let switch_max_sq = squared_i32(TOUCH_SLOT_SWITCH_MAX_TRAVEL_PX);

                // With one real contact, some controllers may keep a stale
                // coordinate in one slot while updating the other. If one
                // candidate is effectively stuck at the previous point and the
                // other shows plausible travel, follow the moved one.
                if raw_bit_count <= 1 {
                    if dist_a <= sticky_sq && dist_b >= switch_min_sq && dist_b <= switch_max_sq {
                        return Some(b);
                    }
                    if dist_b <= sticky_sq && dist_a >= switch_min_sq && dist_a <= switch_max_sq {
                        return Some(a);
                    }
                }

                if dist_a <= dist_b {
                    Some(a)
                } else {
                    Some(b)
                }
            }
        }
    }
}

fn squared_i32(value: i32) -> u32 {
    value.saturating_mul(value) as u32
}

fn squared_distance(a: NormalizedTouchPoint, b: NormalizedTouchPoint) -> u32 {
    let dx = a.x as i32 - b.x as i32;
    let dy = a.y as i32 - b.y as i32;
    dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy)) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

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

        let (ms2, s2) = sample(32, 0, (0, 0), (0, 0), false);
        let (c2, _) = n.normalize(ms2, s2);
        assert_eq!(c2, 0);

        // Must stay released (no self-latching through "recent present" feedback).
        let (ms3, s3) = sample(64, 0, (0, 0), (0, 0), false);
        let (c3, _) = n.normalize(ms3, s3);
        assert_eq!(c3, 0);
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
        let (ms2, s2) = sample(80, 0, (0, 0), (0, 0), true);
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
        assert_eq!(p1, Some(NormalizedTouchPoint { x: 261, y: 260 }));
    }
}
