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
/// may only have one path. Ask the concrete board's own `supports` method
/// rather than assuming.
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
    /// The panel does not implement this mode; check the concrete board's
    /// own `supports` method.
    Unsupported(RefreshMode),
    /// The panel rejected the update or its hardware did not respond.
    Panel,
}

/// The seam every panel shares: geometry and synchronous L8 ingestion.
///
/// Refresh is deliberately not here. The Inkplate drives it through
/// asynchronous I²C power sequencing and timed waveform work — genuinely
/// different in kind from the ST7305's synchronous SPI push — so each board
/// exposes its own concrete `refresh`/`supports` methods instead of forcing
/// both through one synchronous trait method. [`RefreshMode`] and
/// [`RefreshError`] stay here because both boards' concrete APIs use them.
pub trait Panel {
    fn geometry(&self) -> Geometry;

    /// Accept an LVGL L8 (8-bit grayscale) region, packing it into whatever
    /// this panel's hardware expects. `pixels` is row-major with no padding.
    /// The implementation may clip a partially overlapping region to its
    /// panel bounds. It returns `false` without writing when the area is empty,
    /// has no overlap, its exact dimensions cannot be represented, or
    /// `pixels` is shorter than the area's width x height.
    ///
    /// Raw LVGL pixel pointers are converted to slices by the callback adapter;
    /// panel implementations receive only a bounded, safe view.
    fn blit_l8(&mut self, area: DirtyArea, pixels: &[u8]) -> bool;
}
