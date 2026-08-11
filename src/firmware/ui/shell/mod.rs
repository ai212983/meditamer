// The shell is wired incrementally. Remove this allowance once lifecycle,
// composition, and removal consumers come online in later gated phases.
#![allow(dead_code)]

pub(crate) mod callback_action_queue;
pub(crate) mod callback_routes;
pub(crate) mod catalogue;
pub(crate) mod composition;
pub(crate) mod intent_queue;
pub(crate) mod lifecycle;
pub(crate) mod model;
pub(crate) mod navigator;
pub(crate) mod registry;
pub(crate) mod settings;
pub(crate) mod timing;
pub(crate) mod types;

#[cfg(all(test, not(target_os = "none")))]
mod tests;
