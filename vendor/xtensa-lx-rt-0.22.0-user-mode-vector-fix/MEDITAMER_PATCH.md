# Meditamer interrupt-handler PS.UM backport

Status: repository-owned backport of an upstream fix, tracking eventual removal — primary suspect for
the observed panic

## Immutable base

- Package: `xtensa-lx-rt` 0.22.0
- Crates.io checksum: `409a9b4629d429e995cde4dfbd9fe562ccae66f7624514e200733fc5d0ea8905`
- Upstream source revision (release tag `xtensa-lx-rt-v0.22.0`): `347003de8a48320bb7724f53045be3afa9204411`
- License: MIT OR Apache-2.0
- Patched crate-tree SHA-256 (excluding this manifest):
  `c934b3db05280011dad6744d2a9c3d71a78faef5891ddb0c18199e3f1e0c9801`

## Why this backport exists

Backports [esp-rs/esp-hal#6027](https://github.com/esp-rs/esp-hal/pull/6027) ("Fix a few silly
mistakes", merged as `998e4faeaf0afc92b494ece4edc75e80df5624f2`) onto the exact `xtensa-lx-rt` 0.22.0
already pinned (transitively, via `esp-rtos`/`esp-hal`) by the root manifest's lockfile. See
[`../esp-backtrace-0.19.0-window-spill-fix/MEDITAMER_PATCH.md`](../esp-backtrace-0.19.0-window-spill-fix/MEDITAMER_PATCH.md)
for the shared "why now" context. This crate's change is the closest textual match to the observed
device panic: the fault is fixed inside `esp_rtos::run_queue::RunQueue::mark_task_ready`, called from
the `__level_1_interrupt` chain — exactly the assembly path this backport changes (`HANDLE_INTERRUPT_LEVEL`
and `__default_naked_exception`, both of which `call4 __level_N_interrupt`).

## Maintained delta

All changes are in `src/exception/asm.rs`, Xtensa exception/interrupt entry assembly:

- `HANDLE_INTERRUPT_LEVEL` (the macro instantiated per interrupt level, `\level | PS_WOE` →
  `\level | PS_WOE | PS_UM`), `__default_naked_exception`'s level-1-interrupt fast path
  (`1 | PS_WOE` → `1 | PS_WOE | PS_UM`), and its generic exception path (`PS_INTLEVEL_EXCM | PS_WOE` →
  `PS_INTLEVEL_EXCM | PS_WOE | PS_UM`) all now set `PS.UM` when entering a handler. `PS.UM` selects the
  *user* exception vector instead of the *kernel* one; both point at the same handler in this runtime,
  but a nested exception taken while `PS.UM` is clear (e.g. a window-overflow exception during register
  spilling, see below) would be dispatched through the vector this runtime does not expect, landing
  execution at effectively arbitrary code — consistent with a fixed, garbage `EXCVADDR` deep inside
  unrelated RTOS code such as `mark_task_ready`.
- `save_context`'s register-window-spill setup (used by every exception/interrupt entry, including the
  panic's `__level_1_interrupt` chain) clears `PS.EXCM` and sets `PS.WOE` to allow the spill's own
  window-overflow exceptions to fire, but previously left `PS.UM` clear while doing so — the exact
  window in which a nested exception could be misdispatched. Now sets `PS.UM` alongside `PS.WOE` here
  too, with a comment explaining why.
- `.set PS_UM, 0x00000020` gained an explanatory comment; no behavior change.

## Maintenance rule

Re-run the device boot test (`scripts/device/flash.sh release`, watching for the
`LoadStoreError`/`EXCVADDR=0x4000c0d4` signature) after any change to this tree. Once `xtensa-lx-rt`
(and `esp-rtos`/`esp-backtrace`) all ship a released version containing commit `998e4fa`, drop this
vendor tree and its `[patch.crates-io]` entry, and depend on the released version directly.
