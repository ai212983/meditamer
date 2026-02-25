const TOUCH_DECODED_GRACE_MS: u64 = 56;
const TOUCH_RAW_ASSIST_GRACE_MS: u64 = 96;
const TOUCH_SLOT_STICKY_RADIUS_PX: i32 = 16;
const TOUCH_SLOT_HOLD_RADIUS_PX: i32 = 30;
const TOUCH_SLOT_SWITCH_MIN_TRAVEL_PX: i32 = 8;
const TOUCH_SLOT_SWITCH_MAX_TRAVEL_PX: i32 = 500;
const TOUCH_SLOT_DIRECTIONAL_MAX_TRAVEL_PX: i32 = 620;
const TOUCH_SLOT_DIRECTIONAL_DOT_MARGIN: i32 = 128;
const TOUCH_SLOT_AXIS_DOMINANCE_X100: i32 = 180;
const TOUCH_CONTINUITY_MAX_JUMP_PX: i32 = 320;
const TOUCH_MEDIAN_WINDOW: usize = 3;
const TOUCH_MEDIAN_BYPASS_PX: i32 = 20;
const TOUCH_DEJITTER_RADIUS_PX: i32 = 2;
const TOUCH_OUTLIER_MIN_STEP_PX: i32 = 420;
const TOUCH_OUTLIER_STEP_PX_PER_MS_X100: i32 = 800; // 8 px/ms
const TOUCH_OUTLIER_CONFIRM_RADIUS_PX: i32 = 40;

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
    recent_primary_points: [NormalizedTouchPoint; TOUCH_MEDIAN_WINDOW],
    recent_primary_len: usize,
    last_filtered_primary: Option<NormalizedTouchPoint>,
    last_filtered_ms: Option<u64>,
    last_motion_dx: i32,
    last_motion_dy: i32,
    pending_outlier: Option<NormalizedTouchPoint>,
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
            recent_primary_points: [NormalizedTouchPoint::default(); TOUCH_MEDIAN_WINDOW],
            recent_primary_len: 0,
            last_filtered_primary: None,
            last_filtered_ms: None,
            last_motion_dx: 0,
            last_motion_dy: 0,
            pending_outlier: None,
        }
    }

    pub(crate) fn normalize(
        &mut self,
        now_ms: u64,
        sample: NormalizedTouchSample,
    ) -> (u8, Option<NormalizedTouchPoint>) {
        // Treat decoded presence as "controller reported a touch AND at least one
        // decoded coordinate is populated". Status bits can stay asserted while
        // coordinate slots are empty/noisy; those frames should use continuity
        // fallback instead of creating self-latching decoded presence.
        let decoded_present =
            sample.touch_count > 0 && sample_has_decoded_coordinate(sample.points);
        let raw_present = sample.raw[7].count_ones() > 0;
        let recent_decoded_short = self
            .last_decoded_present_ms
            .is_some_and(|t_ms| now_ms.saturating_sub(t_ms) <= TOUCH_DECODED_GRACE_MS);
        let recent_decoded_long = self
            .last_decoded_present_ms
            .is_some_and(|t_ms| now_ms.saturating_sub(t_ms) <= TOUCH_RAW_ASSIST_GRACE_MS);
        // Raw bits are noisy when idle on some panels. They are only trusted as
        // an extension signal after a recently decoded touch sample.
        let continuity_present = recent_decoded_short || (raw_present && recent_decoded_long);

        let allow_continuity_fallback = decoded_present || continuity_present;
        let mut primary = self.select_primary(sample, decoded_present, allow_continuity_fallback);
        let normalized_present = (decoded_present || continuity_present) && primary.is_some();

        if decoded_present {
            self.last_decoded_present_ms = Some(now_ms);
        } else if !recent_decoded_long {
            self.last_decoded_present_ms = None;
        }

        if normalized_present {
            if let Some(point) = primary {
                let filtered = self.filter_primary(now_ms, point);
                self.last_primary = Some(filtered);
                primary = Some(filtered);
            }
        } else {
            self.last_primary = None;
            self.recent_primary_len = 0;
            self.last_filtered_primary = None;
            self.last_filtered_ms = None;
            self.last_motion_dx = 0;
            self.last_motion_dy = 0;
            self.pending_outlier = None;
        }

        let normalized_count = if normalized_present {
            if sample.touch_count > 1 && decoded_coordinate_count(sample.points) > 1 {
                2
            } else {
                1
            }
        } else {
            0
        };

        (
            normalized_count,
            if normalized_present { primary } else { None },
        )
    }

    fn filter_primary(&mut self, now_ms: u64, point: NormalizedTouchPoint) -> NormalizedTouchPoint {
        let prev_filtered = self.last_filtered_primary;
        let outlier_filtered = self.suppress_outlier_step(now_ms, point);
        let median_filtered = self.median_filter(outlier_filtered);
        let dejittered = self.dejitter(median_filtered);
        if let Some(prev) = prev_filtered {
            self.last_motion_dx = dejittered.x as i32 - prev.x as i32;
            self.last_motion_dy = dejittered.y as i32 - prev.y as i32;
        } else {
            self.last_motion_dx = 0;
            self.last_motion_dy = 0;
        }
        self.last_filtered_primary = Some(dejittered);
        self.last_filtered_ms = Some(now_ms);
        dejittered
    }

    fn suppress_outlier_step(
        &mut self,
        now_ms: u64,
        point: NormalizedTouchPoint,
    ) -> NormalizedTouchPoint {
        let Some(prev) = self.last_filtered_primary else {
            return point;
        };
        let Some(prev_ms) = self.last_filtered_ms else {
            return point;
        };
        let dt_ms = now_ms.saturating_sub(prev_ms).max(1);
        let allowed_step_px = TOUCH_OUTLIER_MIN_STEP_PX
            .saturating_add(((TOUCH_OUTLIER_STEP_PX_PER_MS_X100 as u64 * dt_ms) / 100) as i32);
        if squared_distance(point, prev) > squared_i32(allowed_step_px) {
            if let Some(pending) = self.pending_outlier {
                if squared_distance(point, pending) <= squared_i32(TOUCH_OUTLIER_CONFIRM_RADIUS_PX)
                {
                    self.pending_outlier = None;
                    return point;
                }
            }
            self.pending_outlier = Some(point);
            prev
        } else {
            self.pending_outlier = None;
            point
        }
    }

    fn median_filter(&mut self, point: NormalizedTouchPoint) -> NormalizedTouchPoint {
        self.push_recent_primary(point);
        if let Some(prev) = self.last_filtered_primary {
            // Avoid damping legitimate gesture starts. Median is helpful for
            // small jitter spikes, but large directional steps should pass
            // through immediately.
            if squared_distance(point, prev) >= squared_i32(TOUCH_MEDIAN_BYPASS_PX) {
                return point;
            }
        }
        if self.recent_primary_len < TOUCH_MEDIAN_WINDOW {
            return point;
        }

        let a = self.recent_primary_points[TOUCH_MEDIAN_WINDOW - 3];
        let b = self.recent_primary_points[TOUCH_MEDIAN_WINDOW - 2];
        let c = self.recent_primary_points[TOUCH_MEDIAN_WINDOW - 1];
        NormalizedTouchPoint {
            x: median3_u16(a.x, b.x, c.x),
            y: median3_u16(a.y, b.y, c.y),
        }
    }

    fn push_recent_primary(&mut self, point: NormalizedTouchPoint) {
        if self.recent_primary_len < TOUCH_MEDIAN_WINDOW {
            self.recent_primary_points[self.recent_primary_len] = point;
            self.recent_primary_len += 1;
            return;
        }
        self.recent_primary_points[0] = self.recent_primary_points[1];
        self.recent_primary_points[1] = self.recent_primary_points[2];
        self.recent_primary_points[2] = point;
    }

    fn dejitter(&self, point: NormalizedTouchPoint) -> NormalizedTouchPoint {
        let Some(prev) = self.last_filtered_primary else {
            return point;
        };
        if squared_distance(point, prev) <= squared_i32(TOUCH_DEJITTER_RADIUS_PX) {
            prev
        } else {
            point
        }
    }

    fn select_primary(
        &self,
        sample: NormalizedTouchSample,
        decoded_present: bool,
        allow_continuity_fallback: bool,
    ) -> Option<NormalizedTouchPoint> {
        // Without a decoded touch in this sample, never accept incoming
        // coordinates as a new primary except for plausible continuity updates.
        if !decoded_present {
            if !allow_continuity_fallback {
                return None;
            }
            let prev = self.last_primary?;
            let continuity = self.select_continuity_primary(sample, prev);
            return Some(continuity.unwrap_or(prev));
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
                let has_prev = self.last_primary.is_some();
                let dist_a = squared_distance(a, prev);
                let dist_b = squared_distance(b, prev);
                let raw_bit_count = (sample.raw[7].count_ones() as u8).min(2);
                let sticky_sq = squared_i32(TOUCH_SLOT_STICKY_RADIUS_PX);
                let switch_min_sq = squared_i32(TOUCH_SLOT_SWITCH_MIN_TRAVEL_PX);
                let switch_max_sq = squared_i32(TOUCH_SLOT_SWITCH_MAX_TRAVEL_PX);
                let directional_max_sq = squared_i32(TOUCH_SLOT_DIRECTIONAL_MAX_TRAVEL_PX);
                let hold_sq = squared_i32(TOUCH_SLOT_HOLD_RADIUS_PX);

                // With one real contact, some controllers may keep a stale
                // coordinate in one slot while updating the other. If one
                // candidate is effectively stuck at the previous point and the
                // other shows plausible travel, follow the moved one.
                if has_prev && raw_bit_count <= 1 {
                    if dist_a <= sticky_sq
                        && dist_b >= switch_min_sq
                        && dist_b <= switch_max_sq
                        && is_axis_dominant_step(prev, b)
                    {
                        return Some(b);
                    }
                    if dist_b <= sticky_sq
                        && dist_a >= switch_min_sq
                        && dist_a <= switch_max_sq
                        && is_axis_dominant_step(prev, a)
                    {
                        return Some(a);
                    }
                    if dist_a <= hold_sq
                        && dist_b >= switch_min_sq
                        && dist_b <= directional_max_sq
                        && is_axis_dominant_step(prev, b)
                    {
                        return Some(b);
                    }
                    if dist_b <= hold_sq
                        && dist_a >= switch_min_sq
                        && dist_a <= directional_max_sq
                        && is_axis_dominant_step(prev, a)
                    {
                        return Some(a);
                    }

                    let motion_sq = self.last_motion_dx.saturating_mul(self.last_motion_dx)
                        + self.last_motion_dy.saturating_mul(self.last_motion_dy);
                    if motion_sq >= switch_min_sq as i32 {
                        let dax = a.x as i32 - prev.x as i32;
                        let day = a.y as i32 - prev.y as i32;
                        let dbx = b.x as i32 - prev.x as i32;
                        let dby = b.y as i32 - prev.y as i32;
                        let dot_a = dax.saturating_mul(self.last_motion_dx)
                            + day.saturating_mul(self.last_motion_dy);
                        let dot_b = dbx.saturating_mul(self.last_motion_dx)
                            + dby.saturating_mul(self.last_motion_dy);
                        if dist_a <= directional_max_sq
                            && dot_a > dot_b.saturating_add(TOUCH_SLOT_DIRECTIONAL_DOT_MARGIN)
                            && dot_a > 0
                        {
                            return Some(a);
                        }
                        if dist_b <= directional_max_sq
                            && dot_b > dot_a.saturating_add(TOUCH_SLOT_DIRECTIONAL_DOT_MARGIN)
                            && dot_b > 0
                        {
                            return Some(b);
                        }
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

    fn select_continuity_primary(
        &self,
        sample: NormalizedTouchSample,
        previous: NormalizedTouchPoint,
    ) -> Option<NormalizedTouchPoint> {
        let mut best: Option<(NormalizedTouchPoint, u32)> = None;
        for point in sample.points {
            if point.x == 0 && point.y == 0 {
                continue;
            }
            let dist = squared_distance(point, previous);
            if best.is_none_or(|(_, best_dist)| dist < best_dist) {
                best = Some((point, dist));
            }
        }
        let (candidate, dist) = best?;
        let max_jump_sq = squared_i32(TOUCH_CONTINUITY_MAX_JUMP_PX);
        if dist <= max_jump_sq {
            Some(candidate)
        } else {
            None
        }
    }
}

fn squared_i32(value: i32) -> u32 {
    value.saturating_mul(value) as u32
}

fn sample_has_decoded_coordinate(points: [NormalizedTouchPoint; 2]) -> bool {
    points.iter().any(|point| point.x != 0 || point.y != 0)
}

fn decoded_coordinate_count(points: [NormalizedTouchPoint; 2]) -> u8 {
    points
        .iter()
        .filter(|point| point.x != 0 || point.y != 0)
        .count() as u8
}

fn median3_u16(a: u16, b: u16, c: u16) -> u16 {
    let mut values = [a, b, c];
    values.sort_unstable();
    values[1]
}

fn squared_distance(a: NormalizedTouchPoint, b: NormalizedTouchPoint) -> u32 {
    let dx = a.x as i32 - b.x as i32;
    let dy = a.y as i32 - b.y as i32;
    dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy)) as u32
}

fn is_axis_dominant_step(from: NormalizedTouchPoint, to: NormalizedTouchPoint) -> bool {
    let dx = (to.x as i32 - from.x as i32).abs();
    let dy = (to.y as i32 - from.y as i32).abs();
    let major = dx.max(dy);
    let minor = dx.min(dy);
    major >= TOUCH_SLOT_SWITCH_MIN_TRAVEL_PX
        && (minor <= TOUCH_SLOT_HOLD_RADIUS_PX
            || major * 100 >= minor * TOUCH_SLOT_AXIS_DOMINANCE_X100)
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

    #[test]
    fn dual_contact_reports_multitouch_count_when_both_slots_are_active() {
        let mut n = TouchPresenceNormalizer::new();

        let _ = n.normalize(
            0,
            NormalizedTouchSample {
                touch_count: 1,
                points: [
                    NormalizedTouchPoint { x: 220, y: 220 },
                    NormalizedTouchPoint { x: 0, y: 0 },
                ],
                raw: [0, 0, 0, 0, 0, 0, 0, 0x01],
            },
        );

        let (count, point) = n.normalize(
            8,
            NormalizedTouchSample {
                touch_count: 2,
                points: [
                    NormalizedTouchPoint { x: 222, y: 221 },
                    NormalizedTouchPoint { x: 300, y: 320 },
                ],
                raw: [0, 0, 0, 0, 0, 0, 0, 0x03],
            },
        );
        assert_eq!(count, 2);
        assert_eq!(point, Some(NormalizedTouchPoint { x: 222, y: 221 }));
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
}
