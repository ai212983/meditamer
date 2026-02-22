use std::{env, fs, path::PathBuf};

use event_config_compiler::generate_from_path;

fn main() {
    let manifest_dir =
        PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"));
    let config_path = manifest_dir.join("../../config/events.toml");

    println!("cargo:rerun-if-changed={}", config_path.display());

    let generated = generate_from_path(&config_path).unwrap_or_else(|e| {
        panic!(
            "event config compile failed for {}: {e}",
            config_path.display()
        )
    });

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("missing OUT_DIR"));
    let out_file = out_dir.join("event_config.rs");
    fs::write(&out_file, generated)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", out_file.display()));
}
