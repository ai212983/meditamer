//! Generates `event_config.rs`, `ota_build_config.rs`, and
//! `ambient_clock_font.rs` for `firmware::event_engine`, `firmware::update`,
//! and `firmware::ui` respectively.
//!
//! Split out of the root package's `build.rs` in the product and target axis
//! completion plan's Phase 3: everything here is product content (event
//! tuning, OTA build identity, a UI font), consumed by code that moved to
//! this crate. `emit_legacy_wifi_link_wrappers` stayed behind in
//! `targets/meditamer-inkplate/build.rs` -- it emits `cargo:rustc-link-arg`,
//! which only has an effect for the final linked binary, not a library.
//!
//! `config/events.toml` and `assets/fonts` stay at the repo root rather than
//! moving here: `test-support/host/event_engine_host_harness` and
//! `test-support/host/ui_shell_host_harness` already reach the same files
//! from their own nested locations (`#[path]`-shadowing this crate's source
//! rather than depending on it -- see those crates' own doc comments), and
//! duplicating the config would let the two copies drift.

use std::{env, fs, path::PathBuf};

use event_config_compiler::generate_from_path;
use lvgl_font_compiler::{
    generate_ambient_clock_font_for_render, generate_ambient_environment_font_for_render,
};

const OTA_PUBLIC_KEY_ENV: &str = "MEDITAMER_FIRMWARE_PUBLIC_KEY_HEX";
const OTA_BUILD_ID_ENV: &str = "MEDITAMER_FIRMWARE_BUILD_ID";

fn decode_hex_byte(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn write_ota_build_config(out_dir: &std::path::Path) {
    println!("cargo:rerun-if-env-changed={OTA_PUBLIC_KEY_ENV}");
    println!("cargo:rerun-if-env-changed={OTA_BUILD_ID_ENV}");
    let configured = env::var(OTA_PUBLIC_KEY_ENV).ok();
    let build_id = env::var(OTA_BUILD_ID_ENV).unwrap_or_else(|_| "unlabeled".to_owned());
    assert!(
        !build_id.is_empty()
            && build_id.len() <= 31
            && build_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')),
        "{OTA_BUILD_ID_ENV} must be 1-31 ASCII letters, digits, '.', '_' or '-'"
    );
    let mut key = [0u8; 32];
    if let Some(raw) = configured.as_deref() {
        let bytes = raw.as_bytes();
        assert_eq!(
            bytes.len(),
            64,
            "{OTA_PUBLIC_KEY_ENV} must contain 64 hex characters"
        );
        for (index, pair) in bytes.chunks_exact(2).enumerate() {
            let high = decode_hex_byte(pair[0])
                .unwrap_or_else(|| panic!("{OTA_PUBLIC_KEY_ENV} contains a non-hex character"));
            let low = decode_hex_byte(pair[1])
                .unwrap_or_else(|| panic!("{OTA_PUBLIC_KEY_ENV} contains a non-hex character"));
            key[index] = (high << 4) | low;
        }
    }

    let key_literal = key
        .iter()
        .map(|byte| format!("0x{byte:02x}"))
        .collect::<Vec<_>>()
        .join(", ");
    let generated = format!(
        "pub(crate) const OTA_PUBLIC_KEY_CONFIGURED: bool = {};\n\
         // Only the `factory-updater` binary (targets/meditamer-inkplate/src/updater/) verifies\n\
         // bundle signatures with the raw key bytes now that ADR-0014 Phase 5 removed the default\n\
         // binary's own signature verification path; other feature combinations legitimately never\n\
         // read this constant.\n\
         #[allow(dead_code)]\n\
         pub(crate) const OTA_PUBLIC_KEY: [u8; 32] = [{key_literal}];\n\
         pub(crate) const OTA_BUILD_ID: &str = {build_id:?};\n",
        configured.is_some(),
    );
    let path = out_dir.join("ota_build_config.rs");
    fs::write(&path, generated)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", path.display()));
}

/// Compiles the Ambient Home clock-overlay face.
/// LVGL's built-in Montserrat stops at 48 px, so the 128 px clock is compiled
/// from the vendored face over the small `HH:MM` character set.
fn write_ambient_fonts(repo_root: &std::path::Path, out_dir: &std::path::Path) {
    let assets = repo_root.join("assets/fonts");
    println!("cargo:rerun-if-changed={}", assets.display());

    let generated = generate_ambient_clock_font_for_render(&assets)
        .unwrap_or_else(|error| panic!("ambient clock font compile failed: {error}"));

    let path = out_dir.join("ambient_clock_font.rs");
    fs::write(&path, generated)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", path.display()));

    let generated = generate_ambient_environment_font_for_render(&assets)
        .unwrap_or_else(|error| panic!("ambient environment font compile failed: {error}"));
    let path = out_dir.join("ambient_environment_font.rs");
    fs::write(&path, generated)
        .unwrap_or_else(|error| panic!("failed to write {}: {error}", path.display()));
}

fn main() {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"));
    let repo_root = manifest_dir
        .join("../..")
        .canonicalize()
        .expect("products/meditamer is two levels below the repo root");
    let config_path = repo_root.join("config/events.toml");

    println!("cargo:rerun-if-changed={}", config_path.display());

    let generated = generate_from_path(&config_path).unwrap_or_else(|e| {
        panic!(
            "event config compile failed for {}: {e}",
            config_path.display()
        )
    });

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("missing OUT_DIR"));
    write_ota_build_config(&out_dir);
    write_ambient_fonts(&repo_root, &out_dir);
    let out_file = out_dir.join("event_config.rs");
    fs::write(&out_file, generated)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", out_file.display()));
}
