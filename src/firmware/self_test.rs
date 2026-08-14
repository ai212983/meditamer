//! Runtime diagnostics session: SD and Wi-Fi self-checks driven from the serial console.

mod control;
mod model;
mod sd_checks;
mod wifi;

pub(crate) use control::diagnostics_task;
pub(crate) use model::read_diag_runtime_status;
