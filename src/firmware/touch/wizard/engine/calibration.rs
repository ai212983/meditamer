use super::super::super::types::TouchEvent;
use super::swipe::squared_distance_i32;
use super::*;

impl TouchCalibration {
    pub(super) fn from_observations(
        observations: [Option<TapObservation>; CALIBRATION_CORNER_COUNT],
    ) -> Option<Self> {
        let top_left = observations[0]?;
        let top_right = observations[1]?;
        let bottom_right = observations[2]?;
        let bottom_left = observations[3]?;

        let observed_left = average(top_left.observed.x, bottom_left.observed.x);
        let observed_right = average(top_right.observed.x, bottom_right.observed.x);
        let observed_top = average(top_left.observed.y, top_right.observed.y);
        let observed_bottom = average(bottom_left.observed.y, bottom_right.observed.y);
        if observed_right.saturating_sub(observed_left) < CALIBRATION_CAPTURE_RADIUS_PX
            || observed_bottom.saturating_sub(observed_top) < CALIBRATION_CAPTURE_RADIUS_PX
        {
            return None;
        }

        Some(Self {
            observed_left,
            observed_right,
            observed_top,
            observed_bottom,
            target_left: average(top_left.target.x, bottom_left.target.x),
            target_right: average(top_right.target.x, bottom_right.target.x),
            target_top: average(top_left.target.y, top_right.target.y),
            target_bottom: average(bottom_left.target.y, bottom_right.target.y),
        })
    }

    fn apply(self, point: SwipePoint, width: i32, height: i32) -> SwipePoint {
        SwipePoint {
            x: map_axis(
                point.x,
                self.observed_left,
                self.observed_right,
                self.target_left,
                self.target_right,
            )
            .clamp(0, width.saturating_sub(1).max(0)),
            y: map_axis(
                point.y,
                self.observed_top,
                self.observed_bottom,
                self.target_top,
                self.target_bottom,
            )
            .clamp(0, height.saturating_sub(1).max(0)),
        }
    }
}

impl TouchCalibrationWizard {
    pub(super) fn calibration_targets(width: i32, height: i32) -> [SwipePoint; 4] {
        let max_x = width.saturating_sub(1).max(0);
        let max_y = height.saturating_sub(1).max(0);
        let left = CALIBRATION_MARGIN_PX.min(max_x);
        let right = max_x.saturating_sub(CALIBRATION_MARGIN_PX).max(left);
        let top = CALIBRATION_MARGIN_PX.min(max_y);
        let bottom = max_y.saturating_sub(CALIBRATION_MARGIN_PX).max(top);
        [
            SwipePoint { x: left, y: top },
            SwipePoint { x: right, y: top },
            SwipePoint {
                x: right,
                y: bottom,
            },
            SwipePoint { x: left, y: bottom },
        ]
    }

    pub(super) fn start_calibration(&mut self) {
        self.phase = WizardPhase::Calibrate;
        self.hint = "Touch each corner dot.";
        self.calibration_observations = [None; CALIBRATION_CORNER_COUNT];
        // A new run replaces the previous calibration. Keeping the old transform
        // while collecting replacement observations makes an invalid retry look
        // successful when the wizard returns to the precision menu.
        self.calibration = None;
        self.calibration_pending_return = false;
        self.last_test_touch = None;
    }

    pub(super) fn record_calibration_touch(
        &mut self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    ) -> bool {
        let targets = Self::calibration_targets(width, height);
        let mut nearest: Option<(usize, i32)> = None;
        for (index, target) in targets.iter().enumerate() {
            if self.calibration_observations[index].is_some() {
                continue;
            }
            let distance = squared_distance_i32(x, y, target.x, target.y);
            if nearest.is_none_or(|(_, nearest_distance)| distance < nearest_distance) {
                nearest = Some((index, distance));
            }
        }

        let Some((index, distance)) = nearest else {
            return false;
        };
        if distance > CALIBRATION_CAPTURE_RADIUS_PX * CALIBRATION_CAPTURE_RADIUS_PX {
            self.hint = "Touch an unmarked corner dot.";
            return true;
        }

        let target = targets[index];
        self.calibration_observations[index] = Some(TapObservation {
            target,
            observed: SwipePoint { x, y },
        });
        esp_println::println!(
            "touch_calibration: corner={} tx={} ty={} x={} y={} dx={} dy={}",
            index,
            target.x,
            target.y,
            x,
            y,
            x - target.x,
            y - target.y,
        );

        let complete = self.calibration_observations.iter().all(Option::is_some);
        if complete {
            self.calibration = TouchCalibration::from_observations(self.calibration_observations);
            self.calibration_pending_return = true;
            self.hint = if self.calibration.is_some() {
                "Calibration complete. Release to return."
            } else {
                "Calibration invalid. Release and retry."
            };
        } else {
            self.hint = "Corner recorded. Touch the remaining dots.";
        }
        true
    }

    pub(super) fn calibrated_event(
        &self,
        mut event: TouchEvent,
        width: i32,
        height: i32,
    ) -> TouchEvent {
        let Some(calibration) = self.calibration else {
            return event;
        };
        let point = calibration.apply(
            SwipePoint {
                x: event.x as i32,
                y: event.y as i32,
            },
            width,
            height,
        );
        let start = calibration.apply(
            SwipePoint {
                x: event.start_x as i32,
                y: event.start_y as i32,
            },
            width,
            height,
        );
        let contact = calibration.apply(
            SwipePoint {
                x: event.contact_x as i32,
                y: event.contact_y as i32,
            },
            width,
            height,
        );
        event.x = point.x as u16;
        event.y = point.y as u16;
        event.contact_x = contact.x as u16;
        event.contact_y = contact.y as u16;
        event.start_x = start.x as u16;
        event.start_y = start.y as u16;
        event
    }

    pub(super) fn calibration_count(&self) -> usize {
        self.calibration_observations
            .iter()
            .filter(|observation| observation.is_some())
            .count()
    }
}

fn average(a: i32, b: i32) -> i32 {
    a.saturating_add(b) / 2
}

fn map_axis(value: i32, from_min: i32, from_max: i32, to_min: i32, to_max: i32) -> i32 {
    let from_span = from_max.saturating_sub(from_min);
    if from_span <= 0 {
        return value;
    }
    let numerator = i64::from(value.saturating_sub(from_min))
        .saturating_mul(i64::from(to_max.saturating_sub(to_min)));
    i32::try_from(numerator / i64::from(from_span))
        .unwrap_or(if numerator.is_negative() {
            i32::MIN
        } else {
            i32::MAX
        })
        .saturating_add(to_min)
}
