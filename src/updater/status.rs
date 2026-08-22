//! Serial status lines. Same line-oriented, space-separated `KEY=value`
//! convention the production firmware uses (see `FIRMWARE_BOOT` in
//! `src/firmware/update.rs`), so host tooling that already parses that
//! convention needs no second parser for the updater.

use crate::firmware::update::Status as OtaStatus;

#[cfg(not(feature = "sd-qual-push"))]
use super::bundle_stream::{BundleReadError, VerifiedBundle};

pub(super) fn print_ota_status(status: &OtaStatus) {
    console::println!(
        "UPDATER_OTA_STATUS booted={} selected={} state={} build_id={}",
        status.booted.label(),
        status
            .selected
            .map_or("none", crate::firmware::update::Slot::label),
        status
            .image_state
            .map_or("none", crate::firmware::update::image_state_label),
        status.build_id,
    );
}

#[cfg(not(feature = "sd-qual-push"))]
pub(super) fn print_bundle_ok(path: &str, bundle: &VerifiedBundle) {
    console::println!(
        "UPDATER_BUNDLE_OK path={} build_id={} target={} layout={} bytes={}",
        path,
        bundle.header.build_id(),
        bundle.header.target_id,
        bundle.header.layout_id,
        bundle.total_bytes,
    );
}

#[cfg(not(feature = "sd-qual-push"))]
pub(super) fn print_bundle_error(path: &str, error: &BundleReadError) {
    console::println!("UPDATER_BUNDLE_ERROR path={} reason={:?}", path, error);
}
