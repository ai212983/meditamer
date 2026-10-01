//! Product- and board-neutral rendering support.
//!
//! Extracted from `meditamer`'s `firmware::ui::lvgl` by ADR-0015 (Tier 1).
//!
//! Deliberately narrow. The LVGL backend cannot follow until `platform/ui/board`
//! exists, and the L8-to-panel blit that briefly lived here turned out to be
//! Inkplate framebuffer format, not neutral rendering — it now sits with the
//! board driver. See the ADR's "platform/ui/render, as far as it goes" section.

#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]

pub mod geometry;
#[cfg(feature = "lvgl")]
pub mod intent_bridge;
#[cfg(feature = "lvgl")]
pub mod lvgl_adapter;

pub use geometry::DirtyArea;

#[cfg(feature = "ui-interaction-trace")]
pub mod interaction_trace;
