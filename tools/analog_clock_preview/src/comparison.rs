//! Host-only composition comparison metrics for the analog-clock preview.
//!
//! Compares one rendered frame against a previous frame and a static base:
//! how much changed, whether change stayed inside the hand/shadow support,
//! whether newly uncovered (disoccluded) pixels match the cached static
//! base, and how the dithered binary result tracks the continuous gray
//! target. All buffers are standard host heap (`Vec`, slices); there are no
//! embedded statics and nothing here touches firmware, DRAM segments, or
//! the network.
//!
//! Conventions (matching `raster::BitCanvas`): packed planes are MSB-first
//! (`1` ink/black, `0` paper/white); tail bits past `width * height` are
//! never read. `current_gray` holds sRGB bytes (`0` black, `255` white),
//! one per pixel. Footprint planes hold one byte per pixel: `0` is
//! unchanged stationary dial, nonzero is actual continuous hand coverage /
//! shadow contribution (flags `1`/`2` are opaque presence markers, not
//! region labels). Tone uses 16x16 tiles (partial borders included) with
//! both sides in the SAME encoded quantizer domain: the sRGB byte domain
//! where `255` is white, so a window's binary mean is its white fraction
//! times 255. `tone_bias` is the count-weighted signed mean of
//! `binary - target`, `tone_mae` the weighted mean of absolute window
//! differences. Pixel-wise binary-vs-gray MAE is NOT computed: it is a
//! meaningless halftone error. `boundary_mae` covers only windows holding
//! a footprint-boundary pixel (nonzero with a zero 4-neighbor, checked
//! image-wide so tile-edge boundaries count); with no boundary window it
//! is `0.0` (no boundary, not perfect tone). No dither/target identity is
//! assumed; spectral quality is unmeasured.

/// Frame-to-frame comparison of one rendered composition.
#[derive(Debug, Clone, PartialEq)]
pub struct ComparisonMetrics {
    /// Logical pixels where current and previous bits differ.
    pub changed_total: usize,
    /// Changed pixels with both footprints zero (outside the union support).
    pub changed_outside: usize,
    /// Uncovered pixels (current fp 0, previous fp nonzero) whose current
    /// bit differs from the static base bit (stale cache restore).
    pub disocclusion_mismatches: usize,
    /// Count-weighted mean absolute window tone error, units `0..255`.
    pub tone_mae: f64,
    /// Count-weighted signed mean window tone error (`binary - target`).
    pub tone_bias: f64,
    /// Tone MAE over footprint-boundary windows only, else `0.0`.
    pub boundary_mae: f64,
}

const TILE: usize = 16;
const MAX_DIM: u32 = 1200;

fn checked_pixels(width: u32, height: u32) -> Result<usize, String> {
    if width == 0 || height == 0 {
        return Err("dimensions must be nonzero".to_string());
    }
    if width > MAX_DIM || height > MAX_DIM {
        return Err("dimensions exceed host bound 1200".to_string());
    }
    (width as usize)
        .checked_mul(height as usize)
        .ok_or_else(|| "dimensions overflow".to_string())
}

fn packed_len(pixels: usize) -> usize {
    pixels.div_ceil(8)
}

fn bit_at(bits: &[u8], pixel: usize) -> bool {
    bits[pixel / 8] & (0x80 >> (pixel % 8)) != 0
}

/// Validates dimensions and every plane length before any pixel is indexed.
// Eight planes are the comparison contract (current/previous/base); a
// struct would only alias the call sites without removing a parameter.
#[allow(clippy::too_many_arguments)]
fn validate_analyze(
    width: u32,
    height: u32,
    current_gray: &[u8],
    current_bits: &[u8],
    previous_bits: &[u8],
    current_footprint: &[u8],
    previous_footprint: &[u8],
    base_bits: &[u8],
) -> Result<(usize, usize), String> {
    let pixels = checked_pixels(width, height)?;
    let packed = packed_len(pixels);
    if current_gray.len() != pixels {
        return Err("current_gray length mismatch".to_string());
    }
    if current_footprint.len() != pixels || previous_footprint.len() != pixels {
        return Err("footprint length mismatch".to_string());
    }
    if current_bits.len() != packed || previous_bits.len() != packed || base_bits.len() != packed {
        return Err("packed bit plane length mismatch".to_string());
    }
    Ok((pixels, packed))
}

fn change_counts(
    pixels: usize,
    current_bits: &[u8],
    previous_bits: &[u8],
    current_footprint: &[u8],
    previous_footprint: &[u8],
    base_bits: &[u8],
) -> (usize, usize, usize) {
    let mut changed_total = 0usize;
    let mut changed_outside = 0usize;
    let mut disocclusion_mismatches = 0usize;
    for i in 0..pixels {
        let cb = bit_at(current_bits, i);
        if cb != bit_at(previous_bits, i) {
            changed_total += 1;
            if current_footprint[i] == 0 && previous_footprint[i] == 0 {
                changed_outside += 1;
            }
        }
        if current_footprint[i] == 0 && previous_footprint[i] != 0 && cb != bit_at(base_bits, i) {
            disocclusion_mismatches += 1;
        }
    }

    (changed_total, changed_outside, disocclusion_mismatches)
}

/// Analyzes one frame against the previous frame and the static base.
// Same eight-plane contract as `validate_analyze` above.
#[allow(clippy::too_many_arguments)]
pub fn analyze(
    width: u32,
    height: u32,
    current_gray: &[u8],
    current_bits: &[u8],
    previous_bits: &[u8],
    current_footprint: &[u8],
    previous_footprint: &[u8],
    base_bits: &[u8],
) -> Result<ComparisonMetrics, String> {
    let (pixels, _) = validate_analyze(
        width,
        height,
        current_gray,
        current_bits,
        previous_bits,
        current_footprint,
        previous_footprint,
        base_bits,
    )?;
    let w = width as usize;
    let h = height as usize;

    let (changed_total, changed_outside, disocclusion_mismatches) = change_counts(
        pixels,
        current_bits,
        previous_bits,
        current_footprint,
        previous_footprint,
        base_bits,
    );

    let mut bias_num = 0.0f64;
    let mut mae_num = 0.0f64;
    let mut boundary_num = 0.0f64;
    let mut boundary_pixels = 0usize;
    let nx = w.div_ceil(TILE);
    let ny = h.div_ceil(TILE);
    for ty in 0..ny {
        for tx in 0..nx {
            let x0 = tx * TILE;
            let y0 = ty * TILE;
            let x1 = (x0 + TILE).min(w);
            let y1 = (y0 + TILE).min(h);
            let mut gray_sum = 0u64;
            let mut ink = 0u64;
            let mut boundary = false;
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = y * w + x;
                    gray_sum += u64::from(current_gray[i]);
                    if bit_at(current_bits, i) {
                        ink += 1;
                    }
                    if !boundary && current_footprint[i] != 0 {
                        let open = |xx: isize, yy: isize| -> bool {
                            xx < 0
                                || yy < 0
                                || xx >= w as isize
                                || yy >= h as isize
                                || current_footprint[yy as usize * w + xx as usize] == 0
                        };
                        // Full-image 4-neighborhood: a boundary on a tile
                        // edge still marks both adjacent windows.
                        if open(x as isize - 1, y as isize)
                            || open(x as isize + 1, y as isize)
                            || open(x as isize, y as isize - 1)
                            || open(x as isize, y as isize + 1)
                        {
                            boundary = true;
                        }
                    }
                }
            }
            let n = ((x1 - x0) * (y1 - y0)) as f64;
            let target = gray_sum as f64 / n;
            let binary = (n - ink as f64) / n * 255.0;
            let diff = binary - target;
            bias_num += n * diff;
            mae_num += n * diff.abs();
            if boundary {
                boundary_num += n * diff.abs();
                boundary_pixels += (x1 - x0) * (y1 - y0);
            }
        }
    }
    let total = pixels as f64;
    Ok(ComparisonMetrics {
        changed_total,
        changed_outside,
        disocclusion_mismatches,
        tone_mae: mae_num / total,
        tone_bias: bias_num / total,
        boundary_mae: if boundary_pixels == 0 {
            0.0
        } else {
            boundary_num / boundary_pixels as f64
        },
    })
}

fn validate_overlay(
    width: u32,
    height: u32,
    current_bits: &[u8],
    previous_bits: &[u8],
) -> Result<usize, String> {
    let pixels = checked_pixels(width, height)?;
    let packed = packed_len(pixels);
    if current_bits.len() != packed || previous_bits.len() != packed {
        return Err("packed bit plane length mismatch".to_string());
    }
    pixels
        .checked_mul(3)
        .ok_or_else(|| "overlay size overflow".to_string())?;
    Ok(pixels)
}

/// Current frame as grayscale except changed pixels in red (dark red =
/// current ink, light red = current paper). Returns `3 * w * h` RGB bytes.
pub fn changed_overlay_rgb(
    width: u32,
    height: u32,
    current_bits: &[u8],
    previous_bits: &[u8],
) -> Result<Vec<u8>, String> {
    let pixels = validate_overlay(width, height, current_bits, previous_bits)?;
    let mut out = Vec::with_capacity(pixels * 3);
    for i in 0..pixels {
        let cb = bit_at(current_bits, i);
        let changed = cb != bit_at(previous_bits, i);
        let px = match (changed, cb) {
            (true, true) => [176, 0, 0],
            (true, false) => [255, 150, 150],
            (false, true) => [0, 0, 0],
            (false, false) => [255, 255, 255],
        };
        out.extend_from_slice(&px);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zeros(pixels: usize) -> Vec<u8> {
        vec![0u8; pixels]
    }

    fn packed_zeros(pixels: usize) -> Vec<u8> {
        vec![0u8; packed_len(pixels)]
    }

    fn set_bit(bits: &mut [u8], pixel: usize) {
        bits[pixel / 8] |= 0x80 >> (pixel % 8);
    }

    #[test]
    fn packed_msb_order_and_tail_bits_ignored() {
        // 9x1: two bytes, tail is byte1 low 7 bits (pixels 9..15).
        let w = 9u32;
        let h = 1u32;
        let mut cur = packed_zeros(9);
        let mut prev = packed_zeros(9);
        set_bit(&mut cur, 0);
        set_bit(&mut cur, 8);
        cur[1] |= 0x7F;
        prev[1] |= 0x7F;
        let m = analyze(w, h, &zeros(9), &cur, &prev, &zeros(9), &zeros(9), &prev).unwrap();
        assert_eq!(m.changed_total, 2);
        // Tail-only difference is invisible.
        let mut prev2 = prev.clone();
        prev2[1] ^= 0x01;
        let m2 = analyze(w, h, &zeros(9), &cur, &prev2, &zeros(9), &zeros(9), &prev).unwrap();
        assert_eq!(m2.changed_total, 2);
    }

    #[test]
    fn tone_bias_and_mae_calibration() {
        let g = vec![128u8; 256]; // binary mean 255 vs target 128
        let bits = packed_zeros(256);
        let m = analyze(16, 16, &g, &bits, &bits, &zeros(256), &zeros(256), &bits).unwrap();
        assert!((m.tone_bias - 127.0).abs() < 1e-9);
        assert!((m.tone_mae - 127.0).abs() < 1e-9);
        let g0 = zeros(256); // all ink vs black target: exact match
        let mut ink = packed_zeros(256);
        ink.iter_mut().for_each(|b| *b = 0xFF);
        let m0 = analyze(16, 16, &g0, &ink, &ink, &zeros(256), &zeros(256), &ink).unwrap();
        assert!(m0.tone_bias.abs() < 1e-9 && m0.tone_mae.abs() < 1e-9);
        let g64 = vec![64u8; 256]; // half ink (even pixels): binary 127.5
        let mut half = packed_zeros(256);
        for i in (0..256).step_by(2) {
            set_bit(&mut half, i);
        }
        let mh = analyze(16, 16, &g64, &half, &half, &zeros(256), &zeros(256), &half).unwrap();
        assert!((mh.tone_bias - 63.5).abs() < 1e-9);
        assert!((mh.tone_mae - 63.5).abs() < 1e-9);
    }

    #[test]
    fn changed_outside_zero_inside_union_support() {
        let (w, h) = (8u32, 8u32);
        let mut cur = packed_zeros(64);
        let prev = packed_zeros(64);
        let mut cf = zeros(64);
        let mut pf = zeros(64);
        set_bit(&mut cur, 5);
        cf[5] = 1;
        set_bit(&mut cur, 9);
        pf[9] = 2;
        let m = analyze(w, h, &zeros(64), &cur, &prev, &cf, &pf, &prev).unwrap();
        assert_eq!(m.changed_total, 2);
        assert_eq!(m.changed_outside, 0);
        set_bit(&mut cur, 40); // both footprints zero: counts as outside
        let m2 = analyze(w, h, &zeros(64), &cur, &prev, &cf, &pf, &prev).unwrap();
        assert_eq!(m2.changed_total, 3);
        assert_eq!(m2.changed_outside, 1);
    }

    #[test]
    fn disocclusion_detects_wrong_cache_bit() {
        let (w, h) = (8u32, 8u32);
        let mut cur = packed_zeros(64);
        let prev = packed_zeros(64);
        let base = packed_zeros(64);
        let cf = zeros(64);
        let mut pf = zeros(64);
        // Pixel 3 uncovered; current ink vs paper base is stale.
        set_bit(&mut cur, 3);
        pf[3] = 1;
        let m = analyze(w, h, &zeros(64), &cur, &prev, &cf, &pf, &base).unwrap();
        assert_eq!(m.disocclusion_mismatches, 1);
        let mut base_ok = packed_zeros(64); // matching base bit is clean
        set_bit(&mut base_ok, 3);
        let m2 = analyze(w, h, &zeros(64), &cur, &prev, &cf, &pf, &base_ok).unwrap();
        assert_eq!(m2.disocclusion_mismatches, 0);
    }

    #[test]
    fn boundary_at_tile_edge_counts_cross_window() {
        // 32x16: left tile footprint, right tile dial. The x=15 column is a
        // boundary via its x=16 neighbor, inside the left tile's edge.
        let (w, h) = (32u32, 16u32);
        let mut gray = vec![200u8; 512];
        for y in 0..16 {
            for x in 0..16 {
                gray[y * 32 + x] = 100;
            }
        }
        let bits = packed_zeros(512); // all paper: binary mean 255
        let mut cf = zeros(512);
        for y in 0..16 {
            for x in 0..16 {
                cf[y * 32 + x] = 1;
            }
        }
        let m = analyze(w, h, &gray, &bits, &bits, &cf, &zeros(512), &bits).unwrap();
        assert!((m.boundary_mae - 155.0).abs() < 1e-9);
        assert!((m.tone_mae - 105.0).abs() < 1e-9);
        // No boundary at all documents as 0.0, not perfect tone.
        let m2 = analyze(w, h, &gray, &bits, &bits, &zeros(512), &zeros(512), &bits).unwrap();
        assert_eq!(m2.boundary_mae, 0.0);
        assert!(m2.tone_mae > 0.0);
    }

    #[test]
    fn validation_happens_before_any_index() {
        let z64 = zeros(64);
        let p64 = packed_zeros(64);
        assert!(analyze(8, 8, &zeros(4), &p64, &p64, &z64, &z64, &p64).is_err());
        assert!(analyze(8, 8, &z64, &[0u8; 2], &p64, &z64, &z64, &p64).is_err());
        assert!(analyze(0, 8, &[], &[], &[], &[], &[], &[]).is_err());
        assert!(analyze(u32::MAX, 2, &[], &[], &[], &[], &[], &[]).is_err());
        assert!(changed_overlay_rgb(8, 8, &[0u8; 2], &p64).is_err());
        assert!(changed_overlay_rgb(1300, 8, &[], &[]).is_err());
    }

    #[test]
    fn overlay_paints_only_changed_pixels() {
        let mut cur = packed_zeros(8);
        let mut prev = packed_zeros(8);
        set_bit(&mut cur, 0);
        set_bit(&mut prev, 2);
        set_bit(&mut cur, 4);
        set_bit(&mut prev, 4); // unchanged ink
        let out = changed_overlay_rgb(8, 1, &cur, &prev).unwrap();
        assert_eq!(out.len(), 24);
        assert_eq!(&out[0..3], &[176, 0, 0]); // changed, current ink
        assert_eq!(&out[3..6], &[255, 255, 255]); // unchanged paper
        assert_eq!(&out[6..9], &[255, 150, 150]); // changed, current paper
        assert_eq!(&out[9..12], &[255, 255, 255]); // unchanged paper
        assert_eq!(&out[12..15], &[0, 0, 0]); // unchanged ink
    }
}
