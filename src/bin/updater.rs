//! Factory updater entry point (ADR-0014 / docs/plans/single-production-sd-recovery-updater.md,
//! Phase 1). A separate release artifact from `src/main.rs`; see
//! `[[bin]] name = "updater"` in Cargo.toml for why it needs
//! `--no-default-features --features factory-updater`.
#![no_std]
#![no_main]

use esp_backtrace as _;

#[esp_hal::main]
fn main() -> ! {
    meditamer::updater::run()
}
