//! Medinote's product-owned screens and overlays, built through checked LVGL
//! handles.
//!
//! The target owns panel initialization, coordinator adapters, GPIO sampling,
//! and event dispatch. This module owns presentation and construction only;
//! it does not decide when a screen or overlay is entered.

pub mod overlay;
pub mod screen;

#[cfg(test)]
mod tests;
