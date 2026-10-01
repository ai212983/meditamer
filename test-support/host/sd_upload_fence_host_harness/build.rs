use std::{env, fs, path::PathBuf};

/// Derives the host-compilable upload vocabulary from the real product types
/// file. The SD core-path items (`SdResult`/`SdResultCode`) reference the
/// device-only `sdcard::runtime` module and cannot exist on host; they are
/// unrelated to upload fence behavior. Both removals are guarded so product
/// edits to those items fail this build loudly instead of drifting silently.
fn main() {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"));
    let src = manifest_dir.join("../../../products/meditamer/src/firmware/types/sd.rs");
    println!("cargo:rerun-if-changed={}", src.display());
    for pulled in [
        "../../../products/meditamer/src/firmware/storage/sd_task/upload/fence.rs",
        "../../../products/meditamer/src/firmware/storage/sd_task/upload/helpers.rs",
        "../../../products/meditamer/src/firmware/storage/sd_task/upload/types.rs",
        "../../../products/meditamer/src/firmware/storage/sd_task/upload/stream/finish.rs",
    ] {
        println!(
            "cargo:rerun-if-changed={}",
            manifest_dir.join(pulled).display()
        );
    }
    let text = fs::read_to_string(&src).expect("read product types/sd.rs");

    let alias = "pub(crate) type SdResultCode = sdcard::runtime::SdRuntimeResultCode;";
    assert_eq!(
        text.matches(alias).count(),
        1,
        "harness drift: SdResultCode alias changed; update stripping logic"
    );
    let struct_open = "pub(crate) struct SdResult {";
    assert_eq!(
        text.matches(struct_open).count(),
        1,
        "harness drift: SdResult struct changed; update stripping logic"
    );

    let path_anchor = "use super::SD_PATH_MAX;";
    assert_eq!(
        text.matches(path_anchor).count(),
        1,
        "harness drift: SD_PATH_MAX import changed; update rewriting logic"
    );
    let mut out = text.replace(
        alias,
        "// harness: device-only SdResultCode alias stripped (sdcard::runtime is target-gated).",
    );
    // The derived file lands directly in the harness `types` module, where
    // `SD_PATH_MAX` is already in scope, so the product anchor is dropped.
    assert!(out.starts_with(path_anchor) || out.contains(&format!("\n{path_anchor}")));
    out = out.replace(
        path_anchor,
        "// harness: SD_PATH_MAX anchor dropped (in scope).",
    );
    // The core-path request re-export is unused on host; silence it at the
    // item so no file-level attribute is needed.
    out = out.replace(
        "pub(crate) use sdcard::request::{SdCommand, SdCommandKind, SdRequest};",
        "#[allow(unused_imports)]\npub(crate) use sdcard::request::{SdCommand, SdCommandKind, SdRequest};",
    );
    let start = out.find(struct_open).expect("SdResult struct");
    let struct_end = out[start..]
        .find("\n}\n")
        .map(|i| start + i + 3)
        .expect("SdResult struct end");
    out.replace_range(
        start..struct_end,
        "// harness: device-only SdResult struct stripped (depends on SdResultCode).",
    );

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("missing OUT_DIR"));
    fs::write(out_dir.join("upload_sd_types.rs"), out).expect("write derived types");
}
