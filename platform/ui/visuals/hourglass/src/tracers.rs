//! Fixed-capacity presentation tracers for sub-pixel throat flow.
//!
//! Long durations move weighted one-pixel mass packets too infrequently for
//! their instantaneous falling positions to read clearly at 10 Hz. These
//! tracers visualize the sub-pixel grains represented by those packets. They
//! follow the same glass-local gravity, remain one output pixel each, and are
//! deliberately absent from collision and mass accounting.

use super::backend::wall_half_width_px;
use super::fixed::{Fx, Vec2};
use super::particles::{Bulb, HALF_HEIGHT};

pub const TRACER_CAPACITY: usize = 8;
const EMIT_EVERY_TICKS: u8 = 3;
const THROAT_OFFSET: Fx = Fx::from_int(1);

pub struct TracerStore {
    positions: [Vec2; TRACER_CAPACITY],
    velocities: [Vec2; TRACER_CAPACITY],
    active_mask: u8,
    emit_phase: u8,
    next_slot: u8,
}

impl TracerStore {
    pub const fn new() -> Self {
        Self {
            positions: [Vec2::ZERO; TRACER_CAPACITY],
            velocities: [Vec2::ZERO; TRACER_CAPACITY],
            active_mask: 0,
            // Emit on the first flowing tick, then every three ticks. At the
            // target's 10 Hz render cadence this maintains several distinct
            // one-pixel grains in flight without drawing a larger particle.
            emit_phase: EMIT_EVERY_TICKS - 1,
            next_slot: 0,
        }
    }

    pub fn step(&mut self, gravity_local: Vec2, source: Option<Bulb>, dt: Fx) {
        for index in 0..TRACER_CAPACITY {
            if self.active_mask & (1 << index) == 0 {
                continue;
            }
            self.velocities[index] = self.velocities[index] + gravity_local.scale(dt);
            self.positions[index] = self.positions[index] + self.velocities[index].scale(dt);
            if !inside_glass(self.positions[index]) {
                self.active_mask &= !(1 << index);
            }
        }

        let Some(source) = source else {
            return;
        };
        self.emit_phase += 1;
        if self.emit_phase < EMIT_EVERY_TICKS {
            return;
        }
        self.emit_phase = 0;

        let index = self.next_slot as usize;
        self.next_slot = (self.next_slot + 1) % TRACER_CAPACITY as u8;
        let y = match source {
            Bulb::Upper => THROAT_OFFSET,
            Bulb::Lower => -THROAT_OFFSET,
        };
        self.positions[index] = Vec2::new(Fx::ZERO, y);
        self.velocities[index] = Vec2::ZERO;
        self.active_mask |= 1 << index;
    }

    pub const fn positions(&self) -> &[Vec2; TRACER_CAPACITY] {
        &self.positions
    }

    pub const fn active_mask(&self) -> u8 {
        self.active_mask
    }
}

impl Default for TracerStore {
    fn default() -> Self {
        Self::new()
    }
}

fn inside_glass(position: Vec2) -> bool {
    if position.y.abs().raw() >= HALF_HEIGHT.raw() {
        return false;
    }
    let half_width = wall_half_width_px(position.y.to_int()) + 1;
    position.x.abs().raw() <= Fx::from_int(half_width).raw()
}

#[cfg(test)]
mod tests {
    use super::super::particles::GRAVITY_WORLD;
    use super::*;

    #[test]
    fn upright_flow_keeps_several_one_pixel_tracers_in_flight() {
        let mut store = TracerStore::new();
        let dt = Fx::ratio(1, 30);
        let mut maximum_active = 0;
        for _ in 0..30 {
            store.step(GRAVITY_WORLD, Some(Bulb::Upper), dt);
            let active = store.active_mask().count_ones();
            maximum_active = maximum_active.max(active);
            for index in 0..TRACER_CAPACITY {
                if store.active_mask() & (1 << index) != 0 {
                    assert!(inside_glass(store.positions()[index]));
                }
            }
        }
        assert!(maximum_active >= 5);
    }

    #[test]
    fn paused_flow_emits_nothing() {
        let mut store = TracerStore::new();
        for _ in 0..30 {
            store.step(Vec2::new(GRAVITY_WORLD.y, Fx::ZERO), None, Fx::ratio(1, 30));
        }
        assert_eq!(store.active_mask(), 0);
    }

    #[test]
    fn fixed_trace_is_deterministic() {
        fn run() -> ([Vec2; TRACER_CAPACITY], u8) {
            let mut store = TracerStore::new();
            for _ in 0..20 {
                store.step(GRAVITY_WORLD, Some(Bulb::Upper), Fx::ratio(1, 30));
            }
            (*store.positions(), store.active_mask())
        }
        assert_eq!(run(), run());
    }
}
