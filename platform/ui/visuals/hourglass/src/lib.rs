//! The hourglass visual model: a lattice sand simulation whose glass can
//! assume any angle and whose sand responds continuously to gravity and the
//! throat. The simulation is a paper-anchored hybrid: a tested
//! Devlin-Schuster block rule plus explicit product geometry, pacing,
//! arbitrary-angle scheduling, and free-surface relaxation. See the active
//! [model reference](../../../../../docs/references/hourglass/model.md) and the
//! archived [product contract](../../../../../docs/archive/features/medinote-hourglass-app.md).
//!
//! Every module here is plain `core` `no_std` and free of any LVGL, product,
//! or hardware dependency, so the whole model is host-testable on its own.
//! This crate was born inside Medinote (`products/medinote`) but names
//! nothing product-specific; it moved here so any product wanting the same
//! visual model can depend on it directly. A consuming target supplies the
//! LVGL widget (or other presentation) and its own scheduling; Medinote's
//! `apps::hourglass::descriptor` (app-registration glue, not model code)
//! stayed behind and depends on this crate's [`model`] and [`presentation`].
//!
//! [`frame::render_frame`] paints the glass walls and rims, grains, and flow
//! tracers of one [`presentation::FrameData`] onto a `raster::Surface` (the
//! external `platform/ui/visuals/raster` crate's canvas contract, distinct from this
//! crate's own [`crate::raster`] projection module) -- the same one-bit
//! destination `enso` and `flipclock` draw into. A
//! consuming screen still owns canvas storage and token handling, clearing to
//! paper, labels, and invalidation; only the device-neutral pixel painting
//! lives here.

#![no_std]

pub mod backend;
pub mod fixed;
pub mod frame;
pub mod geometry;
pub mod model;
pub mod paper_backend;
pub mod paper_cellular;
pub mod particles;
pub mod presentation;
// Named `raster` for the same reason `platform/ui/visuals/raster` is: a per-frame
// lattice-to-pixel projection. The two are unrelated crates that happen to
// share a name -- this module is `crate::raster` here, `raster::...` only
// when someone reaches for the separate `platform/ui/visuals/raster` crate -- so don't
// let the coincidence obscure which concern each module owns.
pub mod raster;
pub mod rotation;
pub mod tracers;
