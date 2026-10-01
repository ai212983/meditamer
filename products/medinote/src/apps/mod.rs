//! Product apps: `hourglass`, `counter`, and optional network controls. This module is the place an
//! app slots in, not a general app framework -- `platform/ui/shell`'s
//! `SurfaceRegistry`/`types`/`catalogue` already cover provider/surface
//! registration, and Medinote reuses that directly rather than adding a
//! second, product-local shell layer (see the
//! [plan's ledger](../../../../docs/archive/features/medinote-hourglass-app-ledger.md)
//! for the scope decision).

pub mod counter;
pub mod descriptor;
pub mod hourglass;
#[cfg(feature = "network-controls")]
pub mod network;
