# Wi-Fi history narrowing follow-up 63 (2026-03-20)

## Goal

Ensure the wdev-branch trampolines are kept in the ELF so we can reapply the
binary patch on the current instrumented build and compare `mac_event_window`
words 0-11 between patched/unpatched.

## Changes

- Added a `keep_wdev_branch_trampolines()` helper and call in
  `reset_wdev_branch_wrap_diag()` to force references to:
  - `wdev_process_panic_watchdog_trampoline`
  - `lmac_process_rx_suc_data_trampoline`
  - `pp_post_trampoline`
  - `wdev_process_rx_suc_data_trampoline`

This mirrors the existing `wdev_sniffer_probe_trampoline` keep pattern to avoid
LTO/GC removing the symbols.

## Blocker

Local builds are currently blocked by a `compiler_builtins` link conflict when
running `cargo build` or `scripts/build/build.sh` with `FIRMWARE_RUSTUP_TOOLCHAIN=esp-188`.

Error excerpt (summarized):

- std/alloc depends on `compiler_builtins =0.1.158`
- toolchain ships `compiler_builtins 0.1.160` in-tree
- cargo refuses to link two crates with `links = "compiler-rt"`

This prevents rebuilding the ELF to confirm the trampolines are retained.

## Next step

Resolve the toolchain build conflict (either toolchain update or a local
`[patch.crates-io]` override to the in-tree `compiler_builtins`), then rebuild
and re-check the ELF symtab for the trampolines before applying the binary patch.
