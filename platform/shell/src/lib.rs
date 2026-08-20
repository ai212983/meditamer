//! Product-neutral application shell: surface registry, navigator, catalogue,
//! composition, and lifecycle. Extracted from `meditamer`'s
//! `firmware::ui::shell` by ADR-0015 (Tier 1); it names no concrete screen and
//! depends only on `core` and `heapless`.

#![cfg_attr(not(test), no_std)]
// The shell is wired incrementally. Remove this allowance once lifecycle,
// composition, and removal consumers come online in later gated phases.
#![allow(dead_code)]

pub mod callback_action_queue;
pub mod callback_routes;
pub mod catalogue;
pub mod composition;
pub mod intent_queue;
pub mod lifecycle;
pub mod model;
pub mod navigator;
pub mod registry;
pub mod settings;
pub mod timing;
pub mod types;

#[cfg(all(test, not(target_os = "none")))]
mod tests;
