//! The seam between the UI layer and a display panel.
//!
//! Deliberately narrow, and shaped by what two real panels have in common
//! rather than by what a display could conceivably do.
//!
//! The one thing both drivers genuinely share is the *input*: LVGL hands over
//! an 8-bit grayscale (L8) region plus the rectangle it covers, and the panel
//! packs it into whatever its hardware wants. Nothing else survives the pair.
//! The Inkplate packs column-major and bottom-up into a 1bpp buffer, then
//! drives waveforms through a TPS65185; the ST7305 packs four-pixel-wide by
//! two-pixel-tall blocks into descending twelve-pixel column groups and pushes
//! them over SPI. Their framebuffer bytes are not interchangeable, so this
//! trait never exposes one.

#![no_std]

/// Panel size in pixels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Geometry {
    pub width: u16,
    pub height: u16,
}

/// A rectangle of changed pixels, inclusive on both corners.
///
/// Lives here rather than beside the renderer because the panel is the more
/// primitive layer: a board can be driven without a renderer, but not the
/// reverse.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DirtyArea {
    pub x1: i32,
    pub y1: i32,
    pub x2: i32,
    pub y2: i32,
}

impl DirtyArea {
    pub fn union(self, other: Self) -> Self {
        Self {
            x1: self.x1.min(other.x1),
            y1: self.y1.min(other.y1),
            x2: self.x2.max(other.x2),
            y2: self.y2.max(other.y2),
        }
    }
}

/// How much of the panel to drive.
///
/// Not every panel offers both: e-paper distinguishes them because a partial
/// waveform is visibly different and much faster, while a memory-in-pixel LCD
/// may only have one path. Ask [`Panel::supports`] rather than assuming.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefreshMode {
    /// Drive the whole panel.
    Full,
    /// Drive only what [`Panel::blit_l8`] has accumulated since the last
    /// refresh.
    Partial,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefreshError {
    /// The panel does not implement this mode; check [`Panel::supports`].
    Unsupported(RefreshMode),
    /// The panel rejected the update or its hardware did not respond.
    Panel,
}

pub trait Panel {
    fn geometry(&self) -> Geometry;

    /// Whether [`Panel::refresh`] will honour this mode.
    fn supports(&self, mode: RefreshMode) -> bool;

    /// Accept an LVGL L8 (8-bit grayscale) region, packing it into whatever
    /// this panel's hardware expects. Returns `false` if the region was
    /// rejected — out of bounds, empty, or otherwise unusable — in which case
    /// nothing was written.
    ///
    /// # Safety
    ///
    /// `pixels` must point to at least `area` width x height readable bytes of
    /// L8 data, row-major with no padding, valid for the duration of the call.
    /// LVGL guarantees this for the buffer it passes to a flush callback.
    unsafe fn blit_l8(&mut self, area: DirtyArea, pixels: *const u8) -> bool;

    /// Drive accumulated content onto the glass.
    fn refresh(&mut self, mode: RefreshMode) -> Result<(), RefreshError>;
}
