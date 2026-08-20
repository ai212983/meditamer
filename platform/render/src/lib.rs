//! Product- and board-neutral rendering support.
//!
//! Extracted from `meditamer`'s `firmware::ui::lvgl` by ADR-0015 (Tier 1).
//! See the crate manifest for why the rest of that module could not follow.

#![cfg_attr(not(test), no_std)]

pub mod dither;
pub mod intent_bridge;
