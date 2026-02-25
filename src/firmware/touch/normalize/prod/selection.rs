use super::*;

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
