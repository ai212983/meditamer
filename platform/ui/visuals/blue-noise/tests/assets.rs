//! On-disk asset check: the canonical `.bin` files the crate embeds must
//! exist at their exact byte sizes with a balanced threshold histogram.
//!
//! Both files are checked even with no map features enabled. Reading files
//! in a host test does not embed them in the default library build.

use std::collections::BTreeSet;
use std::fs;

fn crate_asset(name: &str) -> std::path::PathBuf {
    // Source-relative: independent of the test runner's working directory.
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join(name)
}

fn check(path: &std::path::Path, width: usize, height: usize, feature: &str) {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    assert_eq!(bytes.len(), width * height, "{feature}: exact byte size");
    let n = bytes.len();
    let mut hist = [0u32; 256];
    for &b in &bytes {
        hist[b as usize] += 1;
    }
    let lo = *hist.iter().min().unwrap();
    let hi = *hist.iter().max().unwrap();
    assert!(hi - lo <= 1, "{feature}: histogram spread {lo}..{hi}");
    let seen: BTreeSet<u8> = bytes.iter().copied().collect();
    assert_eq!(seen.len(), 256, "{feature}: all gray levels present");
    assert_eq!(n, width * height);
}

#[test]
fn asset_600x600() {
    check(
        &crate_asset("blue-noise-600x600.bin"),
        600,
        600,
        "map-600x600",
    );
}

#[test]
fn asset_400x300() {
    check(
        &crate_asset("blue-noise-400x300.bin"),
        400,
        300,
        "map-400x300",
    );
}
