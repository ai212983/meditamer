//! The canvas contract shared by `enso`, `flipclock` and `hourglass`.
//!
//! Each of those is a pure, deterministic model that turns elapsed time into
//! set pixels. What they share is a one-bit destination and the dither that
//! resolves continuous coverage onto it -- and, deliberately, nothing else.
//! `flipclock` and `hourglass` are entirely integer and carry their own
//! fixed-point tables, so the seeded PRNG and float helpers that only `enso`
//! needs live in `enso` rather than here.
//!
//! In particular there is no trait spanning the three. A stepped cellular
//! automaton must be advanced in order and carries state; a card flip is a
//! function of elapsed time within one transition; a brush stroke is a function
//! of progress that only ever adds ink. Those are three different temporal
//! contracts, and a single trait over them would describe all three worse than
//! the three separate APIs do. What they genuinely have in common is where the
//! pixels go, so that is what lives here.

#![no_std]

/// Ordered dithering, and the `libm` it needs. Gated behind the `dither`
/// feature so the integer-only `flipclock` and `hourglass` canvases -- which
/// never dither -- build without pulling in float code; `enso` enables the
/// feature explicitly.
#[cfg(feature = "dither")]
pub mod dither;

#[cfg(feature = "dither")]
pub use dither::Dither;

/// A one-bit pixel destination.
///
/// Coordinates use a top-left origin in the model's own pixel space: `x`
/// increases rightward and `y` downward. Implementations are expected to
/// ignore out-of-bounds writes rather than panic: a caller may legitimately
/// sample a brush rib or a rotated cell that overhangs the canvas, and
/// clipping is the surface's job.
///
/// Generic rather than a trait object everywhere it is consumed, so the
/// per-pixel call monomorphizes away in what is, for all three callers, the
/// inner loop.
pub trait Surface {
    fn width(&self) -> i32;
    fn height(&self) -> i32;

    /// Set one pixel. `ink` is true for the marked state -- black on this
    /// hardware, since the panel's framebuffer is ink-on-paper rather than
    /// luminance.
    fn set(&mut self, x: i32, y: i32, ink: bool);
}

/// A row-major, one-bit-per-pixel canvas over a borrowed buffer, MSB first
/// within each byte.
///
/// This is not any panel's memory format -- the Inkplate framebuffer is
/// column-major and bottom-up, and a board adapts to that itself. What this is
/// for is tests and host previews, which need *a* surface and should not each
/// invent one.
pub struct BitCanvas<'a> {
    width: i32,
    height: i32,
    bits: &'a mut [u8],
}

impl<'a> BitCanvas<'a> {
    /// Bytes needed to back a canvas of this size.
    pub const fn bytes_for(width: i32, height: i32) -> usize {
        let pixels = (width as usize) * (height as usize);
        pixels.div_ceil(8)
    }

    /// Wraps a buffer. Returns `None` when the dimensions are not positive or
    /// the buffer is too small to hold them.
    pub fn new(width: i32, height: i32, bits: &'a mut [u8]) -> Option<Self> {
        if width <= 0 || height <= 0 || bits.len() < Self::bytes_for(width, height) {
            return None;
        }
        Some(Self {
            width,
            height,
            bits,
        })
    }

    fn index(&self, x: i32, y: i32) -> Option<(usize, u8)> {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return None;
        }
        let bit = (y as usize) * (self.width as usize) + (x as usize);
        Some((bit / 8, 0x80 >> (bit % 8)))
    }

    pub fn get(&self, x: i32, y: i32) -> bool {
        self.index(x, y)
            .is_some_and(|(byte, mask)| self.bits[byte] & mask != 0)
    }

    pub fn clear(&mut self) {
        self.bits.fill(0);
    }

    /// How many pixels are inked. Useful as a cheap fingerprint in tests.
    pub fn ink_count(&self) -> u32 {
        // Counts whole bytes, so a buffer longer than the canvas would
        // over-report; `new` bounds the slice but does not truncate it, so
        // count only the bytes this canvas actually addresses.
        let used = Self::bytes_for(self.width, self.height);
        self.bits[..used].iter().map(|byte| byte.count_ones()).sum()
    }
}

impl Surface for BitCanvas<'_> {
    fn width(&self) -> i32 {
        self.width
    }

    fn height(&self) -> i32 {
        self.height
    }

    fn set(&mut self, x: i32, y: i32, ink: bool) {
        if let Some((byte, mask)) = self.index(x, y) {
            if ink {
                self.bits[byte] |= mask;
            } else {
                self.bits[byte] &= !mask;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_for_rounds_up_to_a_whole_byte() {
        assert_eq!(BitCanvas::bytes_for(8, 1), 1);
        assert_eq!(BitCanvas::bytes_for(9, 1), 2);
        assert_eq!(BitCanvas::bytes_for(600, 600), 45_000);
    }

    #[test]
    fn rejects_dimensions_it_cannot_back() {
        let mut bits = [0u8; 4];
        assert!(BitCanvas::new(0, 8, &mut bits).is_none());
        assert!(BitCanvas::new(8, -1, &mut bits).is_none());
        assert!(BitCanvas::new(64, 64, &mut bits).is_none());
        assert!(BitCanvas::new(8, 4, &mut bits).is_some());
    }

    #[test]
    fn set_and_get_round_trip() {
        let mut bits = [0u8; BitCanvas::bytes_for(16, 16)];
        let mut canvas = BitCanvas::new(16, 16, &mut bits).expect("canvas");
        canvas.set(3, 5, true);
        canvas.set(15, 15, true);
        assert!(canvas.get(3, 5));
        assert!(canvas.get(15, 15));
        assert!(!canvas.get(4, 5));
        assert_eq!(canvas.ink_count(), 2);
        canvas.set(3, 5, false);
        assert!(!canvas.get(3, 5));
        assert_eq!(canvas.ink_count(), 1);
    }

    /// Clipping is the surface's contract, so a caller can draw past the edge
    /// without checking first.
    #[test]
    fn out_of_bounds_writes_are_dropped_not_wrapped() {
        let mut bits = [0u8; BitCanvas::bytes_for(8, 8)];
        let mut canvas = BitCanvas::new(8, 8, &mut bits).expect("canvas");
        canvas.set(-1, 0, true);
        canvas.set(0, -1, true);
        canvas.set(8, 0, true);
        canvas.set(0, 8, true);
        assert_eq!(canvas.ink_count(), 0);
        assert!(!canvas.get(-1, 0));
        assert!(!canvas.get(8, 0));
    }

    #[test]
    fn clear_empties_the_canvas() {
        let mut bits = [0u8; BitCanvas::bytes_for(16, 16)];
        let mut canvas = BitCanvas::new(16, 16, &mut bits).expect("canvas");
        for x in 0..16 {
            canvas.set(x, x, true);
        }
        assert_eq!(canvas.ink_count(), 16);
        canvas.clear();
        assert_eq!(canvas.ink_count(), 0);
    }
}
