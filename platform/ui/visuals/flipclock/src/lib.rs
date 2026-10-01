//! Split-flap ("flip clock") digit rendering.
//!
//! Split into a perspective rasterizer ([`flap`]), the static chrome around it
//! ([`housing`]), the halftone screens both draw tone through ([`screen`]), and
//! the build-time digit cards ([`digits`]). Everything is `no_std` and
//! integer-only, and everything writes through [`raster::Surface`] -- the same
//! one-bit destination `enso` and `hourglass` draw into -- so the host preview
//! and a firmware canvas share one implementation.
//!
//! This crate names no LVGL type and no hardware type. The surface adapter
//! lives beside the screen that owns the canvas, in whichever product uses
//! this crate. A pure, chip-neutral visual model: filed under `products/
//! medinote` only by accident of history, reusable by any product that wants
//! a split-flap digit display.

#![no_std]

pub mod digits;
pub mod flap;
pub mod housing;
pub mod screen;

use digits::{CARD_H, CARD_W};

/// Surface geometry, shared by the firmware canvas and the host preview so the
/// two cannot drift.
///
/// Both dimensions sit on the ST7305's partial-window granularity -- 8 px in
/// logical X, 12 px in logical Y -- so one card's dirty box never drags a
/// neighbouring column group in with it. The width carries the free edge's
/// overhang, since the leaf is always wider than the card; the height carries
/// the lean above the card top plus the drop shadow below.
/// Wide enough that the case walls clear the leaf's widest overhang. The leaf
/// reaches `half_w * camera_d / (camera_d - half_h)` at edge-on -- 25 percent
/// past the card here -- and the walls have to sit outside that or they clip
/// the taper, which is the whole three-dimensional cue.
pub const SURFACE_W: i32 = 128;
/// Tall enough for the lean above the card top plus the drop shadow below.
pub const SURFACE_H: i32 = 144;
pub const HINGE_X: i32 = CARD_W / 2 + 24;
pub const HINGE_Y: i32 = CARD_H / 2 + 6;
pub const CAMERA_D: i32 = 300;

pub const fn card() -> flap::Card {
    flap::Card {
        hinge_x: HINGE_X,
        hinge_y: HINGE_Y,
        half_w: CARD_W / 2,
        half_h: CARD_H / 2,
        camera_d: CAMERA_D,
    }
}

// ---------------------------------------------------------------------------
// Tuning
// ---------------------------------------------------------------------------

/// How long one complete flip takes, from the leaf leaving rest to settling.
///
/// The panel self-refreshes at about 26 Hz and its own response is the
/// bottleneck, not the write path, so this divided by ~38 ms is roughly how
/// many distinct poses actually reach the glass. Below about 250 ms the flip
/// reads as a jump; above about 700 ms it reads as slow motion.
pub const FLIP_DURATION_MS: u32 = 460;

/// How progress is spent across [`FLIP_DURATION_MS`]. See [`flap::Easing`].
pub const FLIP_EASING: flap::Easing = flap::Easing::UniformCosine;

/// How many poses the host preview samples a flip at. The device animates from
/// its own tick and does not use this.
pub const PREVIEW_FRAMES: u32 = 12;

// ---------------------------------------------------------------------------

/// A digit that flips to the next one on demand, and the clock driving it.
///
/// Owns no time source: the caller starts a flip and then feeds it elapsed
/// milliseconds, so the same type drives the firmware's 30 Hz runtime tick and
/// the host preview's fixed samples.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FlipClock {
    digit: u8,
    /// Milliseconds into the current flip; `None` at rest.
    elapsed_ms: Option<u32>,
}

impl FlipClock {
    pub const fn new(digit: u8) -> Self {
        Self {
            digit: digit % 10,
            elapsed_ms: None,
        }
    }

    /// A clock parked partway through a flip, for previews and tests.
    pub const fn at(digit: u8, elapsed_ms: u32) -> Self {
        Self {
            digit: digit % 10,
            elapsed_ms: Some(elapsed_ms),
        }
    }

    /// Begins a flip to the next digit.
    ///
    /// Pressing again mid-flip completes the one in progress and starts the
    /// next rather than restarting or stalling, so repeated presses always
    /// advance the digit once each.
    pub fn start(&mut self) {
        if self.elapsed_ms.is_some() {
            self.digit = self.to();
        }
        self.elapsed_ms = Some(0);
    }

    /// Advances the clock. Returns whether a repaint is needed -- true while
    /// animating and on the tick that settles.
    pub fn advance(&mut self, delta_ms: u32) -> bool {
        let Some(elapsed) = self.elapsed_ms else {
            return false;
        };
        let elapsed = elapsed.saturating_add(delta_ms);
        if elapsed >= FLIP_DURATION_MS {
            self.digit = self.to();
            self.elapsed_ms = None;
        } else {
            self.elapsed_ms = Some(elapsed);
        }
        true
    }

    pub const fn is_animating(&self) -> bool {
        self.elapsed_ms.is_some()
    }

    /// The digit the leaf is flipping away from -- the one on the glass at
    /// rest.
    pub const fn from(&self) -> u8 {
        self.digit
    }

    pub const fn to(&self) -> u8 {
        (self.digit + 1) % 10
    }

    pub const fn pose(&self) -> flap::Pose {
        match self.elapsed_ms {
            None => flap::Pose::at(0),
            Some(elapsed) => {
                flap::Pose::at(FLIP_EASING.apply(elapsed * flap::PROGRESS_MAX / FLIP_DURATION_MS))
            }
        }
    }

    pub fn faces(&self) -> flap::Faces<'static> {
        flap::Faces {
            outgoing_top: digits::half(self.from(), false),
            outgoing_bottom: digits::half(self.from(), true),
            incoming_top: digits::half(self.to(), false),
            incoming_bottom: digits::half(self.to(), true),
        }
    }
}

#[cfg(test)]
mod tests {
    use raster::Surface;

    use super::flap::{Easing, Pose, PROGRESS_MAX};
    use super::*;

    /// Records a single column, so alignment can be checked without a whole
    /// framebuffer on a `no_std` test stack.
    struct Probe {
        x: i32,
        column: [bool; SURFACE_H as usize],
    }

    impl Surface for Probe {
        fn width(&self) -> i32 {
            SURFACE_W
        }

        fn height(&self) -> i32 {
            SURFACE_H
        }

        fn set(&mut self, x: i32, y: i32, ink: bool) {
            if x != self.x {
                return;
            }
            if let Some(slot) = usize::try_from(y).ok().and_then(|y| self.column.get_mut(y)) {
                *slot = ink;
            }
        }
    }

    /// At rest the leaf lies flat against the card, so it must reproduce the
    /// static half it covers *exactly*.
    ///
    /// A one-row indexing slip between the two is nearly invisible in a still
    /// frame -- the hinge line hides the seam -- but on the glass it reads as
    /// the digit stepping sideways the moment a flip begins, because the
    /// moving leaf carries the glyph at a different offset from the half it is
    /// meant to be continuous with.
    #[test]
    fn a_resting_card_shows_its_digit_exactly_as_the_bitmap_has_it() {
        const DIGIT: u8 = 2;
        let card = card();
        let clock = FlipClock::new(DIGIT);
        let bitmap = digits::card(DIGIT);
        let half = card.half_h;

        for x in [
            card.hinge_x - 24,
            card.hinge_x - 8,
            card.hinge_x,
            card.hinge_x + 17,
        ] {
            let mut probe = Probe {
                x,
                column: [false; SURFACE_H as usize],
            };
            flap::render_frame(
                &mut probe,
                &card,
                clock.pose(),
                &clock.faces(),
                Default::default(),
            );
            let column = x - (card.hinge_x - card.half_w);

            // Row 0 and the last row are the border caps, and the row just
            // below the hinge is its paper highlight; the renderer owns those.
            for row in 1..half {
                let y = (card.hinge_y - half + row) as usize;
                assert_eq!(
                    probe.column[y],
                    bitmap.ink(column, row),
                    "top half differs at column {column}, row {row}"
                );
            }
            for row in (half + 1)..(2 * half - 1) {
                let y = (card.hinge_y + 1 + row - half) as usize;
                assert_eq!(
                    probe.column[y],
                    bitmap.ink(column, row),
                    "bottom half differs at column {column}, row {row}"
                );
            }
        }
    }

    fn probe(clock: &FlipClock, x: i32) -> [bool; SURFACE_H as usize] {
        let card = card();
        let mut surface = Probe {
            x,
            column: [false; SURFACE_H as usize],
        };
        flap::render_frame(
            &mut surface,
            &card,
            clock.pose(),
            &clock.faces(),
            Default::default(),
        );
        surface.column
    }

    /// A flip must land on exactly the image the next flip starts from.
    ///
    /// The two are different code paths -- landed is the rising phase with the
    /// leaf covering the bottom half, resting is the falling phase with it
    /// covering the top -- so anything either path owns exclusively shows up
    /// as a one-frame flicker at every digit change. The hinge's paper lip did
    /// exactly that until it was moved out of both halves.
    #[test]
    fn a_flip_lands_on_the_image_the_next_flip_rests_on() {
        let landed = FlipClock::at(2, FLIP_DURATION_MS);
        let resting = FlipClock::new(3);
        assert_eq!(landed.to(), resting.from());
        for x in [40, 52, 64, 76, 88] {
            assert_eq!(
                probe(&landed, x),
                probe(&resting, x),
                "landed and resting differ at column {x}"
            );
        }
    }

    fn sweep() -> impl Iterator<Item = Pose> {
        (0..=PROGRESS_MAX).map(Pose::at)
    }

    /// The bug that motivated deriving everything from one parameter: a
    /// hand-authored `cos` above 1 projected the leaf longer than the card it
    /// lands on. It is not expressible now, and this pins that down.
    #[test]
    fn the_cosine_never_leaves_its_range_and_the_sine_never_goes_negative() {
        for pose in sweep() {
            assert!(pose.cos_q15().abs() <= 1 << 15, "cos out of range");
            assert!(pose.sin_q15() >= 0, "sin went negative");
        }
    }

    #[test]
    fn the_angle_stays_on_the_unit_circle() {
        for pose in sweep() {
            let one = (1i64 << 15) * (1i64 << 15);
            let magnitude = i64::from(pose.cos_q15()).pow(2) + i64::from(pose.sin_q15()).pow(2);
            // One integer square root of slack.
            assert!((magnitude - one).abs() < one / 1000, "off the unit circle");
        }
    }

    /// A flip is one continuous rotation, so it crosses vertical exactly once.
    /// Any second crossing would be the leaf changing which half it is.
    #[test]
    fn the_phase_changes_exactly_once_across_a_flip() {
        let mut crossings = 0;
        let mut previous = Pose::at(0).phase();
        for progress in 1..=PROGRESS_MAX {
            let phase = Pose::at(progress).phase();
            if phase != previous {
                crossings += 1;
                previous = phase;
            }
        }
        assert_eq!(crossings, 1);
    }

    /// Every derived quantity scales with `sin t`, so all three must vanish at
    /// both resting ends and peak together at edge-on. A quantity that peaked
    /// somewhere else would be an outlier by definition.
    #[test]
    fn the_derived_quantities_vanish_at_rest_and_peak_together() {
        let rest = Pose::at(0);
        assert_eq!(
            (rest.face_dither(), rest.shadow_px(), rest.edge_px()),
            (0, 0, 0)
        );
        let landed = Pose::at(PROGRESS_MAX);
        assert_eq!(
            (landed.face_dither(), landed.shadow_px(), landed.edge_px()),
            (0, 0, 0)
        );

        // Edge-on is where `sin t` is greatest; every derived quantity scales
        // with it, so each must reach its own maximum at that same progress.
        let edge_on = (0..=PROGRESS_MAX)
            .max_by_key(|progress| Pose::at(*progress).sin_q15())
            .unwrap();
        for extract in [Pose::face_dither, Pose::shadow_px, Pose::edge_px] {
            let highest = (0..=PROGRESS_MAX)
                .map(|progress| extract(Pose::at(progress)))
                .max()
                .unwrap();
            assert_eq!(extract(Pose::at(edge_on)), highest, "peaks disagree");
        }
    }

    /// The shadow is thrown by a leaf descending onto the card, so it starts
    /// light and finishes dark. Scaling it by `sin t` like the other derived
    /// quantities would make it symmetric about edge-on, which reads as the
    /// same shadow in every frame.
    #[test]
    fn the_cast_shadow_darkens_as_the_leaf_descends() {
        let lifting = Pose::at(PROGRESS_MAX / 8);
        let landing = Pose::at(PROGRESS_MAX * 3 / 4);
        assert!(lifting.shadow_level() < landing.shadow_level());

        // The band also tightens as it darkens: far-end weight closes on the
        // anchor's as the leaf lands, so a short late band still reads heavy.
        // Landing keeps at least three quarters of its weight to the far
        // edge; lifted loses at least half.
        assert!(
            landing.shadow_falloff_level() * 4 >= landing.shadow_level() * 3,
            "landing shadow washed out across its band"
        );
        assert!(
            lifting.shadow_falloff_level() * 2 <= lifting.shadow_level(),
            "lifted shadow held its weight to the far edge"
        );

        // And it never reverses while the leaf keeps rotating the same way.
        let mut previous = 0;
        for progress in 0..=(PROGRESS_MAX * 7 / 8) {
            let level = Pose::at(progress).shadow_level();
            assert!(level >= previous, "shadow lightened mid-fall");
            previous = level;
        }
    }

    /// The leaf is a rigid card: it can never project further from the hinge
    /// than the card it lands on, beyond the small lean towards the camera
    /// that low tilt legitimately produces.
    #[test]
    fn the_leaf_never_runs_away_from_the_card() {
        let card = card();
        for pose in sweep() {
            let reach = flap::free_edge_row(&card, pose);
            assert!(
                reach <= card.half_h + card.half_h / 16,
                "leaf reached {reach} against a {} half-card",
                card.half_h
            );
        }
    }

    #[test]
    fn a_flip_starts_at_rest_and_settles_on_the_next_digit() {
        let mut clock = FlipClock::new(7);
        assert!(!clock.is_animating());
        assert_eq!((clock.from(), clock.to()), (7, 8));

        clock.start();
        assert!(clock.is_animating());
        assert!(clock.advance(FLIP_DURATION_MS / 2));
        assert!(clock.is_animating());

        assert!(clock.advance(FLIP_DURATION_MS));
        assert!(!clock.is_animating());
        assert_eq!(clock.from(), 8);
    }

    #[test]
    fn the_digit_wraps_from_nine_to_zero() {
        let mut clock = FlipClock::new(9);
        assert_eq!(clock.to(), 0);
        clock.start();
        clock.advance(FLIP_DURATION_MS);
        assert_eq!(clock.from(), 0);
    }

    /// Pressing again mid-flip must advance the digit once, not restart the
    /// same flip or drop the press.
    #[test]
    fn restarting_mid_flip_completes_the_one_in_progress() {
        let mut clock = FlipClock::new(3);
        clock.start();
        clock.advance(FLIP_DURATION_MS / 3);
        clock.start();
        assert_eq!((clock.from(), clock.to()), (4, 5));
        assert!(clock.is_animating());
    }

    /// At rest the leaf is flush with the card: no tilt, and so no shading,
    /// shadow or visible edge.
    #[test]
    fn a_clock_at_rest_is_flat() {
        let pose = FlipClock::new(0).pose();
        assert_eq!(pose.sin_q15(), 0);
        assert_eq!(
            (pose.face_dither(), pose.shadow_px(), pose.edge_px()),
            (0, 0, 0)
        );
    }

    /// Easing changes the timing and nothing else: whatever curve is chosen,
    /// the flip still starts and ends at rest and still crosses vertical once.
    #[test]
    fn every_easing_curve_spans_the_same_sweep() {
        for easing in [Easing::UniformCosine, Easing::Gravity, Easing::Smooth] {
            assert_eq!(easing.apply(0), 0);
            assert_eq!(easing.apply(PROGRESS_MAX), PROGRESS_MAX);
            let mut previous = 0;
            for progress in 0..=PROGRESS_MAX {
                let eased = easing.apply(progress);
                assert!(eased >= previous, "{easing:?} went backwards");
                assert!(eased <= PROGRESS_MAX, "{easing:?} overshot");
                previous = eased;
            }
        }
    }
}
