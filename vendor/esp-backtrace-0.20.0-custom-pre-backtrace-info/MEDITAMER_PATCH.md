# Meditamer `custom_pre_backtrace(&PanicInfo)` patch

Status: repository-owned, permanent (project-owned hook extension, not an upstream fix being
tracked).

## Immutable base

- Package: `esp-backtrace` 0.20.0
- License: MIT OR Apache-2.0
- Repository: `https://github.com/esp-rs/esp-hal`, `path_in_vcs = "esp-backtrace"`,
  `git sha1 = 63e97be553f435f29171f7ae5f37a41ad737a75a` (from this tarball's
  `.cargo_vcs_info.json`)

## History

This folder replaces `esp-backtrace-0.19.0-window-spill-fix`. That fork carried two deltas onto
`esp-backtrace` 0.19.0:

1. A backport of [esp-rs/esp-hal#6027](https://github.com/esp-rs/esp-hal/pull/6027)'s window-spill
   register-corruption fix (`sp()`'s inline asm: `add a12,a12,a12` → `and a12,a12,a12`), part of the
   three-crate boot-crash fix described in `../README.md`.
2. The `custom_pre_backtrace(&PanicInfo)` signature extension below.

esp-hal 1.2.0 (2026-09-01) pulled in `esp-backtrace` 0.20.0, which already contains delta 1 upstream
(`src/xtensa.rs` is byte-identical to this tarball's copy — confirmed by diff when this fork was cut).
So this fork now carries **only** delta 2, rebased onto 0.20.0. Diffing this tree's `src/lib.rs`
against the unmodified 0.20.0 tarball shows exactly the lines below and nothing else; upstream's own
0.19.0→0.20.0 changes (e.g. `cfg_if!` → `cfg_select!`, an unrelated doc-logo URL update) are not part
of this patch and are carried through unmodified.

## Maintained delta: `custom_pre_backtrace` now receives `&PanicInfo`

Upstream's `custom-pre-backtrace` extension point (`pre_backtrace()` calling an `extern "Rust" fn
custom_pre_backtrace()`) takes no arguments, so a crash-screen implementation hooked there has no way
to read the panic message or `Location` — `PanicInfo` is only ever held by `panic_handler` itself.
Changed `pre_backtrace()` and the `custom_pre_backtrace` extern declaration to both take `info: &
core::panic::PanicInfo`, threaded through from `panic_handler`'s own argument.
`targets/medinote-waveshare`'s crash-screen hook (`src/crash_screen.rs`) and its
`panic-screen-probe` binary are the consumers.

This only changes the Rust-panic path (`panic_handler`/`pre_backtrace`). It says nothing about direct
hardware exceptions (`LoadStoreError` etc.) reaching a separate `exception-handler` vector, which this
crate has but which this project does not yet hook.

## Maintenance rule

Not tied to any upstream fix, so there is no planned removal. If a future `esp-backtrace` bump changes
`pre_backtrace`/`panic_handler`/the `custom-pre-backtrace` feature gate upstream, re-diff against the
new tarball and re-apply just this signature change; re-run the device boot test
(`scripts/device/flash.sh release`, and separately the `panic-screen-probe` binary) after any change to
this tree.
