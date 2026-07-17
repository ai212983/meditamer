# Rust Analyzer Baseline

Run the repository baseline analyzer script:

```bash
scripts/ci/lint_rust_analyzer.sh
```

By default this uses the `stable` Rust toolchain for analyzer execution. Override with:

```bash
RUST_ANALYZER_TOOLCHAIN=<toolchain> scripts/ci/lint_rust_analyzer.sh
```

## Purpose

This baseline provides fast static signal that complements (but does not replace)
`cargo check` and strict `cargo clippy`.

## Workspace-specific limitations

- Firmware is `no_std` and heavily feature/cfg gated.
- Baseline runs with `--disable-build-scripts --disable-proc-macros` for reproducible CI output.
- Analyzer diagnostics can include inactive-code and unresolved-import noise outside active runtime feature paths.
- Treat analyzer output as triage signal; authoritative pass/fail gates are:
  - `cargo +esp check -Zbuild-std=core,alloc --target xtensa-esp32-none-elf --workspace --all-features --bins --lib`
  - `cargo +esp clippy -Zbuild-std=core,alloc --target xtensa-esp32-none-elf --workspace --all-features --bins --lib -- -D warnings`
