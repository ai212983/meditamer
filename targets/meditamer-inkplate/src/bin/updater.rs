//! Factory updater entry point (ADR-0014 / docs/plans/single-production-sd-recovery-updater.md,
//! Phase 1). A separate release artifact from `src/main.rs`; see
//! `[[bin]] name = "updater"` in Cargo.toml for why it needs
//! `--no-default-features --features factory-updater`.
//!
//! Moved from the root package's `src/bin/updater.rs` in the product and
//! target axis completion plan's Phase 3, together with `src/updater/`
//! (this crate's `targets/meditamer-inkplate/src/updater/`) -- a firmware
//! artifact, not product content, per the plan's "artifact configuration"
//! scope for this target. `#[path]`, not a shared `[lib]`: this binary is
//! `src/updater/`'s only consumer, and the main `meditamer` binary's own
//! module tree (`system`) does not need it either.
#![no_std]
#![no_main]

use esp_backtrace as _;

// See `main.rs`'s identical call for why each `[[bin]]` needs its own now.
esp_bootloader_esp_idf::esp_app_desc!();

#[path = "../updater/mod.rs"]
mod updater;

#[esp_hal::main]
fn main() -> ! {
    updater::run()
}
