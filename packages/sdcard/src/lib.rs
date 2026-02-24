#![no_std]

pub mod api;
pub mod fat;
pub mod power;
pub mod probe;
pub mod runtime;

pub use power::{power_off, power_on_for_io, SD_POWER_SETTLE_MS};

pub const SD_PATH_MAX: usize = 64;
pub const SD_WRITE_MAX: usize = 192;
