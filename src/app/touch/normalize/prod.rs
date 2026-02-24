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

        (
            if normalized_present { 1 } else { 0 },
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
            let Some(prev) = self.last_primary else {
                return None;
            };
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
            if best.map_or(true, |(_, best_dist)| dist < best_dist) {
                best = Some((point, dist));
            }
        }
        let Some((candidate, dist)) = best else {
            return None;
        };
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

