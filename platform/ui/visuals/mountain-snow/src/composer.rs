//! Streaming one-bit composer: one caller-owned row in, one packed row out.
//!
//! The device never holds the full order field. A 256-bin barrier
//! histogram (one SD streaming pass, 1 KiB) yields a [`Cut`] — one barrier
//! level plus an intra-level blue-noise fraction — whose snow set
//! approximates the host's equal-area prefix. Each mountain-bounds row is
//! then composed independently: registered rock/snow gray samples mix
//! across a narrow noise band at the cut level and resolve to one bit
//! through the same blue-noise threshold. All buffers are caller-owned;
//! nothing is allocated or retained here.

/// Snow selection for one composition: pixels with `barrier < level` are
/// snow, pixels with `barrier == level` compare their noise byte against
/// `frac` (snow when `noise < frac`, `frac` in `0..=256`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cut {
    /// Barrier level at the cut.
    pub level: u8,
    /// Intra-level noise fraction in `0..=256` (256 selects the level).
    pub frac: u16,
}

/// Derive the [`Cut`] whose snow set holds about `target` pixels.
#[must_use]
pub fn cut_from_histogram(hist: &[u32; 256], target: u32) -> Cut {
    let mut below = 0u32;
    for (level, &pop) in hist.iter().enumerate() {
        if pop == 0 {
            continue;
        }
        let end = below.saturating_add(pop);
        if target < end {
            let frac = (u64::from(target - below) * 256 / u64::from(pop)).min(256);
            return Cut {
                level: level as u8,
                frac: frac as u16,
            };
        }
        below = end;
    }
    // `target` at or past the total: select everything.
    Cut {
        level: 255,
        frac: 256,
    }
}

/// Input length problem. All row slices must agree with `width`, and the
/// packed slices must hold `(width + 7) / 8` bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComposerError {
    /// A gray/noise row is not exactly `width` bytes.
    RowLength,
    /// A packed row is not exactly `(width + 7) / 8` bytes.
    PackedLength,
    /// Coverage percent is above 100.
    InvalidPercent,
}

/// One borrowed source row. Bundled so the composer takes four arguments
/// instead of tripping over the seven-argument limit; every slice stays
/// caller-owned.
#[derive(Clone, Copy, Debug)]
pub struct RowInputs<'a> {
    /// Rock-endpoint gray samples, `width` bytes.
    pub rock: &'a [u8],
    /// Snow-endpoint gray samples, `width` bytes.
    pub snow: &'a [u8],
    /// Arrival barrier levels, `width` bytes.
    pub barrier: &'a [u8],
    /// Eligibility bits, MSB-first, `(width + 7) / 8` bytes.
    pub eligible: &'a [u8],
    /// Blue-noise threshold bytes, `width` bytes.
    pub noise: &'a [u8],
}

/// Compose one row into MSB-first packed one-bit pixels (paper = 0).
///
/// `band` (noise units, `0..=128`) widens the material crossover at the
/// cut level: pixels more than `band` past the fraction are solid, the
/// rest mix rock/snow gray before the noise dither. `band == 0` is a hard
/// cut. Non-eligible pixels always resolve to paper.
pub fn compose_row(
    inputs: &RowInputs<'_>,
    cut: Cut,
    band: u8,
    out_packed: &mut [u8],
) -> Result<(), ComposerError> {
    let width = inputs.rock.len();
    if inputs.snow.len() != width || inputs.barrier.len() != width || inputs.noise.len() != width {
        return Err(ComposerError::RowLength);
    }
    let packed_len = width.div_ceil(8);
    if inputs.eligible.len() != packed_len || out_packed.len() != packed_len {
        return Err(ComposerError::PackedLength);
    }
    let band = u16::from(band.min(128));
    for x in 0..width {
        let ink = if inputs.eligible[x / 8] & (0x80 >> (x % 8)) == 0 {
            false
        } else {
            let gray = mixed_gray(
                inputs.rock[x],
                inputs.snow[x],
                inputs.barrier[x],
                inputs.noise[x],
                cut,
                band,
            );
            // Stable ordered dither on the same map, host-exact: ink iff
            // `gray / 255 <= (noise + 0.5) / 256`, in integer arithmetic.
            u32::from(gray) * 256 <= u32::from(inputs.noise[x]) * 255 + 127
        };
        if ink {
            out_packed[x / 8] |= 0x80 >> (x % 8);
        } else {
            out_packed[x / 8] &= !(0x80 >> (x % 8));
        }
    }
    Ok(())
}

/// Compose one row at temperature-derived coverage `percent` (`0..=100`).
///
/// Endpoints dither a single gray plane through the same blue-noise
/// threshold [`compose_row`] uses: `percent == 0` resolves the rock plane,
/// `percent == 100` resolves the snow plane, ignoring `cut`/`band`.
/// Interior coverage (`1..=99`) delegates to [`compose_row`] unchanged, so
/// current output is preserved byte for byte. Returns
/// [`ComposerError::InvalidPercent`] before touching `out_packed` when
/// `percent > 100`; length validation matches [`compose_row`].
pub fn compose_row_for_percent(
    inputs: &RowInputs<'_>,
    percent: u8,
    cut: Cut,
    band: u8,
    out_packed: &mut [u8],
) -> Result<(), ComposerError> {
    if percent > 100 {
        return Err(ComposerError::InvalidPercent);
    }
    if percent == 0 || percent == 100 {
        let width = inputs.rock.len();
        if inputs.snow.len() != width
            || inputs.barrier.len() != width
            || inputs.noise.len() != width
        {
            return Err(ComposerError::RowLength);
        }
        let packed_len = width.div_ceil(8);
        if inputs.eligible.len() != packed_len || out_packed.len() != packed_len {
            return Err(ComposerError::PackedLength);
        }
        let endpoint = if percent == 0 {
            inputs.rock
        } else {
            inputs.snow
        };
        for x in 0..width {
            let mask = 0x80 >> (x % 8);
            let ink = inputs.eligible[x / 8] & mask != 0
                && u32::from(endpoint[x]) * 256 <= u32::from(inputs.noise[x]) * 255 + 127;
            if ink {
                out_packed[x / 8] |= mask;
            } else {
                out_packed[x / 8] &= !mask;
            }
        }
        return Ok(());
    }
    compose_row(inputs, cut, band, out_packed)
}

/// Mixed gray sample in `0..=255` for one eligible pixel.
fn mixed_gray(rock: u8, snow: u8, barrier: u8, noise: u8, cut: Cut, band: u16) -> u8 {
    if barrier < cut.level {
        return snow;
    }
    if barrier > cut.level {
        return rock;
    }
    let distance = i32::from(cut.frac) - i32::from(noise);
    if band == 0 {
        return if distance >= 0 { snow } else { rock };
    }
    let band = i32::from(band);
    if distance >= band {
        return snow;
    }
    if distance <= -band {
        return rock;
    }
    // Fixed-point blend across the band; products fit `u32` (255 * 254).
    let num = (distance + band) as u32;
    let den = (2 * band) as u32;
    ((u32::from(rock) * (den - num) + u32::from(snow) * num) / den) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs<'a>(
        rock: &'a [u8; 8],
        snow: &'a [u8; 8],
        barrier: &'a [u8; 8],
        eligible: &'a [u8; 1],
        noise: &'a [u8; 8],
    ) -> RowInputs<'a> {
        RowInputs {
            rock,
            snow,
            barrier,
            eligible,
            noise,
        }
    }

    #[test]
    fn cut_targets_exact_level_prefixes() {
        let mut hist = [0u32; 256];
        hist[10] = 100;
        hist[20] = 200;
        assert_eq!(cut_from_histogram(&hist, 0), Cut { level: 10, frac: 0 });
        assert_eq!(
            cut_from_histogram(&hist, 99),
            Cut {
                level: 10,
                frac: 253
            }
        );
        assert_eq!(cut_from_histogram(&hist, 100), Cut { level: 20, frac: 0 });
        assert_eq!(
            cut_from_histogram(&hist, 10_000),
            Cut {
                level: 255,
                frac: 256
            }
        );
    }

    #[test]
    fn solid_rows_select_endpoints() {
        let rock = [60u8; 8];
        let snow = [220u8; 8];
        let barrier = [0u8; 8];
        let eligible = [0xFFu8];
        let mut out = [0u8];
        // Cut past every barrier: all snow. Gray 220 against noise 250:
        // 256*220 = 56320 <= 255*250 + 127 = 63877, so ink.
        let noise = [250u8; 8];
        compose_row(
            &inputs(&rock, &snow, &barrier, &eligible, &noise),
            Cut {
                level: 0,
                frac: 256,
            },
            0,
            &mut out,
        )
        .expect("lengths agree");
        assert_eq!(out, [0xFF]);
        // Cut below every barrier: all rock. Gray 60 against noise 0:
        // 256*60 = 15360 > 127, so paper; ink comes from the dither map.
        let barrier = [200u8; 8];
        let noise = [0u8; 8];
        compose_row(
            &inputs(&rock, &snow, &barrier, &eligible, &noise),
            Cut { level: 0, frac: 0 },
            0,
            &mut out,
        )
        .expect("lengths agree");
        assert_eq!(out, [0x00]);
        let eligible = [0x00u8];
        compose_row(
            &inputs(&rock, &snow, &barrier, &eligible, &noise),
            Cut {
                level: 0,
                frac: 256,
            },
            0,
            &mut out,
        )
        .expect("lengths agree");
        assert_eq!(out, [0x00]);
    }

    #[test]
    fn paper_white_never_inks_and_black_always_does() {
        let barrier = [0u8; 8];
        let eligible = [0xFFu8];
        let mut out = [0u8];
        for noise_byte in [0u8, 127, 255] {
            let noise = [noise_byte; 8];
            let white = [255u8; 8];
            compose_row(
                &inputs(&white, &white, &barrier, &eligible, &noise),
                Cut {
                    level: 0,
                    frac: 256,
                },
                0,
                &mut out,
            )
            .expect("lengths agree");
            assert_eq!(out, [0x00], "paper at noise {noise_byte}");
            let black = [0u8; 8];
            compose_row(
                &inputs(&black, &black, &barrier, &eligible, &noise),
                Cut {
                    level: 0,
                    frac: 256,
                },
                0,
                &mut out,
            )
            .expect("lengths agree");
            assert_eq!(out, [0xFF], "ink at noise {noise_byte}");
        }
    }

    #[test]
    fn band_midpoint_mixes_before_dither() {
        // rock 60, snow 220, band 64, distance 0 -> num=64, den=128 ->
        // gray = (60*64 + 220*64)/128 = 140. Ink iff
        // 256*140 = 35840 <= 255*noise + 127, i.e. noise >= 141.
        let rock = [60u8; 8];
        let snow = [220u8; 8];
        let barrier = [7u8; 8];
        let eligible = [0xFFu8];
        let mut out = [0u8];
        let noise = [0u8; 8];
        compose_row(
            &inputs(&rock, &snow, &barrier, &eligible, &noise),
            Cut {
                level: 7,
                frac: 128,
            },
            64,
            &mut out,
        )
        .expect("lengths agree");
        assert_eq!(out, [0x00]);
        let noise = [200u8; 8];
        compose_row(
            &inputs(&rock, &snow, &barrier, &eligible, &noise),
            Cut {
                level: 7,
                frac: 128,
            },
            64,
            &mut out,
        )
        .expect("lengths agree");
        assert_eq!(out, [0xFF]);
    }

    #[test]
    fn length_mismatches_are_rejected() {
        let row = [0u8; 8];
        let short_row = [0u8; 7];
        let packed = [0u8; 1];
        let mut out = [0u8; 1];
        let cut = Cut { level: 0, frac: 0 };
        let bad = RowInputs {
            rock: &short_row,
            snow: &row,
            barrier: &row,
            eligible: &packed,
            noise: &row,
        };
        assert_eq!(
            compose_row(&bad, cut, 0, &mut out),
            Err(ComposerError::RowLength)
        );
        let mut short = [0u8; 0];
        let good = inputs(&row, &row, &row, &packed, &row);
        assert_eq!(
            compose_row(&good, cut, 0, &mut short),
            Err(ComposerError::PackedLength)
        );
    }
}
