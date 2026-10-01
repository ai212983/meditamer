//! A procedural ensō, drawn as a brush stroke that grows with elapsed time.
//!
//! The whole drawing is a pure function of a seed and a progress value in
//! `0.0..=1.0`. Nothing is stored per frame and nothing accumulates, so a
//! session's entire visual state is one scalar plus the seed, and a five-minute
//! sit and a twelve-hour one cost exactly the same: only the rate at which
//! progress advances differs.
//!
//! Every frame is a finished ensō rather than a partially drawn one -- the arc
//! grows to reveal more of a fixed ink field and pressure curve, and two slow
//! ramps keep thickening and darkening the whole mark as it does, so growth
//! reads as one drawing developing rather than a cursor tracing a finished one.
//!
//! ```no_run
//! use enso::Stroke;
//! use raster::{BitCanvas, Dither};
//!
//! let stroke = Stroke::from_seed(0xC0FFEE, 600.0);
//! let mut bits = [0u8; BitCanvas::bytes_for(600, 600)];
//!
//! // Each frame is a full redraw, so clear before rendering. `Stroke::bounds`
//! // gives the only region that needs it.
//! for step in 1..=4 {
//!     bits.fill(0);
//!     let mut canvas = BitCanvas::new(600, 600, &mut bits).expect("canvas");
//!     stroke.render(step as f32 / 4.0, Dither::Gradient, &mut canvas);
//! }
//! ```

#![no_std]
#[cfg(test)]
extern crate std;

mod curve;
mod ink;
pub mod params;
pub mod rng;
pub mod stroke;

pub use params::{EnsoParams, Entry, Exit, Harmonic};
pub use rng::Rng;
pub use stroke::Stroke;
