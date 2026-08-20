//! Product- and board-neutral rendering support.
//!
//! Extracted from `meditamer`'s `firmware::ui::lvgl` by ADR-0015 (Tier 1).
//!
//! Deliberately narrow. The LVGL backend cannot follow until `platform/board`
//! exists, and the L8-to-panel blit that briefly lived here turned out to be
//! Inkplate framebuffer format, not neutral rendering — it now sits with the
//! board driver. See the ADR's "platform/render, as far as it goes" section.

#![cfg_attr(not(test), no_std)]

pub mod geometry;
#[cfg(feature = "lvgl")]
pub mod intent_bridge;

pub use geometry::DirtyArea;
