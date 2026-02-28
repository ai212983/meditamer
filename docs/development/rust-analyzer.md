# Rust Analyzer Baseline

Run the repository baseline analyzer script:

```bash
scripts/lint_rust_analyzer.sh
```

By default this uses the `stable` Rust toolchain for analyzer execution. Override with:

```bash
RUST_ANALYZER_TOOLCHAIN=<toolchain> scripts/lint_rust_analyzer.sh
```

## Purpose

This baseline provides fast static signal that complements (but does not replace)
`cargo check` and strict `cargo clippy`.

## Workspace-specific limitations

- Firmware is `no_std` and heavily feature/cfg gated.
- Baseline runs with `--disable-build-scripts --disable-proc-macros` for reproducible CI output.
- Analyzer diagnostics can include inactive-code and unresolved-import noise outside active runtime feature paths.
- Treat analyzer output as triage signal; authoritative pass/fail gates are:
  - `cargo check --workspace --all-features --bins --lib`
  - `cargo clippy --workspace --all-features --bins --lib -- -D warnings`
