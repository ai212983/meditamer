pub mod assets;
mod bootstrap;
mod comms;
pub(crate) mod config;
pub(crate) mod event_engine;
#[cfg(feature = "graphics")]
pub mod graphics;
pub(crate) mod psram;
mod render;
mod runtime;
mod storage;
pub(crate) mod store;
mod touch;
pub(crate) mod types;
pub(crate) mod ui;

pub use bootstrap::run;
