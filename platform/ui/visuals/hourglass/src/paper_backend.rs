//! Product adapter for Medinote's paper-anchored cellular model.
//!
//! The paper core owns legal 2x2 transitions and a separately documented local
//! repose pass. This adapter owns device time, arbitrary-angle lattice
//! scheduling, throat pacing, and render conversion. The resulting production
//! model is intentionally not a one-paper reproduction.

use super::fixed::{Fx, Vec2};
use super::paper_cellular::{
    medinote_half_width, GravityDirection, PaperCellular, SandCells, DEFAULT_REPOSE_RUN_CELLS,
    DEFAULT_SURFACE_RELAX_INTERVAL, DEFAULT_TOPPLE_PER_MILLE, MEDINOTE_GRAINS,
};
use super::particles::{Bulb, StepCrossings, GRAVITY_WORLD};
use core::iter::FusedIterator;

pub const PARTICLE_CAPACITY: usize = MEDINOTE_GRAINS;
pub const TOTAL_MASS: u32 = PARTICLE_CAPACITY as u32;
pub const THROAT_HALF_WIDTH: Fx = Fx::from_int(1);
pub const PRODUCTION_DISCHARGE_TICKS: u32 = 15 * 60 * 30;
#[cfg(feature = "hourglass-block-gravity")]
pub const GRAVITY_SCHEDULE: &str = "block-weighted";
#[cfg(not(feature = "hourglass-block-gravity"))]
pub const GRAVITY_SCHEDULE: &str = "global-cardinal";

const MAX_BLOCK_CROSSINGS: u8 = 2;
const PRODUCTION_SEED: u32 = 1;
/// The deterministic 2x2 phase sweep puts the final complete crossing two
/// ticks after its credit horizon. This measured horizon therefore completes
/// the conserved 4,500-cell run on public tick 27,000 exactly.
const THROAT_PACING_TICKS: u32 = PRODUCTION_DISCHARGE_TICKS - 2;

pub struct ParticleStore {
    model: PaperCellular,
    transfer_duration_ticks: u32,
    transfer_accumulator: u64,
    transfer_credit: u8,
    transfer_direction: i8,
    axis_accumulator: u32,
}

impl ParticleStore {
    pub fn new_full_upper_bulb() -> Self {
        Self {
            model: PaperCellular::medinote_unpaced_with_surface(
                PARTICLE_CAPACITY,
                DEFAULT_TOPPLE_PER_MILLE,
                PRODUCTION_SEED,
                DEFAULT_REPOSE_RUN_CELLS,
                DEFAULT_SURFACE_RELAX_INTERVAL,
            )
            .expect("production hybrid model fits its fixed geometry"),
            transfer_duration_ticks: THROAT_PACING_TICKS,
            transfer_accumulator: 0,
            transfer_credit: 0,
            transfer_direction: 0,
            axis_accumulator: 0,
        }
    }

    pub fn iter_positions(&self) -> Positions<'_> {
        Positions {
            cells: self.model.sand_cells(),
            remaining: PARTICLE_CAPACITY,
        }
    }

    pub fn total_mass_in(&self, bulb: Bulb) -> u32 {
        self.model
            .sand_cells()
            .filter(|cell| match bulb {
                Bulb::Upper => cell.y <= 0,
                Bulb::Lower => cell.y > 0,
            })
            .count() as u32
    }

    pub fn step(
        &mut self,
        gravity_local: Vec2,
        _angular_velocity_turns_per_sec: Fx,
        _dt: Fx,
    ) -> StepCrossings {
        let Some(scheduled_direction) = self.next_direction(gravity_local) else {
            return StepCrossings::default();
        };
        self.accrue_crossing_credit(gravity_local);
        let vertical_direction = match scheduled_direction {
            GravityDirection::Down => 1,
            GravityDirection::Up => -1,
            GravityDirection::Left | GravityDirection::Right => 0,
        };
        let budget = if self.transfer_duration_ticks == 0 {
            None
        } else if vertical_direction != 0 && vertical_direction == self.transfer_direction {
            Some(self.transfer_credit)
        } else {
            Some(0)
        };
        #[cfg(feature = "hourglass-block-gravity")]
        let result = self.step_block_weighted(gravity_local, scheduled_direction, budget);
        #[cfg(not(feature = "hourglass-block-gravity"))]
        let result = self.model.step(scheduled_direction, budget);
        let crossed = result.upper_to_lower + result.lower_to_upper;
        self.transfer_credit = self.transfer_credit.saturating_sub(crossed);
        StepCrossings {
            upper_to_lower: u32::from(result.upper_to_lower),
            lower_to_upper: u32::from(result.lower_to_upper),
        }
    }

    fn next_direction(&mut self, gravity: Vec2) -> Option<GravityDirection> {
        let x = gravity.x.raw();
        let y = gravity.y.raw();
        let abs_x = x.unsigned_abs();
        let abs_y = y.unsigned_abs();
        let total = abs_x.checked_add(abs_y)?;
        if total == 0 {
            return None;
        }
        self.axis_accumulator %= total;
        self.axis_accumulator = self.axis_accumulator.saturating_add(abs_x);
        let horizontal = self.axis_accumulator >= total;
        if horizontal {
            self.axis_accumulator -= total;
            Some(if x < 0 {
                GravityDirection::Left
            } else {
                GravityDirection::Right
            })
        } else {
            Some(if y < 0 {
                GravityDirection::Up
            } else {
                GravityDirection::Down
            })
        }
    }

    fn accrue_crossing_credit(&mut self, gravity: Vec2) {
        if self.transfer_duration_ticks == 0 {
            return;
        }
        let vertical_raw = flow_vertical_gravity_raw(gravity);
        let direction = if vertical_raw == 0 {
            0
        } else {
            gravity.y.raw().signum() as i8
        };
        if direction != self.transfer_direction {
            self.transfer_direction = direction;
            self.transfer_accumulator = 0;
            self.transfer_credit = 0;
        }
        if direction == 0 {
            return;
        }
        let upright_raw = GRAVITY_WORLD.y.abs().raw() as u64;
        let credit_cost = u64::from(self.transfer_duration_ticks) * upright_raw;
        self.transfer_accumulator = self
            .transfer_accumulator
            .saturating_add(PARTICLE_CAPACITY as u64 * u64::from(vertical_raw));
        while self.transfer_accumulator >= credit_cost && self.transfer_credit < MAX_BLOCK_CROSSINGS
        {
            self.transfer_accumulator -= credit_cost;
            self.transfer_credit += 1;
        }
    }

    #[cfg(feature = "hourglass-block-gravity")]
    fn step_block_weighted(
        &mut self,
        gravity: Vec2,
        surface_direction: GravityDirection,
        budget: Option<u8>,
    ) -> super::paper_cellular::PaperStep {
        let x = gravity.x.raw();
        let y = gravity.y.raw();
        let abs_x = x.unsigned_abs();
        let abs_y = y.unsigned_abs();
        if abs_x == 0 || abs_y == 0 {
            return self.model.step(surface_direction, budget);
        }
        self.model.step_weighted(
            if y < 0 {
                GravityDirection::Up
            } else {
                GravityDirection::Down
            },
            if x < 0 {
                GravityDirection::Left
            } else {
                GravityDirection::Right
            },
            abs_x,
            abs_x + abs_y,
            surface_direction,
            budget,
        )
    }
}

#[derive(Clone)]
pub struct Positions<'a> {
    cells: SandCells<'a>,
    remaining: usize,
}

impl Iterator for Positions<'_> {
    type Item = Vec2;

    fn next(&mut self) -> Option<Self::Item> {
        let cell = self.cells.next()?;
        self.remaining -= 1;
        Some(Vec2::new(
            Fx::from_int(i32::from(cell.x)),
            Fx::from_int(i32::from(cell.y)),
        ))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for Positions<'_> {}
impl FusedIterator for Positions<'_> {}

/// Vertical gravity available to drive paced throat transfer. The same
/// repose band as the prior cellular backend keeps near-horizontal holds from
/// presenting an effectively infinite countdown.
pub fn flow_vertical_gravity_raw(gravity: Vec2) -> u32 {
    let raw_x = gravity.x.raw().unsigned_abs();
    let raw_y = gravity.y.raw().unsigned_abs();
    if raw_x > raw_y.saturating_mul(2) {
        return 0;
    }
    let upright = GRAVITY_WORLD.y.abs().raw() as u32;
    if raw_y >= upright.saturating_sub(upright / 1_000) {
        upright
    } else {
        raw_y
    }
}

pub fn wall_half_width_px(y: i32) -> i32 {
    medinote_half_width(y) + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{HourglassModel, SessionState, PHYSICS_DT};

    #[test]
    #[ignore = "full 15-minute calibration; run this test with --release --ignored"]
    fn upright_production_session_conserves_mass_and_finishes_at_target() {
        let mut model = HourglassModel::new();
        assert!(model.start());
        let completed_at = (1..=PRODUCTION_DISCHARGE_TICKS + 120).find(|_| {
            model.tick();
            model.state() == SessionState::Complete
        });
        let completed_at = completed_at.expect("paced hybrid model should drain");
        assert_eq!(
            model.ledger().mass_in(Bulb::Upper) + model.ledger().mass_in(Bulb::Lower),
            TOTAL_MASS
        );
        assert_eq!(completed_at, PRODUCTION_DISCHARGE_TICKS);
        assert_eq!(model.remaining_seconds(), Some(0));
    }

    #[test]
    fn cardinal_rotation_settles_without_losing_cells() {
        let mut store = ParticleStore::new_full_upper_bulb();
        let left = Vec2::new(-GRAVITY_WORLD.y, Fx::ZERO);
        for _ in 0..300 {
            store.step(left, Fx::ZERO, PHYSICS_DT);
        }
        assert_eq!(store.iter_positions().count(), PARTICLE_CAPACITY);
    }

    #[test]
    fn rendered_wall_profile_matches_collision_wall_centres() {
        assert_eq!(wall_half_width_px(0), 2);
        assert_eq!(wall_half_width_px(85), 59);
    }
}
