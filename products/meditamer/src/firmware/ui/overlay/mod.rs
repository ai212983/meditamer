//! Base system overlays (navigation cue, refresh control) — surfaces above the
//! active screen with an independent lifetime. See ADR-0007.

pub(in crate::firmware::ui) mod base_overlays;
pub(in crate::firmware::ui) mod clock;
mod clock_font;
pub(in crate::firmware::ui) mod clock_model;
pub(in crate::firmware::ui) mod frontlight_calibration;
pub(in crate::firmware::ui) mod refresh_control;
