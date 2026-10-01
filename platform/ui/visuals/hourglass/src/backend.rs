//! One explicit A/B seam between the retained PBD control and the paper
//! cellular implementation selected by production builds.

#[cfg(not(feature = "hourglass-pbd"))]
pub use super::paper_backend::{
    flow_vertical_gravity_raw, ParticleStore, Positions as BackendPositions,
    GRAVITY_SCHEDULE as ACTIVE_GRAVITY_SCHEDULE, PARTICLE_CAPACITY, THROAT_HALF_WIDTH, TOTAL_MASS,
};
#[cfg(feature = "hourglass-pbd")]
pub use super::particles::{ParticleStore, PARTICLE_CAPACITY, THROAT_HALF_WIDTH, TOTAL_MASS};

#[cfg(feature = "hourglass-pbd")]
#[derive(Clone)]
pub struct BackendPositions<'a> {
    particles: &'a ParticleStore,
    index: usize,
}

#[cfg(feature = "hourglass-pbd")]
impl ParticleStore {
    pub fn iter_positions(&self) -> BackendPositions<'_> {
        BackendPositions {
            particles: self,
            index: 0,
        }
    }
}

#[cfg(feature = "hourglass-pbd")]
impl Iterator for BackendPositions<'_> {
    type Item = super::fixed::Vec2;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index == PARTICLE_CAPACITY {
            return None;
        }
        let position = self.particles.positions()[self.index];
        self.index += 1;
        Some(position)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = PARTICLE_CAPACITY - self.index;
        (remaining, Some(remaining))
    }
}

#[cfg(feature = "hourglass-pbd")]
impl ExactSizeIterator for BackendPositions<'_> {}

#[cfg(feature = "hourglass-pbd")]
impl core::iter::FusedIterator for BackendPositions<'_> {}

/// Vertical gravity available to drive throat flow for the retained PBD
/// control. Unlike the cellular arm, PBD has no quantized repose region; the
/// model's small source dead band still handles effectively horizontal holds.
#[cfg(feature = "hourglass-pbd")]
pub fn flow_vertical_gravity_raw(gravity_local: super::fixed::Vec2) -> u32 {
    let raw = gravity_local.y.abs().raw() as u32;
    let upright = super::particles::GRAVITY_WORLD.y.abs().raw() as u32;
    if raw >= upright.saturating_sub(upright / 1_000) {
        upright
    } else {
        raw
    }
}

#[cfg(not(feature = "hourglass-pbd"))]
pub const ACTIVE_BACKEND: &str = "paper-cellular";
#[cfg(feature = "hourglass-pbd")]
pub const ACTIVE_BACKEND: &str = "pbd";

#[cfg(feature = "hourglass-pbd")]
pub const ACTIVE_GRAVITY_SCHEDULE: &str = "continuous-pbd";

/// Half-size passed to the square rasterizer. Cellular grains represent
/// subpixel occupancy and therefore use exactly one output pixel (`0`);
/// retained PBD grains keep their existing radius-one control rendering.
#[cfg(not(feature = "hourglass-pbd"))]
pub const RENDER_HALF_SIZE_PX: i32 = 0;
#[cfg(feature = "hourglass-pbd")]
pub const RENDER_HALF_SIZE_PX: i32 = 1;

/// Active collision wall profile in local output pixels. The renderer calls
/// this rather than maintaining a merely similar-looking outline.
#[cfg(not(feature = "hourglass-pbd"))]
pub use super::paper_backend::wall_half_width_px;

/// The retained PBD control intentionally keeps its original straight walls
/// so the A/B switch still isolates the solver and its established baseline.
#[cfg(feature = "hourglass-pbd")]
pub fn wall_half_width_px(y: i32) -> i32 {
    use super::particles::{HALF_HEIGHT, HALF_WIDTH};

    let height = HALF_HEIGHT.to_int();
    let distance = y.abs().min(height);
    THROAT_HALF_WIDTH.to_int()
        + (HALF_WIDTH.to_int() - THROAT_HALF_WIDTH.to_int()) * distance / height
}
