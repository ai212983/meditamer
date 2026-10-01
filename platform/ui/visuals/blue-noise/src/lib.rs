//! Shared full-screen blue-noise threshold maps.
//!
//! Each map is a row-major `width * height` byte array of void-and-cluster
//! thresholds generated independently per size (see `README.md` and
//! `generator/generate.cpp`). The bytes live in the binary's read-only data
//! and are only ever borrowed: [`BlueNoiseMap`] is `Copy` and carries a
//! `&'static [u8]`, never a large array by value.
//!
//! Feature flags select which maps are linked in; the default build embeds
//! nothing so a caller pays only for the resolutions it enables:
//!
//! - `map-600x600` enables [`MAP_600X600`]
//! - `map-400x300` enables [`MAP_400X300`]
//!
//! [`map_for_size`] returns the map whose dimensions match exactly, and
//! [`default_map`] returns the largest enabled map. Both return `None` when
//! no suitable map is enabled.

#![no_std]

/// A borrowed blue-noise threshold map.
///
/// `data` holds `width * height` row-major threshold bytes. A threshold
/// byte `b` at a pixel means the ordered-dither cutoff there is
/// `(b + 0.5) / 256`, matching the generator's quantization.
/// Caller-constructed maps require nonzero dimensions no larger than
/// `i32::MAX` and exactly `width * height` bytes. The bundled maps satisfy
/// these invariants.
#[derive(Clone, Copy, Debug)]
pub struct BlueNoiseMap {
    /// Map width in pixels.
    pub width: u32,
    /// Map height in pixels.
    pub height: u32,
    /// Row-major threshold bytes, `width * height` long.
    pub data: &'static [u8],
}

impl BlueNoiseMap {
    /// Ordered-dither threshold in `[0, 1)` at `(x, y)`.
    ///
    /// Coordinates wrap toroidally over the full map dimensions, so negative
    /// and out-of-range inputs (including `i32::MIN`/`i32::MAX`) are valid
    /// and never panic.
    pub fn threshold(self, x: i32, y: i32) -> f32 {
        (f32::from(self.value(x, y)) + 0.5) / 256.0
    }

    /// Raw threshold byte at `(x, y)`, with the same toroidal wrap as
    /// [`BlueNoiseMap::threshold`].
    pub fn value(self, x: i32, y: i32) -> u8 {
        let w = self.width as i32;
        let h = self.height as i32;
        let xi = x.rem_euclid(w) as u32;
        let yi = y.rem_euclid(h) as u32;
        self.data[(yi * self.width + xi) as usize]
    }
}

/// 600x600 full-screen threshold map. Only present with `map-600x600`.
#[cfg(feature = "map-600x600")]
pub static MAP_600X600: BlueNoiseMap = BlueNoiseMap {
    width: 600,
    height: 600,
    data: include_bytes!("../assets/blue-noise-600x600.bin"),
};

/// 400x300 full-screen threshold map. Only present with `map-400x300`.
#[cfg(feature = "map-400x300")]
pub static MAP_400X300: BlueNoiseMap = BlueNoiseMap {
    width: 400,
    height: 300,
    data: include_bytes!("../assets/blue-noise-400x300.bin"),
};

/// The map whose dimensions match `width` x `height` exactly, or `None`
/// when that size is not among the enabled features.
pub fn map_for_size(width: u32, height: u32) -> Option<BlueNoiseMap> {
    #[cfg(feature = "map-600x600")]
    if width == MAP_600X600.width && height == MAP_600X600.height {
        return Some(MAP_600X600);
    }
    #[cfg(feature = "map-400x300")]
    if width == MAP_400X300.width && height == MAP_400X300.height {
        return Some(MAP_400X300);
    }
    let _ = (width, height);
    None
}

/// The largest enabled map (600x600 preferred over 400x300), or `None`
/// when no map feature is enabled.
pub fn default_map() -> Option<BlueNoiseMap> {
    #[cfg(feature = "map-600x600")]
    return Some(MAP_600X600);
    #[cfg(all(not(feature = "map-600x600"), feature = "map-400x300"))]
    return Some(MAP_400X300);
    #[cfg(not(any(feature = "map-600x600", feature = "map-400x300")))]
    return None;
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    /// Enabled maps as `(map, name)` pairs; empty when no feature is on.
    // `vec![]` cannot express the feature-gated pushes, hence the lint allow.
    // `mut` is unused when no map feature is enabled (no pushes compiled in).
    #[allow(clippy::vec_init_then_push, unused_mut)]
    fn enabled() -> Vec<(BlueNoiseMap, &'static str)> {
        let mut v = Vec::new();
        #[cfg(feature = "map-600x600")]
        v.push((MAP_600X600, "600x600"));
        #[cfg(feature = "map-400x300")]
        v.push((MAP_400X300, "400x300"));
        v
    }

    #[test]
    fn data_shape_matches_dims() {
        for (m, name) in enabled() {
            assert_eq!(
                m.data.len(),
                m.width as usize * m.height as usize,
                "{name}: byte count must equal width*height"
            );
        }
    }

    #[test]
    fn coords_wrap_negative_and_extremes() {
        for (m, name) in enabled() {
            let w = m.width as i32;
            let h = m.height as i32;
            // Negative wraps to the far edge.
            assert_eq!(m.value(-1, -1), m.value(w - 1, h - 1), "{name}");
            assert_eq!(m.threshold(-1, -1), m.threshold(w - 1, h - 1), "{name}");
            // Full-period shifts are identity.
            assert_eq!(m.value(w, h), m.value(0, 0), "{name}");
            assert_eq!(m.value(-w, -h), m.value(0, 0), "{name}");
            // Extremes must not panic and must agree with their wrapped cell.
            for (x, y) in [
                (i32::MIN, i32::MIN),
                (i32::MAX, i32::MAX),
                (i32::MIN, 0),
                (0, i32::MAX),
            ] {
                let wx = x.rem_euclid(w);
                let wy = y.rem_euclid(h);
                assert_eq!(m.value(x, y), m.value(wx, wy), "{name} ({x},{y})");
                let t = m.threshold(x, y);
                assert!((0.0..1.0).contains(&t), "{name} threshold range");
            }
        }
    }

    #[test]
    fn threshold_quantization_matches_value() {
        for (m, name) in enabled() {
            for (x, y) in [(0, 0), (7, 5), (-3, 11)] {
                let expect = (f32::from(m.value(x, y)) + 0.5) / 256.0;
                assert_eq!(m.threshold(x, y), expect, "{name} ({x},{y})");
            }
        }
    }

    #[test]
    fn map_selection_exact_dims_only() {
        #[cfg(feature = "map-600x600")]
        {
            let m = map_for_size(600, 600).expect("600x600 enabled");
            assert_eq!((m.width, m.height), (600, 600));
        }
        #[cfg(feature = "map-400x300")]
        {
            let m = map_for_size(400, 300).expect("400x300 enabled");
            assert_eq!((m.width, m.height), (400, 300));
        }
        // Near misses never match: no tiling, cropping, or rescaling here.
        for (w, h) in [
            (600, 599),
            (599, 600),
            (400, 299),
            (300, 400),
            (32, 32),
            (0, 0),
        ] {
            assert!(map_for_size(w, h).is_none(), "must not match {w}x{h}");
        }
    }

    #[test]
    fn default_prefers_largest_enabled() {
        #[cfg(not(any(feature = "map-600x600", feature = "map-400x300")))]
        assert!(default_map().is_none(), "no map enabled -> None");
        #[cfg(any(feature = "map-600x600", feature = "map-400x300"))]
        {
            let d = default_map().expect("a map is enabled");
            #[cfg(feature = "map-600x600")]
            assert_eq!((d.width, d.height), (600, 600));
            #[cfg(all(not(feature = "map-600x600"), feature = "map-400x300"))]
            assert_eq!((d.width, d.height), (400, 300));
        }
    }

    #[test]
    fn histogram_balanced_within_one() {
        for (m, name) in enabled() {
            let n = m.data.len();
            let mut hist = [0u32; 256];
            for &b in m.data {
                hist[b as usize] += 1;
            }
            let (mut lo, mut hi) = (u32::MAX, 0u32);
            for &c in &hist {
                lo = lo.min(c);
                hi = hi.max(c);
            }
            assert!(hi - lo <= 1, "{name}: counts spread {lo}..{hi}");
            assert_eq!(hist.iter().sum::<u32>() as usize, n, "{name}: total");
            // Floor/ceil of n/256 exactly, so every gray level is covered.
            assert_eq!(lo, n as u32 / 256, "{name}: floor");
            assert_eq!(hi, (n as u32).div_ceil(256), "{name}: ceil");
        }
    }

    #[test]
    fn no_small_period_repeat() {
        // A tiled/repeated pattern would alias visibly; the full-grid maps
        // must not repeat exactly at 32- or 64-cell shifts.
        for (m, name) in enabled() {
            let (w, h) = (m.width as usize, m.height as usize);
            for period in [32usize, 64] {
                let mut row_same = 0usize;
                for x in 0..w {
                    if m.data[x] == m.data[period * w + x] {
                        row_same += 1;
                    }
                }
                assert!(row_same < w, "{name}: rows repeat at {period}");
                let mut col_same = 0usize;
                for y in 0..h {
                    if m.data[y * w] == m.data[y * w + period] {
                        col_same += 1;
                    }
                }
                assert!(col_same < h, "{name}: cols repeat at {period}");
            }
        }
    }

    #[test]
    fn half_level_splits_bits_evenly() {
        // At the 50% cutoff about half the bits are set: full gray ramp,
        // not a skewed distribution.
        for (m, name) in enabled() {
            let n = m.data.len();
            let on = m.data.iter().filter(|&&b| b < 128).count();
            let frac = on as f64 / n as f64;
            assert!((frac - 0.5).abs() < 0.01, "{name}: 50% cover {frac}");
        }
    }
}
