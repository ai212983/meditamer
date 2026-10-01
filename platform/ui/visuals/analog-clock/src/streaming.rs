//! Row-streaming regional dither for bounded device memory.
//!
//! [`StreamingDither`] quantizes one composed row per call onto a caller
//! packed [`Surface`], so the device never holds the full gray, region, or
//! footprint planes: per poll it owns three gray rows, three label rows, and
//! three float error rows, all caller-allocated (externally, e.g. PSRAM
//! boxed typed rows). The whole-frame [`crate::dither_regions`] runs this
//! same row worker, so streamed and whole-frame output agree bit for bit.
//!
//! Borrowed/owned buffer contract (for the concurrent UI worker):
//!
//! - The state ([`StreamingDither`]) is pointer-only small state: `width`,
//!   `height`, `next_y`, profiles, the per-frame blue map, and a started
//!   flag. It borrows nothing and allocates nothing (no statics).
//! - The error scratch (`&mut [f32]`, [`crate::regional_scratch_len`]
//!   cells) is caller-owned and passed every row; [`begin_frame`] clears it.
//!   [`StreamingDither::begin_frame`] shows the order.
//! - Gray and label window rows are caller-owned borrows per call: the
//!   current row plus the next two label rows (Atkinson reaches two rows
//!   ahead). The gray row is only needed for the current `y`; future gray
//!   rows are never read.
//! - The surface is caller-owned; a failed call writes nothing (no pixels,
//!   no error cells, no `next_y` advance).
//! - Rows must arrive in order from `0`; after the last row [`finish`]
//!   closes the frame and [`reset`] (or a new [`begin_frame`]) re-arms.
//!
//! [`begin_frame`]: StreamingDither::begin_frame
//! [`finish`]: StreamingDither::finish
//! [`reset`]: StreamingDither::reset
//! [`Surface`]: raster::Surface

use super::region::{ordered_threshold, DitherAlgorithm, DitherRegion, RegionDithers};
use super::regional::regional_scratch_len;
use blue_noise::BlueNoiseMap;
use raster::Surface;

/// What can go wrong before a streamed row is dithered. All variants are
/// caller-fixable geometry, sequencing, or buffer sizes; a failed row call
/// writes nothing and leaves the state (and scratch) untouched.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StreamError {
    /// `width` or `height` is zero: there is no frame to dither.
    EmptyFrame,
    /// `width * height` (or the scratch shape) overflows `usize`.
    FrameTooLarge,
    /// [`StreamingDither::process_row`] ran before [`begin_frame`], or after
    /// [`finish`]/[`reset`] without a new frame.
    ///
    /// [`begin_frame`]: StreamingDither::begin_frame
    /// [`finish`]: StreamingDither::finish
    /// [`reset`]: StreamingDither::reset
    NotStarted,
    /// Rows must arrive in order from `0`: `got` arrived while `expected`
    /// was due.
    OutOfSequence { expected: u32, got: u32 },
    /// The gray row is short for `width`.
    GrayTooShort { need: usize, got: usize },
    /// A needed label row is short for `width`.
    RegionsTooShort { need: usize, got: usize },
    /// The caller scratch is short for
    /// [`crate::regional_scratch_len`]`(width)`.
    ScratchTooShort { need: usize, got: usize },
    /// The surface is smaller than `width` x `height`.
    SurfaceTooSmall,
    /// The frame is complete (`next_y == height`); close it with [`finish`].
    ///
    /// [`finish`]: StreamingDither::finish
    FrameDone,
}

impl core::fmt::Display for StreamError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            StreamError::EmptyFrame => write!(f, "zero-size frame"),
            StreamError::FrameTooLarge => write!(f, "frame overflows usize"),
            StreamError::NotStarted => write!(f, "frame not started"),
            StreamError::OutOfSequence { expected, got } => {
                write!(f, "row {got} arrived while row {expected} was due")
            }
            StreamError::GrayTooShort { need, got } => {
                write!(f, "gray row holds {got}, needs {need}")
            }
            StreamError::RegionsTooShort { need, got } => {
                write!(f, "label row holds {got}, needs {need}")
            }
            StreamError::ScratchTooShort { need, got } => {
                write!(f, "error scratch holds {got}, needs {need}")
            }
            StreamError::SurfaceTooSmall => write!(f, "surface smaller than frame"),
            StreamError::FrameDone => write!(f, "frame already complete"),
        }
    }
}

/// Quantize one pixel to ink (`true`) or paper: dark gray inks, matching the
/// ordered rule `coverage > threshold` at exactly mid-gray. Same rule the
/// whole-frame path uses; kept adjacent to the shared worker so the two can
/// never drift.
pub(crate) fn diffuse_ink(old: f32) -> bool {
    old < 0.5
}

/// The gated error plane over caller-owned scratch: serpentine diffusion
/// with method-boundary clipping. Neighbour methods come from a caller
/// lookup so whole-frame and streaming shares one worker.
struct Gate<'a, F>
where
    F: Fn(i32, i32) -> Option<DitherAlgorithm>,
{
    errors: &'a mut [f32],
    width: u32,
    height: u32,
    method_at: F,
    uniform: bool,
}

impl<F> Gate<'_, F>
where
    F: Fn(i32, i32) -> Option<DitherAlgorithm>,
{
    /// Read and clear one current-row error cell.
    fn take(&mut self, row_base: usize, x: u32) -> f32 {
        let slot = row_base + x as usize + 2;
        let value = self.errors[slot];
        self.errors[slot] = 0.0;
        value
    }

    /// Offer error to a neighbour, identified by screen coordinate plus the
    /// buffer row its error lives on. Drops the share when the neighbour is
    /// off-frame (`None` lookup) or runs a different method (unless the
    /// profile is uniform, in which case every in-frame neighbour
    /// qualifies).
    fn push(&mut self, kind: DitherAlgorithm, x: i32, y: i32, row_base: usize, share: f32) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        if !self.uniform && (self.method_at)(x, y) != Some(kind) {
            return;
        }
        self.errors[row_base + x as usize + 2] += share;
    }
}

/// Error-row offsets for one diffusion step: the current row plus the two
/// rows below. Same-row taps ride the current row buffer (which the
/// serpentine order has already positioned downstream); `nxt`/`nxt2` are
/// the rows below. Grouped so the fan-out worker takes one row argument.
#[derive(Clone, Copy, Debug)]
struct DiffusionRows {
    cur: usize,
    nxt: usize,
    nxt2: usize,
}

/// Fan one pixel's quantization error out to its unfinished neighbours.
fn spread<F>(
    gate: &mut Gate<'_, F>,
    method: DitherAlgorithm,
    x: u32,
    y: u32,
    rows: DiffusionRows,
    rtl: bool,
    err: f32,
) where
    F: Fn(i32, i32) -> Option<DitherAlgorithm>,
{
    // Mirror the kernel when running right to left.
    let s = if rtl { -1 } else { 1 };
    let xi = x as i32;
    let yi = y as i32;
    if method == DitherAlgorithm::FloydSteinberg {
        gate.push(method, xi + s, yi, rows.cur, err * (7.0 / 16.0));
        gate.push(method, xi - s, yi + 1, rows.nxt, err * (3.0 / 16.0));
        gate.push(method, xi, yi + 1, rows.nxt, err * (5.0 / 16.0));
        gate.push(method, xi + s, yi + 1, rows.nxt, err * (1.0 / 16.0));
    } else {
        let share = err * (1.0 / 8.0);
        gate.push(method, xi + s, yi, rows.cur, share);
        gate.push(method, xi + 2 * s, yi, rows.cur, share);
        gate.push(method, xi - s, yi + 1, rows.nxt, share);
        gate.push(method, xi, yi + 1, rows.nxt, share);
        gate.push(method, xi + s, yi + 1, rows.nxt, share);
        gate.push(method, xi, yi + 2, rows.nxt2, share);
    }
}

/// Geometry plus the current gray row for one dithered row: the frame
/// dimensions, the row index, and the caller's gray bytes for that row.
/// Grouped so the shared row worker takes one geometry argument.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RowView<'a> {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) y: u32,
    pub(crate) gray: &'a [u8],
}

/// Dither one composed row onto the surface: the single worker behind both
/// the whole-frame functions and [`StreamingDither`]. `view` carries the
/// frame geometry, the row index, and the current gray row; `method_of`
/// names the algorithm in force at each current-row column; `method_at`
/// names the algorithm at any in-frame coordinate (`None` only for
/// off-frame, which drops the share). Ordered methods resolve through the
/// frame's `blue` map; diffusion runs serpentine with the same parity as
/// the whole-frame pass (odd rows right to left).
pub(crate) fn dither_one_row(
    view: RowView<'_>,
    method_of: impl Fn(u32) -> DitherAlgorithm,
    method_at: impl Fn(i32, i32) -> Option<DitherAlgorithm>,
    uniform: bool,
    blue: Option<BlueNoiseMap>,
    errors: &mut [f32],
    surface: &mut impl Surface,
) {
    let w = view.width as usize;
    let stride = w + 4;
    let row = view.y as usize;
    let rows = DiffusionRows {
        cur: (row % 3) * stride,
        nxt: ((row + 1) % 3) * stride,
        nxt2: ((row + 2) % 3) * stride,
    };
    // Serpentine: odd rows run right to left so error always flows from
    // finished pixels into unfinished ones — same parity as whole-frame.
    let rtl = view.y % 2 == 1;
    let mut gate = Gate {
        errors,
        width: view.width,
        height: view.height,
        method_at,
        uniform,
    };
    for i in 0..view.width {
        let x = if rtl { view.width - 1 - i } else { i };
        let method = method_of(x);
        if let Some(threshold) = ordered_threshold(method, x as i32, view.y as i32, blue) {
            let coverage = 1.0 - f32::from(view.gray[x as usize]) * (1.0 / 255.0);
            surface.set(x as i32, view.y as i32, coverage > threshold);
        } else {
            let old = f32::from(view.gray[x as usize]) * (1.0 / 255.0) + gate.take(rows.cur, x);
            let ink = diffuse_ink(old);
            surface.set(x as i32, view.y as i32, ink);
            let err = old - if ink { 0.0 } else { 1.0 };
            if err != 0.0 {
                spread(&mut gate, method, x, view.y, rows, rtl, err);
            }
        }
    }
}

/// Bounded-memory row-streaming dither state. Pointer-only small state (two
/// dimensions, a row cursor, profiles, the per-frame blue map, a started
/// flag): no borrows, no allocation, no statics. The caller owns every
/// buffer — gray/label window rows, float error scratch, and the packed
/// surface — and may keep them in external PSRAM as owned boxed typed rows.
#[derive(Clone, Copy, Debug)]
pub struct StreamingDither {
    width: u32,
    height: u32,
    profiles: RegionDithers,
    blue: Option<BlueNoiseMap>,
    next_y: u32,
    started: bool,
}

impl StreamingDither {
    /// Open a stream for one `width` x `height` frame under `profiles`.
    /// Validates the geometry up front; the blue map resolves at
    /// [`begin_frame`], once per frame like the whole-frame path.
    ///
    /// [`begin_frame`]: StreamingDither::begin_frame
    pub fn new(width: u32, height: u32, profiles: RegionDithers) -> Result<Self, StreamError> {
        if width == 0 || height == 0 {
            return Err(StreamError::EmptyFrame);
        }
        let w = width as usize;
        let h = height as usize;
        w.checked_mul(h).ok_or(StreamError::FrameTooLarge)?;
        regional_scratch_len(w).ok_or(StreamError::FrameTooLarge)?;
        Ok(Self {
            width,
            height,
            profiles,
            blue: None,
            next_y: 0,
            started: false,
        })
    }

    /// Arm (or re-arm) the stream for a new frame: validates and clears the
    /// caller scratch, resolves the frame's blue map once, and resets the
    /// row cursor to `0`.
    pub fn begin_frame(&mut self, errors: &mut [f32]) -> Result<(), StreamError> {
        let need = regional_scratch_len(self.width as usize).ok_or(StreamError::FrameTooLarge)?;
        if errors.len() < need {
            return Err(StreamError::ScratchTooShort {
                need,
                got: errors.len(),
            });
        }
        errors[..need].fill(0.0);
        self.blue = super::region::blue_map_for_frame(self.width, self.height);
        self.next_y = 0;
        self.started = true;
        Ok(())
    }

    /// The next row due (`0` once a frame is armed).
    pub fn next_y(&self) -> u32 {
        self.next_y
    }

    /// Whether every row has been accepted (the frame still needs
    /// [`finish`] to close).
    ///
    /// [`finish`]: StreamingDither::finish
    pub fn is_done(&self) -> bool {
        self.started && self.next_y == self.height
    }

    /// Dither the current row `y` onto the surface. The caller supplies the
    /// current gray row, the current label row, and the next two label rows
    /// (Atkinson diffuses two rows ahead); future gray rows are never read.
    /// At the frame tail rows past the bottom need no labels: pass empty
    /// slices for `next`/`next2` when `y + 1`/`y + 2` reach past `height`.
    /// Under a uniform profile future labels are never consulted and may be
    /// empty on every row. Validates everything before touching the surface,
    /// scratch, or cursor, so a failed call writes nothing.
    #[allow(clippy::too_many_arguments)]
    pub fn process_row(
        &mut self,
        y: u32,
        gray_row: &[u8],
        current: &[DitherRegion],
        next: &[DitherRegion],
        next2: &[DitherRegion],
        errors: &mut [f32],
        surface: &mut impl Surface,
    ) -> Result<(), StreamError> {
        if !self.started {
            return Err(StreamError::NotStarted);
        }
        if self.next_y == self.height {
            return Err(StreamError::FrameDone);
        }
        if y != self.next_y {
            return Err(StreamError::OutOfSequence {
                expected: self.next_y,
                got: y,
            });
        }
        let w = self.width as usize;
        if gray_row.len() < w {
            return Err(StreamError::GrayTooShort {
                need: w,
                got: gray_row.len(),
            });
        }
        if current.len() < w {
            return Err(StreamError::RegionsTooShort {
                need: w,
                got: current.len(),
            });
        }
        let uniform = self.profiles.is_uniform();
        // Future labels back the two-ahead diffusion taps: require them only
        // while they name in-frame rows of a method-gated (mixed) profile.
        // Uniform passes never gate, tail rows have no future rows.
        if !uniform && y + 1 < self.height && next.len() < w {
            return Err(StreamError::RegionsTooShort {
                need: w,
                got: next.len(),
            });
        }
        if !uniform && y + 2 < self.height && next2.len() < w {
            return Err(StreamError::RegionsTooShort {
                need: w,
                got: next2.len(),
            });
        }
        let need = regional_scratch_len(w).ok_or(StreamError::FrameTooLarge)?;
        if errors.len() < need {
            return Err(StreamError::ScratchTooShort {
                need,
                got: errors.len(),
            });
        }
        if i64::from(surface.width()) < i64::from(self.width)
            || i64::from(surface.height()) < i64::from(self.height)
        {
            return Err(StreamError::SurfaceTooSmall);
        }
        let profiles = self.profiles;
        let blue = self.blue;
        let width = self.width;
        let height = self.height;
        dither_one_row(
            RowView {
                width,
                height: self.height,
                y,
                gray: &gray_row[..w],
            },
            |x| {
                if uniform {
                    profiles.background
                } else {
                    profiles.for_region(current[x as usize])
                }
            },
            |nx, ny| {
                if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                    return None;
                }
                if uniform {
                    return Some(profiles.background);
                }
                let ny = ny as u32;
                let nx = nx as usize;
                if ny == y {
                    Some(profiles.for_region(current[nx]))
                } else if ny == y + 1 {
                    Some(profiles.for_region(next[nx]))
                } else if ny == y + 2 {
                    Some(profiles.for_region(next2[nx]))
                } else {
                    // Diffusion never reaches further than two rows ahead;
                    // anything beyond is unreachable, reported as off-frame.
                    None
                }
            },
            uniform,
            blue,
            &mut errors[..need],
            surface,
        );
        self.next_y += 1;
        Ok(())
    }

    /// Close a complete frame. Succeeds only when every row was accepted;
    /// clears the armed flag so a further row needs a new [`begin_frame`].
    /// A partial frame reports [`StreamError::OutOfSequence`] naming the row
    /// still due.
    ///
    /// [`begin_frame`]: StreamingDither::begin_frame
    pub fn finish(&mut self) -> Result<(), StreamError> {
        if !self.started {
            return Err(StreamError::NotStarted);
        }
        if self.next_y != self.height {
            return Err(StreamError::OutOfSequence {
                expected: self.next_y,
                got: self.height,
            });
        }
        self.started = false;
        Ok(())
    }

    /// Drop the armed flag and rewind the cursor without touching caller
    /// buffers. The next row needs a new [`begin_frame`] (which re-clears
    /// the scratch).
    ///
    /// [`begin_frame`]: StreamingDither::begin_frame
    pub fn reset(&mut self) {
        self.started = false;
        self.next_y = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_rejects_empty_or_overflowing_frames() {
        assert_eq!(
            StreamingDither::new(0, 4, RegionDithers::default()).unwrap_err(),
            StreamError::EmptyFrame
        );
        assert_eq!(
            StreamingDither::new(4, 0, RegionDithers::default()).unwrap_err(),
            StreamError::EmptyFrame
        );
    }

    #[test]
    fn row_before_begin_frame_is_not_started() {
        let mut stream = StreamingDither::new(2, 2, RegionDithers::default()).expect("geometry");
        let mut scratch = [0.0f32; 18];
        let mut surface = StubSurface::new(2, 2);
        let gray = [128u8; 2];
        let labels = [DitherRegion::Background; 2];
        let result = stream.process_row(0, &gray, &labels, &[], &[], &mut scratch, &mut surface);
        assert_eq!(result, Err(StreamError::NotStarted));
        assert_eq!(stream.next_y(), 0);
    }

    /// Minimal [`Surface`] for unit tests; the integration parity tests use
    /// their own capture surface.
    struct StubSurface {
        width: i32,
        height: i32,
    }

    impl StubSurface {
        fn new(width: u32, height: u32) -> Self {
            Self {
                width: width as i32,
                height: height as i32,
            }
        }
    }

    impl Surface for StubSurface {
        fn width(&self) -> i32 {
            self.width
        }
        fn height(&self) -> i32 {
            self.height
        }
        fn set(&mut self, _x: i32, _y: i32, _ink: bool) {}
    }
}
