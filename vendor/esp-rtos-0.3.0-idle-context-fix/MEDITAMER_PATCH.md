# Meditamer idle-context stack/reschedule backport

Status: repository-owned backport of an upstream fix, tracking eventual removal

## Immutable base

- Package: `esp-rtos` 0.3.0
- Crates.io checksum: `551f90766e1527edaa0c91e8d559e9e2a60397b545e93357ac61fb31845e5712`
- Upstream source revision (release tag `esp-rtos-v0.3.0`): `902a7c668f95279e731758e9e6ecdeb852aa8101`
- License: MIT OR Apache-2.0
- Patched crate-tree SHA-256 (excluding this manifest):
  `517c3b9375804477bc6bf31278cc6019a3fdb4f078bca8b9fce5e209fb47ba8a`

## Why this backport exists

Backports [esp-rs/esp-hal#6027](https://github.com/esp-rs/esp-hal/pull/6027) ("Fix a few silly
mistakes", merged as `998e4faeaf0afc92b494ece4edc75e80df5624f2`) onto the exact `esp-rtos` 0.3.0 already
pinned by the root manifest. See
[`../esp-backtrace-0.19.0-window-spill-fix/MEDITAMER_PATCH.md`](../esp-backtrace-0.19.0-window-spill-fix/MEDITAMER_PATCH.md)
for the shared "why now" context (the device-side `LoadStoreError` / `EXCVADDR=0x4000c0d4` panic inside
`esp_rtos::run_queue::RunQueue::mark_task_ready`, called from the `__level_1_interrupt` handler chain,
and the fact that no released version of the three crates PR #6027 touches yet contains `998e4fa`).

## Maintained delta

Three independent changes from the PR, applied to `src/lib.rs`, `src/scheduler.rs`, and
`src/task/xtensa.rs`:

- **`src/lib.rs`** — `allocate_main_task`'s stack-slice length was computed in bytes
  (`stack_top as usize - stack_bottom as usize`) but the slice element type is `MaybeUninit<u32>` (4
  bytes), so the slice covered 4x the real stack and `ensure_no_stack_overflow`'s bound check against it
  was correspondingly loose. Fixed on both the main-core path (`start_with_idle_hook`) and the
  second-core path (`start_second_core_with_stack_guard_offset`, using `STACK_SIZE`) by dividing the
  element count by 4.
- **`src/scheduler.rs`** (the file containing `RunQueue::mark_task_ready`, matching the observed panic's
  call chain) — `SchedulerState::run_scheduler` previously only checked
  `ensure_no_stack_overflow`/re-queued the current task when `read_thread_pointer()` was non-null. The
  idle context has no `Task` and runs on the *main* task's stack with a null thread pointer — identical
  to what a task that just deleted itself also looks like — so the idle path silently skipped stack
  overflow checking, and a deep idle hook could overflow the main stack unnoticed. Fixed by adding a
  `CpuState::idle` flag (set right before every context switch, `next_task.is_none()`) so the reschedule
  path can distinguish "idling on the main stack" (check `main_task`'s guard) from "task deleted itself"
  (nothing to check — that stack is about to be freed) when the thread pointer is null. The `if let
  Some(current_task) = current_task { ... }` block that combined the stack check and the
  ready-state-requeue is split in two so the stack check can run on the idle/main-task path while the
  requeue-if-ready logic still only applies to a real current task.
- **`src/task/xtensa.rs`** — doc-comment-only correction: `PS_UM` was documented as not mattering yet;
  it actually selects the user (vs. kernel) exception vector, and tasks must run with it set because the
  interrupt handlers do too (see the `xtensa-lx-rt` backport in this repo). No code change here — tasks
  already set `PS_UM` in their initial `PS` value.

**Not backported:** the PR's fourth `scheduler.rs` hunk, gated `#[cfg(all(multi_core,
sleep_light_sleep))]`, rewrites a `cpu_idle()` function that does not exist anywhere in the 0.3.0
released source (`sleep_light_sleep`/light-sleep support was added upstream after this release was cut).
There is nothing in 0.3.0 for that hunk to apply to.

## Maintenance rule

Re-run the device boot test (`scripts/device/flash.sh release`, watching for the
`LoadStoreError`/`EXCVADDR=0x4000c0d4` signature) after any change to this tree. Once `esp-rtos` (and
`esp-backtrace`/`xtensa-lx-rt`) all ship a released version containing commit `998e4fa`, drop this
vendor tree and its `[patch.crates-io]` entry, and depend on the released version directly.
