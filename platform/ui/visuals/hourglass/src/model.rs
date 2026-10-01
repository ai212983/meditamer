//! Session state and the sand-mass ledger: the piece that ties
//! [`super::rotation`] and [`super::particles`] together into one
//! deterministic hourglass, per the plan's "Rotation, flow, and time model".
//!
//! [`HourglassModel::tick`] is the app's one physics step. Everything else
//! (LVGL, `KEY` GPIO, Embassy scheduling) lives in the target and only calls
//! this model or reads its outputs.

use core::mem::MaybeUninit;

use super::backend::{flow_vertical_gravity_raw, BackendPositions, ParticleStore, TOTAL_MASS};
use super::fixed::{rotate_inverse, Fx, Turns, Vec2};
use super::particles::{Bulb, StepCrossings, GRAVITY_WORLD};
use super::rotation::{RejectedCommand, RotationCommand, RotationModel};
use super::tracers::{TracerStore, TRACER_CAPACITY};

/// Fixed real-time physics step. Sand, rotation, and the countdown all
/// advance on this same wall clock.
pub const PHYSICS_HZ: u32 = 30;
pub const PHYSICS_DT: Fx = Fx::ratio(1, PHYSICS_HZ as i32);

/// Fixed V1 geometry's checked upright discharge time, rounded up for the
/// whole-second countdown. A future duration selector must change grain count
/// or throat geometry rather than slowing gravity.
pub const DEFAULT_DURATION_S: u32 =
    REFERENCE_DISCHARGE_TICKS.saturating_add(PHYSICS_HZ - 1) / PHYSICS_HZ;

/// Below this magnitude, `observed_rate` is treated as "no measurable flow"
/// rather than a very slow trickle, per the plan's "report ETA only above a
/// minimum observed rate; otherwise report Paused or Settling". Mass units
/// per second.
const MIN_OBSERVED_RATE: Fx = Fx::ratio(1, 20);

/// How close to exactly horizontal (`gravity_local.y == 0`) counts as "no
/// defined source chamber" for pause/ETA purposes. Not zero exactly: the
/// smoothstep turn motion crosses the true horizontal instantaneously, and a
/// hair-trigger dead band would flicker `Paused` on and off across that one
/// tick. In pixels/second^2, against a gravity magnitude of 420.
const GRAVITY_DEAD_BAND: Fx = Fx::from_int(4);

/// EWMA weight applied to each tick's instantaneous crossing rate. Chosen to
/// be responsive within a handful of physics steps (a rotation's whole
/// smoothstep motion is ~17 ticks) without being so fast that render-visible
/// per-tick particle noise shows up as ETA jitter.
const RATE_EWMA_ALPHA: Fx = Fx::ratio(3, 20);

/// A visible cell earns crossing credit every six ticks upright. One second
/// without any crossing therefore means flow has genuinely stopped, while
/// remaining long enough to cover a delayed complete 2x2 throat phase.
const FLOW_QUIET_TICKS: u8 = PHYSICS_HZ as u8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionState {
    /// Presented but not explicitly started yet.
    Ready,
    /// Throat flow is active, or still within the bounded quiet interval
    /// between weighted packet crossings.
    Running,
    /// No throat flow is possible at the settled orientation, or no crossing
    /// has occurred for the sustained quiet interval. Sand may still settle
    /// within a bulb without advancing the timer.
    Paused,
    /// The bulb gravity is currently trying to drain is empty. This is
    /// orientation-relative: turning a completed glass over makes the full
    /// opposite bulb the source and resumes the session.
    Complete,
}

/// Tracks each bulb's mass and the observed real-time crossing rate, from a
/// stream of [`StepCrossings`]. Deliberately independent of
/// [`super::particles::ParticleStore`]'s own position data, so a future
/// physics backend only needs to supply crossing events, not a specific
/// particle representation.
#[derive(Clone, Copy, Debug)]
pub struct MassLedger {
    mass_in_upper: u32,
    mass_in_lower: u32,
    observed_rate: Fx,
    ever_crossed: bool,
}

impl MassLedger {
    pub const fn new_full_upper(total: u32) -> MassLedger {
        MassLedger {
            mass_in_upper: total,
            mass_in_lower: 0,
            observed_rate: Fx::ZERO,
            ever_crossed: false,
        }
    }

    /// `dt` is real wall-clock time: the rate and ETA this ledger reports are
    /// real seconds, the only unit meaningful to someone watching the glass.
    pub fn record(&mut self, crossings: StepCrossings, dt_wall: Fx) {
        let net = crossings.net_to_lower();
        self.mass_in_upper = (self.mass_in_upper as i32 - net).max(0) as u32;
        self.mass_in_lower = (self.mass_in_lower as i32 + net).max(0) as u32;
        if net != 0 {
            self.ever_crossed = true;
        }
        let this_tick_total = crossings.upper_to_lower + crossings.lower_to_upper;
        let instantaneous_rate = Fx::from_int(this_tick_total as i32).divide(dt_wall);
        self.observed_rate = self.observed_rate.mul(Fx::ONE - RATE_EWMA_ALPHA)
            + instantaneous_rate.mul(RATE_EWMA_ALPHA);
    }

    pub const fn mass_in(&self, bulb: Bulb) -> u32 {
        match bulb {
            Bulb::Upper => self.mass_in_upper,
            Bulb::Lower => self.mass_in_lower,
        }
    }

    pub const fn observed_rate(&self) -> Fx {
        self.observed_rate
    }

    pub const fn ever_crossed(&self) -> bool {
        self.ever_crossed
    }

    pub const fn is_empty(&self, bulb: Bulb) -> bool {
        self.mass_in(bulb) == 0
    }
}

/// Deterministic upright reference solve for the fixed V1 geometry and
/// material parameters. Keep boot bounded on the MCU: the host regression
/// below recomputes the solve and must match this checked result exactly.
#[cfg(not(feature = "hourglass-pbd"))]
const REFERENCE_DISCHARGE_TICKS: u32 = super::paper_backend::PRODUCTION_DISCHARGE_TICKS;
#[cfg(feature = "hourglass-pbd")]
const REFERENCE_DISCHARGE_TICKS: u32 = 62;

pub struct HourglassModel {
    rotation: RotationModel,
    particles: ParticleStore,
    tracers: TracerStore,
    ledger: MassLedger,
    started: bool,
    flow_quiet_ticks: u8,
}

/// Allocation-free render view over the active backend's particle positions.
/// Cellular builds convert compact lattice cells as they are read; the PBD
/// control reads its native vector state through the same interface.
#[derive(Clone)]
pub struct ParticlePositions<'a> {
    inner: BackendPositions<'a>,
}

impl Iterator for ParticlePositions<'_> {
    type Item = Vec2;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl ExactSizeIterator for ParticlePositions<'_> {}
impl core::iter::FusedIterator for ParticlePositions<'_> {}

impl HourglassModel {
    pub fn new() -> HourglassModel {
        HourglassModel {
            rotation: RotationModel::new(),
            particles: ParticleStore::new_full_upper_bulb(),
            tracers: TracerStore::new(),
            ledger: MassLedger::new_full_upper(TOTAL_MASS),
            started: false,
            flow_quiet_ticks: 0,
        }
    }

    /// Initialize the model in its final allocation without an unsafe field
    /// projection.
    ///
    /// The previous sparse backend needed raw field-by-field initialization to
    /// avoid a 42 KiB stack temporary. The compact bitmap backend is small
    /// enough for the compiler's normal return-place optimization; target ELF
    /// stack gates verify that this safe construction remains affordable.
    pub fn init_in_place(slot: &mut MaybeUninit<Self>) -> &mut Self {
        slot.write(Self::new())
    }

    /// Start a freshly presented hourglass. Returns true only for the first
    /// start action; subsequent actions can be interpreted as rotations.
    pub fn start(&mut self) -> bool {
        if self.started {
            return false;
        }
        self.started = true;
        true
    }

    pub fn apply_command(&mut self, command: RotationCommand) -> Result<(), RejectedCommand> {
        self.rotation.apply_command(command)
    }

    /// Advance the whole app by exactly one physics step (`PHYSICS_DT` of
    /// real wall time). Deterministic: given the same sequence of accepted
    /// commands at the same tick indices, produces the same angle, crossing
    /// ledger, and particle positions regardless of how often (or whether) a
    /// caller reads them in between -- rendering never drives this.
    pub fn tick(&mut self) {
        if !self.started {
            return;
        }
        self.rotation.tick(PHYSICS_DT);
        let angle = self.rotation.angle();
        let angular_v_wall = self.rotation.angular_velocity();
        let gravity_local = rotate_inverse(GRAVITY_WORLD, angle);
        let crossings = self
            .particles
            .step(gravity_local, angular_v_wall, PHYSICS_DT);
        let crossed_throat = crossings.upper_to_lower + crossings.lower_to_upper != 0;
        if !self.rotation.is_settled() || crossed_throat {
            self.flow_quiet_ticks = 0;
        } else {
            self.flow_quiet_ticks = self.flow_quiet_ticks.saturating_add(1);
        }
        self.ledger.record(crossings, PHYSICS_DT);
        let source = self.source_bulb();
        self.tracers.step(
            gravity_local,
            source.filter(|bulb| !self.ledger.is_empty(*bulb)),
            PHYSICS_DT,
        );
    }

    pub const fn angle(&self) -> Turns {
        self.rotation.angle()
    }

    pub fn positions(&self) -> ParticlePositions<'_> {
        ParticlePositions {
            inner: self.particles.iter_positions(),
        }
    }

    pub const fn tracer_positions(&self) -> &[Vec2; TRACER_CAPACITY] {
        self.tracers.positions()
    }

    pub const fn tracer_active_mask(&self) -> u8 {
        self.tracers.active_mask()
    }

    pub const fn ledger(&self) -> &MassLedger {
        &self.ledger
    }

    /// The bulb gravity is currently pulling sand out of, or `None` at (or
    /// within the dead band of) exactly horizontal, where no chamber is "up".
    fn source_bulb(&self) -> Option<Bulb> {
        let gravity_y = rotate_inverse(GRAVITY_WORLD, self.rotation.angle()).y;
        if gravity_y.raw() > GRAVITY_DEAD_BAND.raw() {
            Some(Bulb::Upper)
        } else if gravity_y.raw() < -GRAVITY_DEAD_BAND.raw() {
            Some(Bulb::Lower)
        } else {
            None
        }
    }

    pub fn state(&self) -> SessionState {
        if !self.started {
            return SessionState::Ready;
        }
        let gravity_local = rotate_inverse(GRAVITY_WORLD, self.rotation.angle());
        let Some(source) = self.source_bulb() else {
            return if self.rotation.is_settled() {
                SessionState::Paused
            } else {
                SessionState::Running
            };
        };
        if self.ledger.is_empty(source) {
            return SessionState::Complete;
        }
        if self.rotation.is_settled()
            && (flow_vertical_gravity_raw(gravity_local) == 0
                || self.flow_quiet_ticks >= FLOW_QUIET_TICKS)
        {
            return SessionState::Paused;
        }
        SessionState::Running
    }

    /// ETA derived from the mass in the bulb gravity is currently draining
    /// and the calibrated angle-dependent release rate. A 180-degree turn
    /// therefore swaps to the mass accumulated in the other bulb. `None`
    /// means the current orientation has no finite flow rate.
    pub fn remaining_seconds(&self) -> Option<u32> {
        let gravity_local = rotate_inverse(GRAVITY_WORLD, self.rotation.angle());
        let source = self.source_bulb()?;
        let source_mass = self.ledger.mass_in(source);
        if source_mass == 0 {
            return Some(0);
        }
        if self.started && self.rotation.is_settled() && self.flow_quiet_ticks >= FLOW_QUIET_TICKS {
            return None;
        }
        calibrated_remaining_seconds(source_mass, flow_vertical_gravity_raw(gravity_local))
    }

    /// `mass_in_current_upper_chamber / observed_rate` from the plan:
    /// available only while flow is currently measurable and a source
    /// chamber is defined. `None` otherwise (`Paused`/`Settling`).
    pub fn conditional_eta_seconds(&self) -> Option<Fx> {
        let source = self.source_bulb()?;
        if self.ledger.observed_rate().raw() < MIN_OBSERVED_RATE.raw() {
            return None;
        }
        Some(Fx::from_int(self.ledger.mass_in(source) as i32).divide(self.ledger.observed_rate()))
    }

    /// `T * mass_in_current_upper_chamber / N`: always available (given a
    /// defined source chamber), independent of recently observed flow.
    pub fn mass_fraction_eta_seconds(&self) -> Option<Fx> {
        let source = self.source_bulb()?;
        let duration = Fx::from_int(DEFAULT_DURATION_S as i32);
        let fraction = Fx::from_int(self.ledger.mass_in(source) as i32)
            .divide(Fx::from_int(TOTAL_MASS as i32));
        Some(duration.mul(fraction))
    }
}

impl Default for HourglassModel {
    fn default() -> Self {
        Self::new()
    }
}

fn ceil_div_u64(numerator: u64, denominator: u64) -> u64 {
    numerator / denominator + u64::from(!numerator.is_multiple_of(denominator))
}

fn calibrated_remaining_seconds(source_mass: u32, vertical_gravity_raw: u32) -> Option<u32> {
    if source_mass == 0 {
        return Some(0);
    }
    if vertical_gravity_raw == 0 {
        return None;
    }
    let upright_raw = GRAVITY_WORLD.y.abs().raw() as u64;
    let numerator = u64::from(REFERENCE_DISCHARGE_TICKS) * u64::from(source_mass) * upright_raw;
    let denominator =
        u64::from(TOTAL_MASS) * u64::from(vertical_gravity_raw) * u64::from(PHYSICS_HZ);
    Some(ceil_div_u64(numerator, denominator).min(u64::from(u32::MAX)) as u32)
}

#[cfg(test)]
mod tests {
    use super::super::rotation::{RotationCommandKind, TimestampUs};
    use super::*;

    fn quarter_turn(model: &mut HourglassModel, at_us: u64) {
        model
            .apply_command(RotationCommand {
                kind: RotationCommandKind::RotateBy(Fx::ratio(1, 4)),
                at: TimestampUs(at_us),
            })
            .unwrap();
    }

    #[test]
    fn fresh_model_is_ready_with_all_mass_upper() {
        let mut model = HourglassModel::new();
        assert_eq!(model.state(), SessionState::Ready);
        model.tick();
        assert_eq!(model.state(), SessionState::Ready);
        assert_eq!(model.ledger().mass_in(Bulb::Upper), TOTAL_MASS);
        assert_eq!(model.ledger().mass_in(Bulb::Lower), 0);
        assert!(model.start());
        assert_eq!(model.state(), SessionState::Running);
        assert!(!model.start());
    }

    #[test]
    #[cfg(feature = "hourglass-pbd")]
    fn an_upright_session_reaches_complete_at_its_checked_drain_time() {
        let mut model = HourglassModel::new();
        assert!(model.start());
        let mut became_running = false;
        let mut completed_at: Option<u32> = None;
        let max_ticks = REFERENCE_DISCHARGE_TICKS * 2;
        for tick in 1..=max_ticks {
            model.tick();
            if model.state() == SessionState::Running {
                became_running = true;
            }
            if model.state() == SessionState::Complete && completed_at.is_none() {
                completed_at = Some(tick);
            }
        }
        assert!(became_running, "session should pass through Running");
        let completed_at = completed_at.expect("session should complete within generous slack");
        assert_eq!(completed_at, REFERENCE_DISCHARGE_TICKS);
    }

    #[test]
    fn mass_is_conserved_across_a_mixed_rotation_trace() {
        let mut model = HourglassModel::new();
        model.start();
        let mut at = 1u64;
        for _ in 0..4 {
            quarter_turn(&mut model, at);
            at += 1_000_000;
            for _ in 0..400 {
                model.tick();
            }
            let total = model.ledger().mass_in(Bulb::Upper) + model.ledger().mass_in(Bulb::Lower);
            assert_eq!(total, TOTAL_MASS);
        }
    }

    #[test]
    #[cfg(not(feature = "hourglass-pbd"))]
    fn horizontal_hold_pauses_and_vertical_resumes() {
        let mut model = HourglassModel::new();
        model.start();
        quarter_turn(&mut model, 1);
        for _ in 0..1_000 {
            model.tick(); // settle at 90 degrees: horizontal
        }
        assert_eq!(model.state(), SessionState::Paused);
        assert!(model.conditional_eta_seconds().is_none());

        model
            .apply_command(RotationCommand {
                kind: RotationCommandKind::RotateBy(-Fx::ratio(1, 4)),
                at: TimestampUs(2_000_000),
            })
            .unwrap(); // return to the original vertical/source bulb
        for _ in 0..1_000 {
            model.tick();
        }
        assert_eq!(model.state(), SessionState::Running);
    }

    #[test]
    fn mass_fraction_eta_is_available_even_without_recent_flow() {
        let model = HourglassModel::new();
        // Upright but not yet ticked: no observed flow yet, so the
        // rate-conditional ETA is unavailable, but the always-on formula is.
        assert!(model.conditional_eta_seconds().is_none());
        let eta = model.mass_fraction_eta_seconds().unwrap();
        assert_eq!(eta, Fx::from_int(DEFAULT_DURATION_S as i32));
    }

    #[test]
    fn real_gravity_has_render_visible_motion_within_one_second() {
        let mut model = HourglassModel::new();
        let initial = model.positions().fold(0u64, |hash, position| {
            hash.wrapping_mul(0x100_0000_01B3)
                ^ position.x.raw() as u32 as u64
                ^ (position.y.raw() as u32 as u64).rotate_left(32)
        });
        model.start();
        for _ in 0..PHYSICS_HZ {
            model.tick();
        }
        let after = model.positions().fold(0u64, |hash, position| {
            hash.wrapping_mul(0x100_0000_01B3)
                ^ position.x.raw() as u32 as u64
                ^ (position.y.raw() as u32 as u64).rotate_left(32)
        });
        assert_ne!(after, initial);
    }

    #[test]
    #[cfg(not(feature = "hourglass-pbd"))]
    fn displayed_countdown_follows_source_mass_and_pauses_without_flow() {
        let mut upright = HourglassModel::new();
        assert_eq!(upright.remaining_seconds(), Some(DEFAULT_DURATION_S));
        upright.start();
        for _ in 0..(PHYSICS_HZ * 10) {
            upright.tick();
        }
        assert_eq!(upright.remaining_seconds(), Some(DEFAULT_DURATION_S - 10));

        let mut model = HourglassModel::new();
        model.start();
        quarter_turn(&mut model, 1_000_000);
        for _ in 0..30 {
            model.tick();
        }
        assert_eq!(model.state(), SessionState::Paused);
        assert_eq!(model.remaining_seconds(), None);
        for _ in 0..(PHYSICS_HZ * 3) {
            model.tick();
        }
        assert_eq!(model.state(), SessionState::Paused);
        assert_eq!(model.remaining_seconds(), None);
    }

    #[test]
    #[cfg(not(feature = "hourglass-pbd"))]
    fn weighted_packet_gaps_do_not_flicker_running_state() {
        let mut model = HourglassModel::new();
        model.start();
        for _ in 0..(PHYSICS_HZ * 10) {
            model.tick();
            assert_eq!(model.state(), SessionState::Running);
            assert!(model.remaining_seconds().is_some());
        }
    }

    #[test]
    #[cfg(not(feature = "hourglass-pbd"))]
    fn half_turn_rebases_countdown_to_sand_in_the_other_bulb() {
        let mut model = HourglassModel::new();
        model.start();
        for _ in 0..(PHYSICS_HZ * 10) {
            model.tick();
        }
        let transferred_before_turn = model.ledger().mass_in(Bulb::Lower);
        assert!(transferred_before_turn > 0);

        model
            .apply_command(RotationCommand {
                kind: RotationCommandKind::RotateBy(Fx::HALF),
                at: TimestampUs(1_000_000),
            })
            .unwrap();
        for _ in 0..12 {
            model.tick();
        }

        assert_eq!(model.source_bulb(), Some(Bulb::Lower));
        let rebased = model.remaining_seconds().unwrap();
        assert!(rebased > 0);
        assert!(
            rebased <= 11,
            "ten seconds of transferred sand should reverse in about ten seconds, got {rebased}"
        );
    }

    #[test]
    #[cfg(not(feature = "hourglass-pbd"))]
    fn arbitrary_angles_select_direction_and_scale_eta_across_a_full_turn() {
        let angle_20 = Turns::wrap(Fx::ratio(1, 18));
        let angle_160 = Turns::wrap(Fx::ratio(4, 9));
        let angle_200 = Turns::wrap(Fx::ratio(5, 9));
        let angle_340 = Turns::wrap(Fx::ratio(17, 18));
        let gravities = [angle_20, angle_160, angle_200, angle_340]
            .map(|angle| rotate_inverse(GRAVITY_WORLD, angle));

        assert!(gravities[0].y.raw() > 0);
        assert!(gravities[1].y.raw() < 0);
        assert!(gravities[2].y.raw() < 0);
        assert!(gravities[3].y.raw() > 0);

        let rates = gravities.map(flow_vertical_gravity_raw);
        let min_rate = *rates.iter().min().unwrap();
        let max_rate = *rates.iter().max().unwrap();
        assert!(
            max_rate - min_rate <= GRAVITY_WORLD.y.raw() as u32 / 1_000,
            "symmetric 20-degree offsets should have the same flow rate: {rates:?}"
        );

        let upright_eta = calibrated_remaining_seconds(
            TOTAL_MASS,
            flow_vertical_gravity_raw(rotate_inverse(GRAVITY_WORLD, Turns::ZERO)),
        )
        .unwrap();
        for rate in rates {
            let tilted_eta = calibrated_remaining_seconds(TOTAL_MASS, rate).unwrap();
            assert!(tilted_eta > upright_eta);
            assert!(tilted_eta <= upright_eta + 60);
        }

        for horizontal in [Turns::wrap(Fx::ratio(1, 4)), Turns::wrap(Fx::ratio(3, 4))] {
            let gravity = rotate_inverse(GRAVITY_WORLD, horizontal);
            assert_eq!(flow_vertical_gravity_raw(gravity), 0);
            assert_eq!(calibrated_remaining_seconds(TOTAL_MASS, 0), None);
        }
    }
}
