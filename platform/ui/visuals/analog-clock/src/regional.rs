//! Regional dithering over a composed frame: [`dither_regions`].
//!
//! The host composes full gray and region planes with
//! [`crate::render_region_row`], then this module quantizes each pixel
//! exactly once onto a [`raster::Surface`]. Ordered regions resolve through
//! their threshold field; diffusion regions (Floyd-Steinberg, Atkinson) run
//! serpentine over the composed gray with caller-owned scratch.
//!
//! Boundary policy (explicit): quantization error is offered only to
//! in-frame neighbours running the *same diffusion variant*. Error never
//! crosses into an ordered region (which reads no error anyway), into the
//! other diffusion variant, or off the frame — the shortfall is clipped,
//! exactly as canonical diffusion clips at image edges. When every region
//! selects the same algorithm the gate always passes, so a uniform profile
//! is bit-for-bit canonical whole-image diffusion: semantic labels alone
//! never gate error. Mixed profiles therefore show a deliberate hard method
//! boundary one pixel wide; tone next to it shifts slightly versus the
//! canonical pass, which is the price of keeping methods unmixed.
//!
//! Footprint: the caller owns the gray plane, the region plane, and the
//! `errors` scratch of [`regional_scratch_len`]`(width)` `f32`s — three
//! rows of `width + 4` cells (two guard cells per side absorb the Atkinson
//! `x ± 2` taps, three rows cover the current plus two-ahead rows). The
//! core keeps no statics and allocates nothing; scratch is cleared on every
//! call, so repeated invocations are independent.

use super::region::RegionalDitherError;
use super::region::{DitherAlgorithm, DitherRegion, RegionDithers};
use raster::Surface;

/// Caller-owned `f32` error cells for [`dither_regions`]: three rows of
/// `width + 4`, or `None` when that shape overflows `usize`.
pub fn regional_scratch_len(width: usize) -> Option<usize> {
    width.checked_add(4)?.checked_mul(3)
}

/// Dither a composed frame onto a monochrome surface: one binary decision
/// per pixel, never a re-composition. Validates every size up front (short
/// planes, short scratch, undersized surface, empty or overflowing frame)
/// and clears the scratch before use, so a failed call writes nothing and a
/// repeated call starts clean. See the module docs for the method-boundary
/// policy.
#[allow(clippy::too_many_arguments)]
pub fn dither_regions(
    width: u32,
    height: u32,
    gray: &[u8],
    regions: &[DitherRegion],
    profiles: &RegionDithers,
    errors: &mut [f32],
    surface: &mut impl Surface,
) -> Result<(), RegionalDitherError> {
    if width == 0 || height == 0 {
        return Err(RegionalDitherError::EmptyFrame);
    }
    let w = width as usize;
    let h = height as usize;
    let pixels = w.checked_mul(h).ok_or(RegionalDitherError::FrameTooLarge)?;
    if gray.len() < pixels {
        return Err(RegionalDitherError::GrayTooShort {
            need: pixels,
            got: gray.len(),
        });
    }
    if regions.len() < pixels {
        return Err(RegionalDitherError::RegionsTooShort {
            need: pixels,
            got: regions.len(),
        });
    }
    let need = regional_scratch_len(w).ok_or(RegionalDitherError::FrameTooLarge)?;
    if errors.len() < need {
        return Err(RegionalDitherError::ScratchTooShort {
            need,
            got: errors.len(),
        });
    }
    if i64::from(surface.width()) < i64::from(width)
        || i64::from(surface.height()) < i64::from(height)
    {
        return Err(RegionalDitherError::SurfaceTooSmall);
    }
    errors[..need].fill(0.0);

    // The frame's blue-noise map, selected once per frame: exact-size match
    // else the largest enabled map (cropped/wrapped for experimental sizes,
    // never rescaled). Every BlueNoise pixel in this frame shares it.
    let blue = super::region::blue_map_for_frame(width, height);
    let uniform = profiles.is_uniform();
    // One shared row worker (see `streaming::dither_one_row`): the streaming
    // path runs these same rows one per poll, so whole-frame and streamed
    // output agree bit for bit by construction.
    for y in 0..height {
        let row = y as usize;
        super::streaming::dither_one_row(
            super::streaming::RowView {
                width,
                height,
                y,
                gray: &gray[row * w..row * w + w],
            },
            |x| {
                if uniform {
                    profiles.background
                } else {
                    profiles.for_region(regions[row * w + x as usize])
                }
            },
            |nx, ny| {
                if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                    return None;
                }
                if uniform {
                    return Some(profiles.background);
                }
                Some(profiles.for_region(regions[ny as usize * w + nx as usize]))
            },
            uniform,
            blue,
            &mut errors[..need],
            surface,
        );
    }
    Ok(())
}

/// Dither one gray plane with a single algorithm, whole frame to
/// completion, ignoring semantic labels: the composition experiment builds
/// its stationary candidates (unoccluded dial, keep-probability mask) and
/// its reference frame this way. Ordered algorithms resolve through the
/// same per-frame blue map and the same threshold fields as
/// [`dither_regions`]; diffusion runs the same serpentine gate in
/// always-uniform mode, so a uniform [`dither_regions`] profile and this
/// function agree bit for bit. Same validation-before-mutation contract:
/// short gray/scratch, undersized surface, and empty or overflowing frames
/// fail before anything is written, and the scratch is cleared on entry.
pub fn dither_flat(
    width: u32,
    height: u32,
    gray: &[u8],
    algo: DitherAlgorithm,
    errors: &mut [f32],
    surface: &mut impl Surface,
) -> Result<(), RegionalDitherError> {
    if width == 0 || height == 0 {
        return Err(RegionalDitherError::EmptyFrame);
    }
    let w = width as usize;
    let h = height as usize;
    let pixels = w.checked_mul(h).ok_or(RegionalDitherError::FrameTooLarge)?;
    if gray.len() < pixels {
        return Err(RegionalDitherError::GrayTooShort {
            need: pixels,
            got: gray.len(),
        });
    }
    let need = regional_scratch_len(w).ok_or(RegionalDitherError::FrameTooLarge)?;
    if errors.len() < need {
        return Err(RegionalDitherError::ScratchTooShort {
            need,
            got: errors.len(),
        });
    }
    if i64::from(surface.width()) < i64::from(width)
        || i64::from(surface.height()) < i64::from(height)
    {
        return Err(RegionalDitherError::SurfaceTooSmall);
    }
    errors[..need].fill(0.0);

    // The frame's blue-noise map, selected once per frame exactly like
    // `dither_regions` does — never a private second resolution.
    let blue = super::region::blue_map_for_frame(width, height);
    for y in 0..height {
        let row = y as usize;
        super::streaming::dither_one_row(
            super::streaming::RowView {
                width,
                height,
                y,
                gray: &gray[row * w..row * w + w],
            },
            |_| algo,
            |nx, ny| {
                if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                    None
                } else {
                    Some(algo)
                }
            },
            true,
            blue,
            &mut errors[..need],
            surface,
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::streaming::diffuse_ink;

    #[test]
    fn scratch_shape_is_three_guarded_rows() {
        assert_eq!(regional_scratch_len(0), Some(12));
        assert_eq!(regional_scratch_len(16), Some(60));
        assert_eq!(regional_scratch_len(usize::MAX), None);
        assert_eq!(regional_scratch_len(usize::MAX - 3), None);
    }

    #[test]
    fn mid_gray_quantizes_to_ink_boundary() {
        assert!(diffuse_ink(0.499));
        assert!(!diffuse_ink(0.5));
    }
}
