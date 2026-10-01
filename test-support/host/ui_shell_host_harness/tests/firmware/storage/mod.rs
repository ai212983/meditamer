//! Real product SD loader intake, unmodified.
//!
//! `super::super::psram` inside resolves to `crate::firmware::psram` (the
//! host stub sibling), so the actual queue functions run here on host.

#[path = "../../../../../../products/meditamer/src/firmware/storage/ambient_assets.rs"]
pub mod ambient_assets;
#[path = "../../../../../../products/meditamer/src/firmware/storage/clock_assets.rs"]
pub mod clock_assets;
#[path = "../../../../../../products/meditamer/src/firmware/storage/mountain_assets.rs"]
pub mod mountain_assets;
