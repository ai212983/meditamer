//! First analog-clock prototype: the shared CPU renderer intended for
//! Meditamer and Medinote.
//!
//! The whole picture is a pure function of borrowed sprite maps plus a
//! [`scene::ClockScene`]. Nothing is allocated and nothing is stored: a
//! caller evaluates one pixel ([`render::evaluate_pixel`]), streams rows
//! ([`render::render_gray_row`]), or dithers straight onto a
//! [`raster::Surface`] ([`render::render_surface`]) with only constant
//! stack scratch, so no full grayscale frame ever needs to exist. That is
//! the device-facing contract; the host preview in
//! `tools/analog_clock_preview` decodes PNGs and owns every buffer.
//!
//! Pipeline per pixel, all in dial units (dial radius = 1, y down, z up):
//!
//! 1. Dial base albedo from [`dial`] (ticks, hub, paper).
//! 2. Each hand inverse-mapped into sprite space around its pivot,
//!    bilinearly sampled (albedo, alpha, normal, specular), with its
//!    tangent-space normal XY rotated along with the sprite.
//! 3. Direct light (diffuse albedo plus independent additive Blinn-Phong
//!    specular) shadowed by deterministic area-light samples whose per
//!    sample blocking is the union over both hands; ambient stays
//!    unshadowed. Only hands above the receiver cast.
//! 4. Over-composite dial, hour, minute in linear light, then sRGB-encoded
//!    grayscale out, dithered exactly once at screen pixels.
//!
//! Float/`libm` is the accepted first prototype. Whether the device keeps
//! it is a later measurement, not a claim made here.

#![no_std]

#[cfg(test)]
extern crate std;

pub mod assets;
pub mod compose;
pub mod dial;
pub mod region;
pub mod regional;
pub mod render;
pub mod scene;
pub mod shadow_masks;
pub mod streaming;

pub use assets::{DialMap, HandMaps, Hands, HOUR_PIVOT, MINUTE_PIVOT};
// The shared full-screen threshold maps the BlueNoise path resolves
// through, re-exported so callers need no second dependency to inspect
// which map a frame uses. The maps themselves stay borrowed read-only data
// owned by the `blue-noise` crate.
pub use blue_noise::{default_map, map_for_size, BlueNoiseMap};
pub use compose::{
    compose_surface, packed_bits_len, physical_footprint, removal_keep_row, render_base_row,
    render_cached_row_with_masks, render_compose_row, render_compose_row_with_masks,
    render_dynamic_row, shade_cached_row_with_masks, CachedRowBuffers, Candidates, ComposeError,
    ComposeRowBuffers, CompositionMode, Footprint, MaskRowBuffers, UnknownCompositionMode,
};
pub use region::{
    render_region_row, DitherAlgorithm, DitherRegion, RegionDithers, RegionalDitherError,
};
pub use regional::{dither_flat, dither_regions, regional_scratch_len};
pub use render::{evaluate_pixel, render_gray_row, render_surface};
pub use scene::{angles_for_time, ClockScene};
pub use shadow_masks::{
    mask_row_counts, mask_row_scratch_len, prepare_mask_row, row_affected_range, MaskContext,
    MaskRowWork, MaskStats,
};
pub use streaming::{StreamError, StreamingDither};
// The ordered-dither thresholds the final grayscale resolves through,
// re-exported so callers need no second dependency for them.
pub use raster::Dither;
// The one-bit destination the regional path quantizes onto, re-exported so
// callers implementing their own surface need no second dependency either.
pub use raster::Surface;
