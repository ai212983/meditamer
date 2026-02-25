mod selection;

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
