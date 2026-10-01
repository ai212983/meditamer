//! Bounded position-based granular solver, per the plan's "Granular model
//! and rendering" section.
//!
//! Everything here runs in **glass-local coordinates**: the boundary is a
//! fixed bowtie (two trapezoidal bulbs meeting at a throat) that never
//! rotates. Only two inputs carry the glass's commanded angle, exactly as
//! the plan's rotation section derives: `gravity_local(a) = R(-a) *
//! gravity_world`, and the wall's own velocity at a local point `r`, which
//! reduces (in local coordinates) to `angular_velocity_rad * J * r` -- no
//! per-step rotation of the geometry itself. [`super::rotation`] and
//! [`super::model`] own `a(t)` and `angular_velocity`; this module only ever
//! receives their already-computed values.

use super::fixed::{Fx, Vec2};

/// Enough mass for a visible stream within the measured S3 deadline and
/// memory margins.
pub const PARTICLE_CAPACITY: usize = 384;

/// Every particle carries the same mass, so total mass is just the particle
/// count.
pub const TOTAL_MASS: u32 = PARTICLE_CAPACITY as u32;

// Glass geometry, in local-frame pixels. A bowtie: two trapezoidal bulbs
// (top and bottom) meeting at a point-width throat, symmetric about both
// axes. Chosen to fit comfortably inside a widget a few hundred pixels
// across on this board's 400x300 panel; not device-measured.
pub const HALF_WIDTH: Fx = Fx::from_int(58);
pub const HALF_HEIGHT: Fx = Fx::from_int(85);
// A throat at 4x the grain diameter: narrow enough to read as a pinch, wide
// enough that a handful of grains cannot bridge it and jam. A narrower
// throat (half-width 6, 3x grain diameter) arches and nearly stalls flow.
pub const THROAT_HALF_WIDTH: Fx = Fx::from_int(4);
pub const PARTICLE_RADIUS: Fx = Fx::from_int(1);

/// World gravity, `+y` is down (screen convention). Magnitude is a solver
/// tuning constant, not a physical unit -- pixels per second squared.
pub const GRAVITY_WORLD: Vec2 = Vec2::new(Fx::ZERO, Fx::from_int(420));

const RESTITUTION: Fx = Fx::ratio(1, 10);
/// Bulk granular energy loss per real second. Particle contacts are otherwise
/// perfectly elastic position corrections, which leaves a horizontal pile
/// numerically stirring forever. Scaling by `dt` keeps this a real-time
/// material property rather than a step-count property.
const LINEAR_DAMPING_PER_SECOND: Fx = Fx::from_int(4);
/// Tangential velocity retained after wall contact. This is applied on the
/// fixed real-time solver step so grains do not appear to adhere to walls.
const TANGENTIAL_RETENTION: Fx = Fx::ratio(49, 50);
const SOLVER_ITERATIONS: u32 = 3;

/// Which bulb a particle currently occupies, by local `y` sign. The throat
/// has zero height (a single pinch point at `y = 0`), so this partition is
/// exhaustive and exact.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Bulb {
    Upper,
    Lower,
}

fn bulb_of(position: Vec2) -> Bulb {
    if position.y.raw() <= 0 {
        Bulb::Upper
    } else {
        Bulb::Lower
    }
}

/// One physics step's crossing tally, in mass units (particle count).
/// [`super::model::MassLedger`] turns this into the session's running
/// chamber masses.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct StepCrossings {
    pub upper_to_lower: u32,
    pub lower_to_upper: u32,
}

impl StepCrossings {
    pub const fn net_to_lower(self) -> i32 {
        self.upper_to_lower as i32 - self.lower_to_upper as i32
    }
}

/// A convex polygon boundary tested by perpendicular distance to each edge's
/// line, winding-agnostic (the "inside" sign is derived once from the
/// polygon's own signed area, so vertices may be listed in either order).
///
/// `open_edge` names one edge index that is *not* enforced: each bulb here is
/// described as a closed quadrilateral purely so [`Self::new`] can derive a
/// consistent winding, but the edge running along the throat (`y = 0`) is a
/// passage between the two bulbs, not a wall. Enforcing it as an ordinary
/// edge seals the throat and stalls flow.
struct ConvexRegion<const N: usize> {
    vertices: [Vec2; N],
    inside_sign: Fx,
    open_edge: usize,
}

impl<const N: usize> ConvexRegion<N> {
    fn new(vertices: [Vec2; N], open_edge: usize) -> ConvexRegion<N> {
        let mut area2 = Fx::ZERO;
        for i in 0..N {
            let a = vertices[i];
            let b = vertices[(i + 1) % N];
            area2 += a.x.mul(b.y) - b.x.mul(a.y);
        }
        let inside_sign = if area2.raw() >= 0 { Fx::ONE } else { -Fx::ONE };
        ConvexRegion {
            vertices,
            inside_sign,
            open_edge,
        }
    }

    /// If `point` is within `margin` of the boundary or outside it, returns
    /// the corrected point (pushed to keep exactly `margin` clearance from
    /// the binding edge) and that edge's outward unit normal. `None` when
    /// already clear of every edge by at least `margin`.
    fn push_out(&self, point: Vec2, margin: Fx) -> Option<(Vec2, Vec2)> {
        // A sentinel comfortably larger than any real distance at this
        // widget's pixel scale, but small enough to stay inside `Fx::mul`'s
        // safe range (Q16.16 in an `i32` only multiplies safely up to
        // roughly +/-180 before the `i64` intermediate's right shift
        // overflows `i32` on the way back down) -- this value is only ever
        // added/subtracted/compared, never squared, so it does not need that
        // headroom itself, but nothing here should get in the habit of
        // reusing "big sentinel" constants across both uses.
        let mut worst_distance = Fx::from_int(10_000);
        let mut worst_normal = Vec2::ZERO;
        for i in 0..N {
            if i == self.open_edge {
                continue;
            }
            let a = self.vertices[i];
            let b = self.vertices[(i + 1) % N];
            let edge = b - a;
            let edge_len = edge.length();
            if edge_len.raw() == 0 {
                continue;
            }
            let to_point = point - a;
            let cross = edge.x.mul(to_point.y) - edge.y.mul(to_point.x);
            let signed_inside = cross.mul(self.inside_sign);
            let distance = signed_inside.divide(edge_len);
            if distance.raw() < worst_distance.raw() {
                worst_distance = distance;
                // Direction of increasing "inside": inside_sign * quarter_turn(edge).
                let inward = edge.quarter_turn().scale(self.inside_sign);
                let inward_len = inward.length();
                let outward = if inward_len.raw() == 0 {
                    Vec2::ZERO
                } else {
                    inward.scale(-Fx::ONE.divide(inward_len))
                };
                worst_normal = outward;
            }
        }
        if worst_distance.raw() < margin.raw() {
            // Pull back toward the interior (against the outward normal) by
            // however much the margin was violated.
            let corrected = point - worst_normal.scale(margin - worst_distance);
            Some((corrected, worst_normal))
        } else {
            None
        }
    }
}

fn upper_bulb() -> ConvexRegion<4> {
    // Edge 2 (throat-half-width, 0) -> (-throat-half-width, 0) is the throat:
    // open, not a wall.
    ConvexRegion::new(
        [
            Vec2::new(-HALF_WIDTH, -HALF_HEIGHT),
            Vec2::new(HALF_WIDTH, -HALF_HEIGHT),
            Vec2::new(THROAT_HALF_WIDTH, Fx::ZERO),
            Vec2::new(-THROAT_HALF_WIDTH, Fx::ZERO),
        ],
        2,
    )
}

fn lower_bulb() -> ConvexRegion<4> {
    // Edge 0 (-throat-half-width, 0) -> (throat-half-width, 0) is the throat:
    // open, not a wall.
    ConvexRegion::new(
        [
            Vec2::new(-THROAT_HALF_WIDTH, Fx::ZERO),
            Vec2::new(THROAT_HALF_WIDTH, Fx::ZERO),
            Vec2::new(HALF_WIDTH, HALF_HEIGHT),
            Vec2::new(-HALF_WIDTH, HALF_HEIGHT),
        ],
        0,
    )
}

/// Fixed-capacity uniform grid broad phase over the glass's local extent.
/// Four pixels is two collision diameters: the surrounding nine cells still
/// contain every possible contact while avoiding the much larger candidate
/// set produced by the former eight-pixel cells.
const CELL_SIZE: i32 = 4;
const GRID_W: usize = (2 * 58 / CELL_SIZE + 2) as usize;
const GRID_H: usize = (2 * 85 / CELL_SIZE + 2) as usize;

struct Grid {
    head: [i16; GRID_W * GRID_H],
    next: [i16; PARTICLE_CAPACITY],
}

impl Grid {
    fn new() -> Grid {
        Grid {
            head: [-1; GRID_W * GRID_H],
            next: [-1; PARTICLE_CAPACITY],
        }
    }

    fn cell_of(position: Vec2) -> (usize, usize) {
        let cx = ((position.x.to_int() + HALF_WIDTH.to_int()) / CELL_SIZE)
            .clamp(0, GRID_W as i32 - 1) as usize;
        let cy = ((position.y.to_int() + HALF_HEIGHT.to_int()) / CELL_SIZE)
            .clamp(0, GRID_H as i32 - 1) as usize;
        (cx, cy)
    }

    fn insert(&mut self, index: usize, position: Vec2) {
        let (cx, cy) = Self::cell_of(position);
        let cell = cy * GRID_W + cx;
        self.next[index] = self.head[cell];
        self.head[cell] = index as i16;
    }

    fn head_at(&self, x: usize, y: usize) -> i16 {
        self.head[y * GRID_W + x]
    }

    fn next_after(&self, index: usize) -> i16 {
        self.next[index]
    }
}

pub struct ParticleStore {
    position: [Vec2; PARTICLE_CAPACITY],
    velocity: [Vec2; PARTICLE_CAPACITY],
}

impl ParticleStore {
    /// Every grain starts at rest, packed into the upper bulb -- the
    /// deterministic "Ready" state a fresh session and the checked reference
    /// run begin from.
    pub fn new_full_upper_bulb() -> ParticleStore {
        let mut position = [Vec2::ZERO; PARTICLE_CAPACITY];
        let spacing = PARTICLE_RADIUS.mul_int(2) + Fx::from_int(1);
        let usable_half_width = HALF_WIDTH - PARTICLE_RADIUS - Fx::from_int(2);
        let columns = ((usable_half_width.mul_int(2)).divide(spacing))
            .to_int()
            .max(1) as usize;
        let mut placed = 0usize;
        let mut row = 0i32;
        'fill: loop {
            let y = -HALF_HEIGHT + PARTICLE_RADIUS + Fx::from_int(2) + spacing.mul_int(row);
            if y.raw() > 0 {
                break;
            }
            // This row's own width limit at this height (the bulb narrows
            // toward the throat), independent of the packing's overall
            // `columns` count.
            let row_half_width = boundary_half_width_at(y, Bulb::Upper) - PARTICLE_RADIUS;
            let row_columns = ((row_half_width.mul_int(2)).divide(spacing))
                .to_int()
                .clamp(1, columns as i32) as usize;
            // Alternate row offset: avoids a perfectly crystalline lattice,
            // which settles into unrealistic rigid columns under gravity.
            let offset = if row % 2 == 0 {
                Fx::ZERO
            } else {
                spacing.divide(Fx::from_int(2))
            };
            for column in 0..row_columns {
                if placed >= PARTICLE_CAPACITY {
                    break 'fill;
                }
                let x = -row_half_width + PARTICLE_RADIUS + spacing.mul_int(column as i32) + offset;
                if x.raw() <= row_half_width.raw() {
                    position[placed] = Vec2::new(x, y);
                    placed += 1;
                }
            }
            row += 1;
        }
        // Any remaining capacity (a very small/narrow bulb) is stacked just
        // above the throat rather than left uninitialised.
        for slot in position.iter_mut().skip(placed) {
            *slot = Vec2::new(Fx::ZERO, -PARTICLE_RADIUS.mul_int(2));
        }
        ParticleStore {
            position,
            velocity: [Vec2::ZERO; PARTICLE_CAPACITY],
        }
    }

    pub fn positions(&self) -> &[Vec2; PARTICLE_CAPACITY] {
        &self.position
    }

    pub fn total_mass_in(&self, bulb: Bulb) -> u32 {
        self.position
            .iter()
            .filter(|p| bulb_of(**p) == bulb)
            .count() as u32
    }

    /// Advance by exactly one physics step. `gravity_local` and
    /// `angular_velocity_turns_per_sec` are the only angle-dependent inputs
    /// (see the module doc); everything else -- particle contacts, boundary
    /// geometry -- is angle-independent by construction.
    pub fn step(
        &mut self,
        gravity_local: Vec2,
        angular_velocity_turns_per_sec: Fx,
        dt: Fx,
    ) -> StepCrossings {
        let before: [Bulb; PARTICLE_CAPACITY] = core::array::from_fn(|i| bulb_of(self.position[i]));
        let mut predicted = self.predicted_positions(gravity_local, dt);
        let mut boundary_normal: [Option<Vec2>; PARTICLE_CAPACITY] = [None; PARTICLE_CAPACITY];
        self.solve_constraints(&mut predicted, &mut boundary_normal);
        self.commit_step(
            &before,
            &predicted,
            &boundary_normal,
            angular_velocity_turns_per_sec,
            dt,
        )
    }

    fn predicted_positions(&self, gravity_local: Vec2, dt: Fx) -> [Vec2; PARTICLE_CAPACITY] {
        let dt_squared = dt.mul(dt);
        let accel_term = if dt_squared.raw() == 0 {
            gravity_local.scale(dt).scale(dt)
        } else {
            gravity_local.scale(dt_squared)
        };
        core::array::from_fn(|i| self.position[i] + self.velocity[i].scale(dt) + accel_term)
    }

    fn solve_constraints(
        &self,
        predicted: &mut [Vec2; PARTICLE_CAPACITY],
        boundary_normal: &mut [Option<Vec2>; PARTICLE_CAPACITY],
    ) {
        let upper = upper_bulb();
        let lower = lower_bulb();
        for _ in 0..SOLVER_ITERATIONS {
            resolve_particle_contacts(predicted);
            resolve_boundaries(predicted, boundary_normal, &upper, &lower);
        }
    }

    fn commit_step(
        &mut self,
        before: &[Bulb; PARTICLE_CAPACITY],
        predicted: &[Vec2; PARTICLE_CAPACITY],
        boundary_normal: &[Option<Vec2>; PARTICLE_CAPACITY],
        angular_velocity_turns_per_sec: Fx,
        dt: Fx,
    ) -> StepCrossings {
        let omega_rad_per_sec = angular_velocity_turns_per_sec.mul(Fx::TWO_PI);
        let velocity_retention =
            (Fx::ONE - LINEAR_DAMPING_PER_SECOND.mul(dt)).clamp(Fx::ZERO, Fx::ONE);
        self.update_particles(
            predicted,
            boundary_normal,
            omega_rad_per_sec,
            velocity_retention,
            dt,
        );
        crossings_from(before, &self.position)
    }

    fn update_particles(
        &mut self,
        predicted: &[Vec2; PARTICLE_CAPACITY],
        boundary_normal: &[Option<Vec2>; PARTICLE_CAPACITY],
        omega_rad_per_sec: Fx,
        velocity_retention: Fx,
        dt: Fx,
    ) {
        for i in 0..PARTICLE_CAPACITY {
            self.velocity[i] = next_velocity(
                predicted[i],
                self.position[i],
                boundary_normal[i],
                omega_rad_per_sec,
                velocity_retention,
                dt,
            );
            self.position[i] = predicted[i];
        }
    }
}

fn resolve_particle_contacts(predicted: &mut [Vec2; PARTICLE_CAPACITY]) {
    let mut grid = Grid::new();
    for (index, position) in predicted.iter().enumerate() {
        grid.insert(index, *position);
    }
    for i in 0..PARTICLE_CAPACITY {
        let (cx, cy) = Grid::cell_of(predicted[i]);
        for dy in -1i32..=1 {
            for dx in -1i32..=1 {
                resolve_neighbour_contacts(predicted, &grid, i, cx as i32 + dx, cy as i32 + dy);
            }
        }
    }
}

fn resolve_neighbour_contacts(
    predicted: &mut [Vec2; PARTICLE_CAPACITY],
    grid: &Grid,
    i: usize,
    x: i32,
    y: i32,
) {
    if x < 0 || y < 0 || x >= GRID_W as i32 || y >= GRID_H as i32 {
        return;
    }
    let mut cursor = grid.head_at(x as usize, y as usize);
    while cursor >= 0 {
        let j = cursor as usize;
        cursor = grid.next_after(j);
        if j > i {
            resolve_particle_pair(predicted, i, j);
        }
    }
}

fn resolve_boundaries(
    predicted: &mut [Vec2; PARTICLE_CAPACITY],
    boundary_normal: &mut [Option<Vec2>; PARTICLE_CAPACITY],
    upper: &ConvexRegion<4>,
    lower: &ConvexRegion<4>,
) {
    for i in 0..PARTICLE_CAPACITY {
        let region = if predicted[i].y.raw() <= 0 {
            upper
        } else {
            lower
        };
        if let Some((corrected, normal)) = region.push_out(predicted[i], PARTICLE_RADIUS) {
            predicted[i] = corrected;
            boundary_normal[i] = Some(normal);
        }
    }
}

fn next_velocity(
    predicted: Vec2,
    position: Vec2,
    boundary_normal: Option<Vec2>,
    omega_rad_per_sec: Fx,
    velocity_retention: Fx,
    dt: Fx,
) -> Vec2 {
    let mut velocity = (predicted - position).scale(Fx::ONE.divide(dt));
    if let Some(normal) = boundary_normal {
        let wall_velocity = predicted.quarter_turn().scale(omega_rad_per_sec);
        let relative = velocity - wall_velocity;
        let normal_component = relative.x.mul(normal.x) + relative.y.mul(normal.y);
        let normal_vec = normal.scale(normal_component);
        let tangent_vec = relative - normal_vec;
        let bounced_normal = if normal_component.raw() > 0 {
            normal_vec.scale(-RESTITUTION)
        } else {
            normal_vec
        };
        velocity = wall_velocity + bounced_normal + tangent_vec.scale(TANGENTIAL_RETENTION);
    }
    velocity.scale(velocity_retention)
}

fn crossings_from(
    before: &[Bulb; PARTICLE_CAPACITY],
    positions: &[Vec2; PARTICLE_CAPACITY],
) -> StepCrossings {
    let mut crossings = StepCrossings::default();
    for (index, position) in positions.iter().enumerate() {
        match (before[index], bulb_of(*position)) {
            (Bulb::Upper, Bulb::Lower) => crossings.upper_to_lower += 1,
            (Bulb::Lower, Bulb::Upper) => crossings.lower_to_upper += 1,
            _ => {}
        }
    }
    crossings
}

fn resolve_particle_pair(predicted: &mut [Vec2; PARTICLE_CAPACITY], i: usize, j: usize) {
    let delta = predicted[j] - predicted[i];
    let min_distance = PARTICLE_RADIUS.mul_int(2);
    let distance_squared = delta.length_squared();
    if distance_squared.raw() == 0 {
        // Exactly coincident (only possible from a degenerate seed): nudge
        // along a fixed axis so the pair separates deterministically rather
        // than dividing by zero.
        predicted[i] = predicted[i] - Vec2::new(min_distance.divide(Fx::from_int(2)), Fx::ZERO);
        predicted[j] = predicted[j] + Vec2::new(min_distance.divide(Fx::from_int(2)), Fx::ZERO);
        return;
    }
    if distance_squared.raw() >= min_distance.mul(min_distance).raw() {
        return;
    }
    // Integer square root is the expensive part of contact projection. Run
    // it only for a confirmed overlap, not every candidate supplied by the
    // broad phase.
    let distance = delta.length();
    let push = (min_distance - distance).divide(Fx::from_int(2));
    let direction = delta.scale(Fx::ONE.divide(distance));
    predicted[i] = predicted[i] - direction.scale(push);
    predicted[j] = predicted[j] + direction.scale(push);
}

/// The bulb's half-width at local `y`, per the linear taper toward the
/// throat used both by seeding and (implicitly, via [`ConvexRegion`]) by
/// collision.
fn boundary_half_width_at(y: Fx, bulb: Bulb) -> Fx {
    match bulb {
        Bulb::Upper => {
            let t = (y + HALF_HEIGHT).divide(HALF_HEIGHT); // 0 at rim, 1 at throat
            HALF_WIDTH + (THROAT_HALF_WIDTH - HALF_WIDTH).mul(t)
        }
        Bulb::Lower => {
            let t = y.divide(HALF_HEIGHT); // 0 at throat, 1 at rim
            THROAT_HALF_WIDTH + (HALF_WIDTH - THROAT_HALF_WIDTH).mul(t)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: Fx = Fx::ratio(1, 50);
    const UPRIGHT_GRAVITY: Vec2 = GRAVITY_WORLD; // a = 0, local frame == world frame

    #[test]
    fn seeding_fills_capacity_inside_the_upper_bulb() {
        let store = ParticleStore::new_full_upper_bulb();
        assert_eq!(store.total_mass_in(Bulb::Upper), TOTAL_MASS);
        assert_eq!(store.total_mass_in(Bulb::Lower), 0);
        for position in store.positions() {
            assert!(position.y.raw() <= 0);
            let half_width = boundary_half_width_at(position.y, Bulb::Upper);
            assert!(
                position.x.abs().raw() <= half_width.raw() + 1,
                "seed particle escaped the bulb: {:?} vs half-width {:?}",
                position,
                half_width
            );
        }
    }

    #[test]
    fn mass_is_conserved_while_upright_and_flowing() {
        let mut store = ParticleStore::new_full_upper_bulb();
        for _ in 0..3_000 {
            store.step(UPRIGHT_GRAVITY, Fx::ZERO, DT);
            let total = store.total_mass_in(Bulb::Upper) + store.total_mass_in(Bulb::Lower);
            assert_eq!(total, TOTAL_MASS);
        }
    }

    #[test]
    fn sand_falls_from_upper_to_lower_when_upright() {
        let mut store = ParticleStore::new_full_upper_bulb();
        for _ in 0..3_000 {
            store.step(UPRIGHT_GRAVITY, Fx::ZERO, DT);
        }
        // Not every grain necessarily finishes in 3000 steps (60s of model
        // time), but flow under gravity should have moved a clear majority.
        assert!(
            store.total_mass_in(Bulb::Lower) > TOTAL_MASS * 3 / 4,
            "expected most mass to have crossed by now, got {} of {}",
            store.total_mass_in(Bulb::Lower),
            TOTAL_MASS
        );
    }

    #[test]
    fn particles_never_penetrate_the_boundary_by_more_than_solver_slack() {
        let mut store = ParticleStore::new_full_upper_bulb();
        let tolerance = Fx::from_int(1); // one pixel of solver slack
        for _ in 0..500 {
            store.step(UPRIGHT_GRAVITY, Fx::ZERO, DT);
            for position in store.positions() {
                let bulb = if position.y.raw() <= 0 {
                    Bulb::Upper
                } else {
                    Bulb::Lower
                };
                let half_width = boundary_half_width_at(position.y, bulb);
                assert!(position.x.abs().raw() <= (half_width + tolerance).raw());
                assert!(position.y.abs().raw() <= (HALF_HEIGHT + tolerance).raw());
            }
        }
    }

    #[test]
    fn a_horizontal_glass_has_no_flow_across_the_throat() {
        let mut store = ParticleStore::new_full_upper_bulb();
        // a = 0.25 turns: gravity_local points along local x, perpendicular
        // to the throat axis. The freshly packed 384-grain pile may shed a
        // grain through the open throat while contacts first relax; once
        // settled there must be no sustained flow.
        let horizontal_gravity = Vec2::new(GRAVITY_WORLD.y, Fx::ZERO);
        let mut settling_crossings = 0;
        for _ in 0..3_000 {
            let crossings = store.step(horizontal_gravity, Fx::ZERO, DT);
            settling_crossings += crossings.upper_to_lower + crossings.lower_to_upper;
        }
        let upper_after_settling = store.total_mass_in(Bulb::Upper);
        for _ in 0..500 {
            let crossings = store.step(horizontal_gravity, Fx::ZERO, DT);
            assert_eq!(
                crossings.upper_to_lower + crossings.lower_to_upper,
                0,
                "horizontal pile still flowing after {settling_crossings} settling crossings"
            );
        }
        assert_eq!(store.total_mass_in(Bulb::Upper), upper_after_settling);
    }

    #[test]
    fn stepping_is_deterministic_for_a_fixed_trace() {
        fn run() -> [Vec2; PARTICLE_CAPACITY] {
            let mut store = ParticleStore::new_full_upper_bulb();
            for i in 0..200 {
                let turns_fraction = Fx::ratio(i % 40, 200);
                let (s, c) =
                    super::super::fixed::sin_cos(super::super::fixed::Turns::wrap(turns_fraction));
                let gravity = Vec2::new(GRAVITY_WORLD.y.mul(s), GRAVITY_WORLD.y.mul(c));
                store.step(gravity, Fx::ratio(1, 10), DT);
            }
            *store.positions()
        }
        assert_eq!(run(), run());
    }

    #[test]
    fn convex_region_push_out_reports_the_correct_outward_side() {
        let region = upper_bulb();
        // Center of the bulb, well inside: no violation.
        assert!(region
            .push_out(
                Vec2::new(Fx::ZERO, -HALF_HEIGHT.divide(Fx::from_int(2))),
                PARTICLE_RADIUS
            )
            .is_none());

        // Far outside to the right: pushed back with a normal that points
        // further right (positive x), not left.
        let outside = Vec2::new(HALF_WIDTH.mul_int(2), -HALF_HEIGHT.divide(Fx::from_int(2)));
        let (corrected, normal) = region.push_out(outside, PARTICLE_RADIUS).unwrap();
        assert!(normal.x.raw() > 0);
        assert!(corrected.x.raw() < outside.x.raw());
    }
}
