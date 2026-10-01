//! Borrowed hand-sprite maps. The host tool decodes PNGs and owns the
//! buffers; this crate only borrows them, so firmware adds no statics and
//! no heap for assets.
//!
//! Expected source files (unchanged on disk):
//!
//! - `assets/hour_hand_d.png` / `assets/minute_hand_d.png`: diffuse RGBA,
//!   245x810 and 156x1014. Alpha is the silhouette authority: it masks all
//!   three maps, so normal/specular background values never contribute.
//! - `assets/hour_hand_n.png` / `assets/minute_hand_n.png`: tangent-space
//!   normals, RGB, no alpha. Stored green points **up** (OpenGL convention):
//!   the disk's top rim reads G > 128 while its bottom rim reads G < 128,
//!   so [`decode_normal`] flips green into the renderer's y-down space.
//! - `assets/hour_hand_s.png` / `assets/minute_hand_s.png`: specular
//!   strength, single gray channel, read as data (no gamma).
//!
//! Canonical sprites point up. Pivots are the solid-disk centers measured
//! from the diffuse alpha. The hollow rings are decorative cutouts:
//!
//! - hour disk center: pivot (121.5, 687.0)
//! - minute disk center: pivot (77.5, 935.5)
//!
//! A scene may override either pivot; the defaults below are the measured
//! values.

/// Measured hour-hand pivot in sprite pixels: (121.5, 687.0) on 245x810.
pub const HOUR_PIVOT: (f32, f32) = (121.5, 687.0);
/// Measured minute-hand pivot in sprite pixels: (77.5, 935.5) on 156x1014.
pub const MINUTE_PIVOT: (f32, f32) = (77.5, 935.5);

/// One hand's decoded maps, borrowed. Widths and heights are sprite pixels;
/// map slices are tightly packed rows (normal RGB; alpha/spec one byte per
/// pixel). The pivot is in sprite pixels from the top-left.
///
/// Albedo layout: `albedo` holds either tightly packed RGB (`pixels * 3`
/// bytes, the canonical export) or tightly packed lossless grayscale
/// (`pixels` bytes, one sRGB gray byte per pixel). The grayscale form is a
/// pure memory compacting: the canonical source has all three RGB channels
/// identical at every texel (even outside alpha), so sampling gray once and
/// replicating it feeds the pipeline the exact floats the RGB path would
/// produce. Alpha, specular, and normal maps stay full-resolution and
/// unchanged. The host exporter verifies channel identity before compacting.
pub struct HandMaps<'a> {
    pub width: u16,
    pub height: u16,
    pub albedo: &'a [u8],
    pub alpha: &'a [u8],
    pub normal: &'a [u8],
    pub spec: &'a [u8],
    pub pivot_x: f32,
    pub pivot_y: f32,
}

impl HandMaps<'_> {
    /// Whether the albedo slice is the compact grayscale form (one byte per
    /// pixel) rather than packed RGB. Only meaningful when [`validate`]
    /// passes; a malformed slice of another length is neither.
    pub fn albedo_is_gray(&self) -> bool {
        let w = self.width as usize;
        let h = self.height as usize;
        match w.checked_mul(h) {
            Some(pixels) => self.albedo.len() == pixels,
            None => false,
        }
    }

    /// Checks slice lengths against the geometry and that the pivot is a
    /// finite point inside the sprite: x in `[0, width)`, y in `(0, height)`.
    /// The upper bound is exclusive (an edge texel center is the last valid
    /// point) and y must be positive because the scale factor is
    /// `pivot_y / len`. Albedo accepts `pixels * 3` (RGB) or `pixels` (gray);
    /// any other length is rejected. Out-of-range sprite reads still clip to
    /// transparent; this only rejects mis-sized buffers up front.
    pub fn validate(&self) -> bool {
        let w = self.width as usize;
        let h = self.height as usize;
        if w == 0 || h == 0 {
            return false;
        }
        let px = w.checked_mul(h);
        let Some(pixels) = px else { return false };
        (self.albedo.len() == pixels * 3 || self.albedo.len() == pixels)
            && self.alpha.len() == pixels
            && self.normal.len() == pixels * 3
            && self.spec.len() == pixels
            && self.pivot_x.is_finite()
            && self.pivot_y.is_finite()
            && self.pivot_x >= 0.0
            && self.pivot_y > 0.0
            && self.pivot_x < self.width as f32
            && self.pivot_y < self.height as f32
    }
}

/// Both hands' borrowed maps. Minute renders above hour.
pub struct Hands<'a> {
    pub hour: HandMaps<'a>,
    pub minute: HandMaps<'a>,
}

/// Borrowed grayscale dial artwork (one sRGB gray byte per pixel, row
/// major). The host owns the buffer; the core only borrows it, so
/// firmware adds no statics and no heap for the dial either.
///
/// When a scene carries a valid map, the baked artwork replaces the
/// procedural dial entirely (paper, ticks, and hub are all suppressed);
/// every baked mark counts as [`crate::DitherRegion::Background`]. An
/// invalid map (wrong length, empty, non-square) falls back to the
/// procedural dial instead of panicking.
#[derive(Clone, Copy, Debug)]
pub struct DialMap<'a> {
    pub width: u32,
    pub height: u32,
    pub pixels: &'a [u8],
}

impl DialMap<'_> {
    /// Checks the slice against the geometry: nonzero square artwork with
    /// exactly one gray byte per pixel.
    pub fn validate(&self) -> bool {
        if self.width == 0 || self.height == 0 || self.width != self.height {
            return false;
        }
        (self.width as usize)
            .checked_mul(self.height as usize)
            .is_some_and(|n| self.pixels.len() == n)
    }
}

impl Hands<'_> {
    pub fn validate(&self) -> bool {
        self.hour.validate() && self.minute.validate()
    }
}

/// Decodes one stored normal texel into y-down renderer space with unit
/// length (up to quantization). Green flips: the maps store green-up, the
/// renderer works y-down, so G above 128 means the surface tips toward the
/// top of the sprite (negative y), not the bottom.
pub fn decode_normal(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let nx = (f32::from(r) - 127.5) / 127.5;
    let ny = (127.5 - f32::from(g)) / 127.5;
    let nz = (f32::from(b) - 127.5) / 127.5;
    let len = libm::sqrtf(nx * nx + ny * ny + nz * nz);
    if len > 1e-6 {
        (nx / len, ny / len, nz / len)
    } else {
        (0.0, 0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    fn maps(w: u16, h: u16) -> (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>) {
        let n = w as usize * h as usize;
        (
            std::vec![0u8; n * 3],
            std::vec![0u8; n],
            std::vec![0u8; n * 3],
            std::vec![0u8; n],
        )
    }

    #[test]
    fn validate_accepts_matching_buffers() {
        let (albedo, alpha, normal, spec) = maps(4, 5);
        let hand = HandMaps {
            width: 4,
            height: 5,
            albedo: &albedo,
            alpha: &alpha,
            normal: &normal,
            spec: &spec,
            pivot_x: 2.0,
            pivot_y: 3.0,
        };
        assert!(hand.validate());
    }

    #[test]
    fn validate_rejects_mismatched_or_bad_geometry() {
        let (albedo, alpha, normal, spec) = maps(4, 5);
        let short: Vec<u8> = std::vec![0u8; 10];
        let base = || HandMaps {
            width: 4,
            height: 5,
            albedo: &albedo,
            alpha: &alpha,
            normal: &normal,
            spec: &spec,
            pivot_x: 2.0,
            pivot_y: 3.0,
        };
        assert!(!HandMaps {
            alpha: &short,
            ..base()
        }
        .validate());
        assert!(!HandMaps { width: 0, ..base() }.validate());
        assert!(!HandMaps {
            pivot_x: 5.0,
            ..base()
        }
        .validate());
        // Exclusive upper bound: the edge itself is outside.
        assert!(!HandMaps {
            pivot_x: 4.0,
            ..base()
        }
        .validate());
        let square = std::vec![0u8; 16];
        assert!(DialMap {
            width: 4,
            height: 4,
            pixels: &square
        }
        .validate());
        assert!(!DialMap {
            width: 4,
            height: 5,
            pixels: &square
        }
        .validate());
        assert!(!DialMap {
            width: 4,
            height: 4,
            pixels: &short
        }
        .validate());
        assert!(!DialMap {
            width: 0,
            height: 0,
            pixels: &[]
        }
        .validate());
        assert!(!HandMaps {
            pivot_y: 5.0,
            ..base()
        }
        .validate());
        // Zero-length pivot row would zero the hand scale.
        assert!(!HandMaps {
            pivot_y: 0.0,
            ..base()
        }
        .validate());
        assert!(!HandMaps {
            pivot_y: f32::NAN,
            ..base()
        }
        .validate());
    }

    #[test]
    fn flat_normal_decodes_to_straight_on() {
        let (nx, ny, nz) = decode_normal(127, 127, 255);
        assert!(nx.abs() < 0.02 && ny.abs() < 0.02 && nz > 0.99);
    }

    /// Green-up convention: the disk's top rim (G high) tips toward the top
    /// of the sprite, i.e. negative y in renderer space.
    #[test]
    fn stored_green_high_means_surface_tips_up() {
        let (_, top_ny, _) = decode_normal(127, 154, 251);
        let (_, bottom_ny, _) = decode_normal(127, 73, 243);
        assert!(top_ny < -0.05, "top rim should tip up, got {top_ny}");
        assert!(bottom_ny > 0.05, "bottom rim should tip down");
    }

    #[test]
    fn red_encodes_x_without_sign_flip() {
        let (left_nx, _, _) = decode_normal(37, 127, 218);
        let (right_nx, _, _) = decode_normal(217, 127, 218);
        assert!(left_nx < -0.3 && right_nx > 0.3);
    }
}
