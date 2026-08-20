# Meditamer window-spill register-corruption backport

Status: repository-owned backport of an upstream fix, tracking eventual removal

## Immutable base

- Package: `esp-backtrace` 0.19.0
- Crates.io checksum: `37950e24b2dfd98f1581102d1798281d4d9547af881e6bffc2c2b534c026ec8f`
- Upstream source revision (release tag `esp-backtrace-v0.19.0`): `5c97f0d92b696dcabf53d721e3dca07a6d6e872c`
- License: MIT OR Apache-2.0
- Patched crate-tree SHA-256 (excluding this manifest):
  `b1b0632a46a64476eacd244ef5f87145605520f0898a8b42b5d18e93376d7016`

## Why this backport exists

Investigated alongside a device-side `LoadStoreError` panic (`EXCVADDR=0x4000c0d4`, fixed across every
run, ~1.1-1.5s after boot, always inside `esp_rtos::run_queue::RunQueue::mark_task_ready` called from
the `__level_1_interrupt` handler chain — see
[ADR-0014](../../docs/architecture/0014-single-production-sd-recovery-updater.md)'s pre-existing radio/
net-subsystem crash note). Upstream diagnosed and fixed a cluster of related Xtensa exception/interrupt
bugs together in
[esp-rs/esp-hal#6027](https://github.com/esp-rs/esp-hal/pull/6027) ("Fix a few silly mistakes"), merged
as commit `998e4faeaf0afc92b494ece4edc75e80df5624f2`. None of the three affected crates
(`esp-backtrace`, `esp-rtos`, `xtensa-lx-rt`) had a released version containing that commit as of this
backport (crates.io's newest versions of all three were cut 2026-04-16, before the 2026-08-03 merge), so
the fix is vendored onto the exact released versions already pinned by the root manifest instead of
waiting on a new release.

## Maintained delta

`sp()`'s inline asm used `add a12,a12,a12` five times, purely to force Xtensa register-window spilling
before reading the previous stack pointer at `sp - 12`. `add` can corrupt `a12`'s value across a
register-window rotation in a way `and a12,a12,a12` (a true no-op on the register's value) does not.
Upstream's fix (esp-hal#6027) replaces all five `add` instances with `and`. This crate has no other
change in the backport; it's included because the panic's stack-walking-adjacent register handling was
one of upstream's three co-fixed bugs, and this repository backports the full PR rather than
cherry-picking a subset. See
[`../esp-rtos-0.3.0-idle-context-fix/MEDITAMER_PATCH.md`](../esp-rtos-0.3.0-idle-context-fix/MEDITAMER_PATCH.md)
for the fix most directly implicated in the observed panic, and
[`../xtensa-lx-rt-0.22.0-user-mode-vector-fix/MEDITAMER_PATCH.md`](../xtensa-lx-rt-0.22.0-user-mode-vector-fix/MEDITAMER_PATCH.md)
for the PS.UM interrupt-vector fix that matches the panic's `__level_1_interrupt` call chain most
closely.

## Maintenance rule

Re-run the device boot test (`scripts/device/flash.sh release`, watching for the
`LoadStoreError`/`EXCVADDR=0x4000c0d4` signature) after any change to this tree. Once `esp-backtrace`,
`esp-rtos`, and `xtensa-lx-rt` all ship a released version containing commit `998e4fa`, drop this vendor
tree and its `[patch.crates-io]` entry, and depend on the released version directly.
