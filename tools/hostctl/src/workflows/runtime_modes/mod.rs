//! Runtime-mode smoke workflow.
//!
//! [`runtime`] holds the scenario runtime that drives the device; [`run`] is the
//! CLI entry point that wires options, logging, and the workflow file together.

mod run;
mod runtime;
#[cfg(test)]
mod tests;

pub use run::run_runtime_modes_smoke;
pub use runtime::RuntimeModesSmokeOptions;
