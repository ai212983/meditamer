//! Timestamped rotation commands, continuous commanded angle, and angular
//! velocity, per the plan's "Rotation, flow, and time model".
//!
//! Angle motion is driven by the fixed-step physics ticker, not by wall
//! time: [`RotationModel::tick`] always advances by exactly one physics
//! step's `dt`, so a fixed command trace and a fixed number of ticks produce
//! the same angle and angular velocity regardless of render cadence -- the
//! render task only ever reads the latest state. `at`/[`TimestampUs`] exists
//! solely to order and de-duplicate *input* events (a `KEY` press arriving
//! out of order, or twice), never to drive the integration itself.

use super::fixed::{Fx, Turns};

/// Microseconds since an arbitrary monotonic epoch. The target adapts its
/// real clock (`embassy_time::Instant`) into this; nothing here depends on
/// Embassy or hardware.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct TimestampUs(pub u64);

/// How long a commanded turn takes to complete, in seconds. Presentation
/// policy (feels responsive without looking like a snap), not a physics
/// constant -- local to this module rather than `config`, since nothing else
/// reads it.
const TURN_DURATION_S: Fx = Fx::ratio(35, 100);

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RotationCommandKind {
    /// V1: a signed quarter-turn request. Positive turns are clockwise (this
    /// app's `y`-down local frame), matching one `KEY` press. Later
    /// press/release adapters may issue smaller or larger signed deltas; the
    /// integration below does not assume a quarter turn specifically.
    RotateBy(Fx),
}

#[derive(Clone, Copy, Debug)]
pub struct RotationCommand {
    pub kind: RotationCommandKind,
    pub at: TimestampUs,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RejectedCommand {
    /// `at` did not strictly follow the last accepted command's timestamp.
    Stale,
}

#[derive(Clone, Copy, Debug)]
struct Segment {
    /// Angle at the moment this segment started, so a command that arrives
    /// mid-turn begins its own smoothstep from wherever the glass actually
    /// is rather than snapping to a queued endpoint.
    a0: Fx,
    da: Fx,
    elapsed: Fx,
    duration: Fx,
}

/// Commanded angle and angular velocity. Angle is always available; angular
/// velocity is zero exactly when no segment is animating (`is_settled`).
#[derive(Clone, Copy, Debug)]
pub struct RotationModel {
    angle: Turns,
    segment: Option<Segment>,
    last_command_at: Option<TimestampUs>,
}

impl RotationModel {
    pub const fn new() -> RotationModel {
        RotationModel {
            angle: Turns::ZERO,
            segment: None,
            last_command_at: None,
        }
    }

    /// Accept a new command, or reject a stale one. Accepting while a
    /// previous segment is still animating freezes the current angle as the
    /// new segment's start (`a0`), so overlapping `KEY` presses compose into
    /// one continuous motion instead of jumping.
    pub fn apply_command(&mut self, command: RotationCommand) -> Result<(), RejectedCommand> {
        if let Some(last) = self.last_command_at {
            if command.at <= last {
                return Err(RejectedCommand::Stale);
            }
        }
        self.last_command_at = Some(command.at);
        let RotationCommandKind::RotateBy(delta) = command.kind;
        self.segment = Some(Segment {
            a0: self.angle.value(),
            da: delta,
            elapsed: Fx::ZERO,
            duration: TURN_DURATION_S,
        });
        Ok(())
    }

    /// Advance by exactly one physics step. A no-op once settled.
    pub fn tick(&mut self, dt: Fx) {
        let Some(segment) = &mut self.segment else {
            return;
        };
        segment.elapsed = (segment.elapsed + dt).min(segment.duration);
        let u = segment.elapsed.divide(segment.duration);
        let s = smoothstep(u);
        self.angle = Turns::wrap(segment.a0 + segment.da.mul(s));
        if segment.elapsed.raw() >= segment.duration.raw() {
            self.segment = None;
        }
    }

    pub const fn angle(&self) -> Turns {
        self.angle
    }

    /// Turns per second. Zero exactly when settled.
    pub fn angular_velocity(&self) -> Fx {
        match &self.segment {
            None => Fx::ZERO,
            Some(segment) => {
                let u = segment.elapsed.divide(segment.duration);
                segment
                    .da
                    .mul(smoothstep_derivative(u))
                    .divide(segment.duration)
            }
        }
    }

    pub const fn is_settled(&self) -> bool {
        self.segment.is_none()
    }
}

impl Default for RotationModel {
    fn default() -> RotationModel {
        RotationModel::new()
    }
}

/// `3u^2 - 2u^3`, for `u` in `[0, 1]`.
fn smoothstep(u: Fx) -> Fx {
    let u2 = u.mul(u);
    let u3 = u2.mul(u);
    u2.mul_int(3) - u3.mul_int(2)
}

/// `d/du (3u^2 - 2u^3) = 6u - 6u^2`, for `u` in `[0, 1]`.
fn smoothstep_derivative(u: Fx) -> Fx {
    u.mul_int(6) - u.mul(u).mul_int(6)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: Fx = Fx::ratio(1, 50); // 50 Hz physics step

    fn run_ticks(model: &mut RotationModel, count: u32) {
        for _ in 0..count {
            model.tick(DT);
        }
    }

    fn quarter_turn_at(at_us: u64) -> RotationCommand {
        RotationCommand {
            kind: RotationCommandKind::RotateBy(Fx::ratio(1, 4)),
            at: TimestampUs(at_us),
        }
    }

    #[test]
    fn fresh_model_is_settled_at_zero() {
        let model = RotationModel::new();
        assert!(model.is_settled());
        assert_eq!(model.angle().value(), Fx::ZERO);
        assert_eq!(model.angular_velocity(), Fx::ZERO);
    }

    #[test]
    fn a_quarter_turn_command_settles_at_90_degrees() {
        let mut model = RotationModel::new();
        model.apply_command(quarter_turn_at(1)).unwrap();
        assert!(!model.is_settled());
        run_ticks(&mut model, 1_000); // far past the 0.35s turn duration
        assert!(model.is_settled());
        let tolerance = Fx::ratio(1, 2000);
        let diff = (model.angle().value() - Fx::ratio(1, 4)).abs();
        assert!(
            diff.raw() <= tolerance.raw(),
            "angle should settle near 0.25 turns"
        );
        assert_eq!(model.angular_velocity(), Fx::ZERO);
    }

    #[test]
    fn four_quarter_turns_complete_a_full_circle() {
        let mut model = RotationModel::new();
        for step in 0..4u64 {
            model
                .apply_command(quarter_turn_at(step * 1_000_000 + 1))
                .unwrap();
            run_ticks(&mut model, 1_000);
        }
        assert!(model.is_settled());
        let diff = model.angle().value().abs();
        assert!(diff.raw() <= Fx::ratio(1, 2000).raw());
    }

    #[test]
    fn angular_velocity_is_nonzero_mid_turn_and_zero_at_the_endpoints() {
        let mut model = RotationModel::new();
        model.apply_command(quarter_turn_at(1)).unwrap();
        // Immediately after the command: smoothstep's derivative is zero at
        // u=0, so velocity starts at zero even though a segment is active.
        assert_eq!(model.angular_velocity(), Fx::ZERO);
        run_ticks(&mut model, 8); // partway through the 0.35s turn
        assert!(model.angular_velocity().raw() > 0);
        run_ticks(&mut model, 1_000); // past completion
        assert!(model.is_settled());
        assert_eq!(model.angular_velocity(), Fx::ZERO);
    }

    #[test]
    fn a_stale_command_is_rejected_and_does_not_disturb_motion() {
        let mut model = RotationModel::new();
        model.apply_command(quarter_turn_at(100)).unwrap();
        run_ticks(&mut model, 1_000);
        let settled_angle = model.angle().value();

        let result = model.apply_command(quarter_turn_at(50)); // earlier than 100
        assert_eq!(result, Err(RejectedCommand::Stale));
        assert_eq!(model.angle().value(), settled_angle);
        assert!(model.is_settled());
    }

    #[test]
    fn a_command_mid_turn_composes_from_the_current_angle_rather_than_snapping() {
        let mut model = RotationModel::new();
        model.apply_command(quarter_turn_at(1)).unwrap();
        run_ticks(&mut model, 8); // partway through the first turn
        let mid_angle = model.angle().value();
        assert!(mid_angle.raw() > 0);

        model.apply_command(quarter_turn_at(2)).unwrap();
        // The new segment starts exactly where the glass was, not at 0 and
        // not at the old target -- so one tick later the angle is close to
        // where it already was, not discontinuous.
        model.tick(DT);
        let just_after = model.angle().value();
        let jump = (just_after - mid_angle).abs();
        assert!(jump.raw() < Fx::ratio(1, 20).raw(), "angle should not jump");
    }

    #[test]
    fn arbitrary_stationary_angle_holds_without_a_pending_command() {
        let mut model = RotationModel::new();
        model.apply_command(quarter_turn_at(1)).unwrap();
        run_ticks(&mut model, 1_000);
        let held = model.angle().value();
        run_ticks(&mut model, 500);
        assert_eq!(model.angle().value(), held);
        assert!(model.is_settled());
    }
}
