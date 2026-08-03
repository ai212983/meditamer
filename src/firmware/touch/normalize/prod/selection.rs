use super::*;

#[derive(Clone, Copy)]
enum CandidateSet {
    Empty,
    Single(NormalizedTouchPoint),
    Pair(CandidatePair),
}

#[derive(Clone, Copy)]
struct CandidatePair {
    a: NormalizedTouchPoint,
    b: NormalizedTouchPoint,
}

#[derive(Clone, Copy)]
struct CandidateDistances {
    pair: CandidatePair,
    previous: NormalizedTouchPoint,
    a: u32,
    b: u32,
}

#[derive(Clone, Copy)]
struct MotionVector {
    dx: i32,
    dy: i32,
}

impl TouchPresenceNormalizer {
    pub(super) fn select_primary(
        &self,
        sample: NormalizedTouchSample,
        decoded_present: bool,
        allow_continuity_fallback: bool,
    ) -> Option<NormalizedTouchPoint> {
        // Without a decoded touch in this sample, never accept incoming
        // coordinates as a new primary except for plausible continuity updates.
        if !decoded_present {
            return self.select_undecoded_primary(sample, allow_continuity_fallback);
        }

        match classify_candidates(sample.points) {
            CandidateSet::Empty => {
                if allow_continuity_fallback {
                    self.last_primary
                } else {
                    None
                }
            }
            CandidateSet::Single(point) => Some(point),
            CandidateSet::Pair(pair) => Some(self.resolve_candidate_pair(sample, pair)),
        }
    }

    fn select_undecoded_primary(
        &self,
        sample: NormalizedTouchSample,
        allow_continuity_fallback: bool,
    ) -> Option<NormalizedTouchPoint> {
        if !allow_continuity_fallback {
            return None;
        }
        let previous = self.last_primary?;
        Some(
            self.select_continuity_primary(sample, previous)
                .unwrap_or(previous),
        )
    }

    fn resolve_candidate_pair(
        &self,
        sample: NormalizedTouchSample,
        pair: CandidatePair,
    ) -> NormalizedTouchPoint {
        let previous = self.last_primary.unwrap_or(pair.a);
        let candidates = CandidateDistances::new(pair, previous);
        let raw_bit_count = (sample.raw[7].count_ones() as u8).min(2);

        if self.last_primary.is_some() && raw_bit_count <= 1 {
            if let Some(point) = candidates.select_stale_slot_handoff() {
                return point;
            }
            let motion = MotionVector {
                dx: self.last_motion_dx,
                dy: self.last_motion_dy,
            };
            if let Some(point) = candidates.select_directional_continuation(motion) {
                return point;
            }
        }

        candidates.nearest()
    }

    pub(super) fn select_continuity_primary(
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

fn classify_candidates(points: [NormalizedTouchPoint; 2]) -> CandidateSet {
    let [a, b] = points;
    match (is_populated(a), is_populated(b)) {
        (false, false) => CandidateSet::Empty,
        (true, false) => CandidateSet::Single(a),
        (false, true) => CandidateSet::Single(b),
        (true, true) => CandidateSet::Pair(CandidatePair { a, b }),
    }
}

fn is_populated(point: NormalizedTouchPoint) -> bool {
    point.x != 0 || point.y != 0
}

impl CandidateDistances {
    fn new(pair: CandidatePair, previous: NormalizedTouchPoint) -> Self {
        Self {
            pair,
            previous,
            a: squared_distance(pair.a, previous),
            b: squared_distance(pair.b, previous),
        }
    }

    // With one real contact, some controllers may keep a stale coordinate in
    // one slot while updating the other. Follow the moved slot when the other
    // remains within either the strict sticky radius or the wider hold radius.
    fn select_stale_slot_handoff(self) -> Option<NormalizedTouchPoint> {
        let switch_max_sq = squared_i32(TOUCH_SLOT_SWITCH_MAX_TRAVEL_PX);
        let directional_max_sq = squared_i32(TOUCH_SLOT_DIRECTIONAL_MAX_TRAVEL_PX);

        if follows_stale_slot(
            self.previous,
            self.a,
            self.b,
            squared_i32(TOUCH_SLOT_STICKY_RADIUS_PX),
            switch_max_sq,
            self.pair.b,
        ) {
            return Some(self.pair.b);
        }
        if follows_stale_slot(
            self.previous,
            self.b,
            self.a,
            squared_i32(TOUCH_SLOT_STICKY_RADIUS_PX),
            switch_max_sq,
            self.pair.a,
        ) {
            return Some(self.pair.a);
        }
        if follows_stale_slot(
            self.previous,
            self.a,
            self.b,
            squared_i32(TOUCH_SLOT_HOLD_RADIUS_PX),
            directional_max_sq,
            self.pair.b,
        ) {
            return Some(self.pair.b);
        }
        if follows_stale_slot(
            self.previous,
            self.b,
            self.a,
            squared_i32(TOUCH_SLOT_HOLD_RADIUS_PX),
            directional_max_sq,
            self.pair.a,
        ) {
            return Some(self.pair.a);
        }
        None
    }

    fn select_directional_continuation(self, motion: MotionVector) -> Option<NormalizedTouchPoint> {
        if !motion.is_significant() {
            return None;
        }

        let dot_a = motion.dot_from(self.previous, self.pair.a);
        let dot_b = motion.dot_from(self.previous, self.pair.b);
        let directional_max_sq = squared_i32(TOUCH_SLOT_DIRECTIONAL_MAX_TRAVEL_PX);
        if is_directionally_preferred(self.a, dot_a, dot_b, directional_max_sq) {
            return Some(self.pair.a);
        }
        if is_directionally_preferred(self.b, dot_b, dot_a, directional_max_sq) {
            return Some(self.pair.b);
        }
        None
    }

    fn nearest(self) -> NormalizedTouchPoint {
        if self.a <= self.b {
            self.pair.a
        } else {
            self.pair.b
        }
    }
}

impl MotionVector {
    fn is_significant(self) -> bool {
        let motion_sq = self.dx.saturating_mul(self.dx) + self.dy.saturating_mul(self.dy);
        motion_sq >= squared_i32(TOUCH_SLOT_SWITCH_MIN_TRAVEL_PX) as i32
    }

    fn dot_from(self, from: NormalizedTouchPoint, to: NormalizedTouchPoint) -> i32 {
        let dx = to.x as i32 - from.x as i32;
        let dy = to.y as i32 - from.y as i32;
        dx.saturating_mul(self.dx) + dy.saturating_mul(self.dy)
    }
}

fn follows_stale_slot(
    previous: NormalizedTouchPoint,
    stale_distance: u32,
    moving_distance: u32,
    stale_max_distance: u32,
    moving_max_distance: u32,
    moving: NormalizedTouchPoint,
) -> bool {
    stale_distance <= stale_max_distance
        && moving_distance >= squared_i32(TOUCH_SLOT_SWITCH_MIN_TRAVEL_PX)
        && moving_distance <= moving_max_distance
        && is_axis_dominant_step(previous, moving)
}

fn is_directionally_preferred(
    distance: u32,
    dot: i32,
    other_dot: i32,
    directional_max_sq: u32,
) -> bool {
    distance <= directional_max_sq
        && dot > other_dot.saturating_add(TOUCH_SLOT_DIRECTIONAL_DOT_MARGIN)
        && dot > 0
}
