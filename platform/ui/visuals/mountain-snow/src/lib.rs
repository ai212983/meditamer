//! Ambient Home mountain-snow progression: Phase-3 runtime shape.
//!
//! [`policy`] maps one parent-tick temperature sample to snow coverage
//! with hysteresis and a minimum visible step, requesting no refresh of
//! its own. [`pmv`] retains the independent Fanger heat-balance model but
//! does not control the current mountain. [`composer`] resolves one
//! caller-owned image row to packed one-bit pixels through a
//! barrier-level cut, so the device streams source rows instead of
//! holding the full order field.
//!
//! No heap, no statics, one dependency (the `asset-source` access
//! contract, itself dependency-free): host and device agree bit for
//! bit. All retained buffers (canvas, row scratch) and the source rows
//! themselves stay with the caller — on the device, fallibly in PSRAM
//! and on SD respectively.

#![no_std]

#[cfg(test)]
#[macro_use]
extern crate std;

pub mod composer;
pub mod pack;
pub mod pmv;
pub mod policy;
pub mod source;
