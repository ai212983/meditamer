#![no_std]

pub mod assets;
pub mod drivers;
pub mod event_engine;
pub mod gpio_fast;
#[cfg(feature = "graphics")]
pub mod graphics;
pub mod platform;

pub use drivers::inkplate as inkplate_hal;

#[cfg(feature = "graphics")]
pub use graphics::shanshui;
#[cfg(feature = "graphics")]
pub use graphics::sumi_sun;
#[cfg(feature = "graphics")]
pub use graphics::suminagashi;
