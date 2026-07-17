#![no_std]

#[cfg(any(target_os = "none", feature = "host-tests"))]
pub mod fat;
#[cfg(target_os = "none")]
pub mod power;
#[cfg(target_os = "none")]
pub mod probe;
#[cfg(all(not(target_os = "none"), feature = "host-tests"))]
#[path = "probe_host.rs"]
pub mod probe;
#[cfg(target_os = "none")]
pub mod runtime;

#[cfg(target_os = "none")]
pub use power::{power_off, power_on_for_io, SD_POWER_SETTLE_MS};

pub const SD_PATH_MAX: usize = 64;
pub const SD_WRITE_MAX: usize = 192;
