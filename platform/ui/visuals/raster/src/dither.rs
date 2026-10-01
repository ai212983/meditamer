//! Ordered dithering: how continuous ink coverage resolves onto a panel that
//! has exactly one bit per pixel.
//!
//! Every mode here is a pure function of the pixel coordinate -- a threshold
//! field, not a running error term. That is a hard requirement rather than a
//! simplification, because of how the Inkplate partial waveform works. A drawing
//! that grows monotonically (ink is added and never removed) flips each pixel
//! at most once for the whole of its animation, which is the best case the
//! panel offers: no reverse transitions, so no ghosting accumulates. Error
//! diffusion would break that. Its output at a pixel depends on error carried
//! in from neighbours, so advancing the animation re-derives the entire
//! pattern and pixels flip *both* ways -- visible churn in a region that should
//! only be filling in. See `docs/references/display-refresh.md`.

/// The fractional part, always in `0.0..1.0`.
fn fract(x: f32) -> f32 {
    x - libm::floorf(x)
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Dither {
    /// A hard 50% threshold. No texture: coverage is either ink or paper.
    None,
    /// The classic 4x4 ordered Bayer matrix.
    ///
    /// Cheap and familiar, but its 4-pixel period is plainly visible as a
    /// crosshatch across large, smoothly varying areas -- which is exactly what
    /// a brush stroke's bled edge is. Prefer [`Dither::Gradient`] there.
    Bayer4,
    /// Interleaved gradient noise: isotropic, no repeating grid, and reads as
    /// paper grain rather than as a screen.
    ///
    /// This is a cheap approximation of blue noise, not the real thing. A
    /// void-and-cluster tile would be spectrally better; it would also be a
    /// baked table rather than three multiplies, and swapping it in later means
    /// changing this one arm.
    #[default]
    Gradient,
}

impl Dither {
    /// The threshold for one pixel, in `0.0..1.0`. Ink the pixel when its
    /// coverage exceeds this.
    pub fn threshold(self, x: i32, y: i32) -> f32 {
        match self {
            Dither::None => 0.5,
            Dither::Bayer4 => {
                const MATRIX: [[u8; 4]; 4] =
                    [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
                let cell = MATRIX[y.rem_euclid(4) as usize][x.rem_euclid(4) as usize];
                // Offset by half a step so the matrix straddles its levels
                // instead of clipping one end.
                (cell as f32 + 0.5) * (1.0 / 16.0)
            }
            Dither::Gradient => {
                // Jimenez's interleaved gradient noise. The constants are the
                // published ones, trimmed to what an `f32` can actually hold
                // (the paper writes the last as 52.9829189); they are
                // irrational-ish on purpose, so the pattern never closes into a
                // visible tile.
                let magic = 0.06711056 * x as f32 + 0.00583715 * y as f32;
                fract(52.982_918 * fract(magic))
            }
        }
    }

    /// Whether `coverage` (in `0.0..=1.0`) survives the threshold at this pixel.
    #[inline]
    pub fn inks(self, coverage: f32, x: i32, y: i32) -> bool {
        coverage > self.threshold(x, y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODES: [Dither; 3] = [Dither::None, Dither::Bayer4, Dither::Gradient];

    #[test]
    fn every_threshold_stays_in_the_unit_range() {
        for mode in MODES {
            for y in -8..600 {
                for x in -8..600 {
                    let value = mode.threshold(x, y);
                    assert!(
                        (0.0..=1.0).contains(&value),
                        "{mode:?} gave {value} at ({x}, {y})"
                    );
                }
            }
        }
    }

    /// Negative coordinates must not panic or fold onto a different phase --
    /// a caller can legitimately sample just outside the canvas.
    #[test]
    fn bayer_wraps_negative_coordinates_by_period() {
        for offset in 0..4 {
            assert_eq!(
                Dither::Bayer4.threshold(offset, offset),
                Dither::Bayer4.threshold(offset - 4, offset - 4)
            );
        }
    }

    #[test]
    fn full_coverage_always_inks_and_empty_never_does() {
        for mode in MODES {
            for y in 0..64 {
                for x in 0..64 {
                    assert!(mode.inks(1.01, x, y), "{mode:?} refused full coverage");
                    assert!(!mode.inks(0.0, x, y), "{mode:?} inked empty coverage");
                }
            }
        }
    }

    /// The property that makes a dither usable as a tone ramp: over an area,
    /// the inked fraction should track the requested coverage.
    #[test]
    fn inked_fraction_tracks_requested_coverage() {
        for mode in [Dither::Bayer4, Dither::Gradient] {
            for step in 1..10 {
                let coverage = step as f32 / 10.0;
                let mut inked = 0;
                for y in 0..64 {
                    for x in 0..64 {
                        if mode.inks(coverage, x, y) {
                            inked += 1;
                        }
                    }
                }
                let fraction = inked as f32 / (64.0 * 64.0);
                assert!(
                    (fraction - coverage).abs() < 0.06,
                    "{mode:?} rendered {coverage} coverage as {fraction}"
                );
            }
        }
    }

    /// The monotone guarantee the partial waveform depends on: raising coverage
    /// can only ever add ink at a pixel, never take it away.
    #[test]
    fn rising_coverage_never_un_inks_a_pixel() {
        for mode in MODES {
            for y in 0..32 {
                for x in 0..32 {
                    let mut was_inked = false;
                    for step in 0..=100 {
                        let inked = mode.inks(step as f32 / 100.0, x, y);
                        assert!(
                            inked || !was_inked,
                            "{mode:?} un-inked ({x}, {y}) at step {step}"
                        );
                        was_inked |= inked;
                    }
                }
            }
        }
    }
}
