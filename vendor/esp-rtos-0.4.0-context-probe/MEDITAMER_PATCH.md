# Meditamer temporary context-copy probe

Status: repository-owned incident instrumentation, not an RTOS fix.

## Immutable base

- Package: `esp-rtos` 0.4.0, reviewed by full diff against the registry source.
- Upstream crate checksum:
  `2f8e99f512a839c78a77c2e55e1f0d208851cfa48935847d5e0b5e8b2c5a6289`
- Patched crate-tree SHA-256 (excluding this manifest):
  `0272570ae7d42083c96d58bea6c46de092266f6adcd31604b54bf060a72f6db9`
- License: MIT OR Apache-2.0
- Patched by: workspace root and `targets/meditamer-inkplate` via
  `[patch.crates-io]` at `vendor/esp-rtos-0.4.0-context-probe`.
  `targets/medinote-waveshare` does not patch it and keeps registry 0.4.0.

## Maintained delta

Exact diff against registry 0.4.0 (five files; everything else identical):

- `src/task/xtensa.rs`: `task_switch` records the first implausible saved or
  post-copy `THREADPTR` per core (stage 1 before, stage 2 after) around the
  two unchanged `copy_nonoverlapping` copies, and gains `#[ram]` so the added
  interrupt-path reads execute from IRAM. Task selection and both copies are
  unchanged; with the feature off the added blocks compile out.
- `src/context_probe.rs` (new): 72-byte first-bad-copy record (two per-core
  slots of nine words) in `.rtc_slow.persistent`, committed `MAGIC`-last and
  first-write-wins; `take()` drains and clears it at boot before the
  scheduler starts. Observes the copy only.
- `src/lib.rs`: `pub mod context_probe` under
  `#[cfg(all(feature = "context-probe", xtensa))]`.
- `Cargo.toml`: `context-probe = []` (no dependencies, off by default).
- `src/timer/mod.rs`: the cold `schedule` error formatter is out-of-line in
  `schedule_timer_failed` (`#[cold] #[inline(never)]`, runs from flash).
  Success path, retry-halving, and error type are unchanged.

Six source-file digests plus the whole-tree digest, including the package
manifest, are pinned in `scripts/ci/check_ble_controller_patch.sh`; the task-deletion
pins there (`scheduler.rs`, `task/mod.rs`, both identical to upstream) keep
the `queue_lifecycle` bounded-registry assumption intact.

## Safety bounds (source review only)

- No task-selection change: scheduler, run-queue, and task-set code are
  byte-identical to upstream.
- The plausibility window (`0` or `0x3ff0_0000..0x4000_0000`) only gates
  recording; it never rejects a context or alters control flow.
- Concurrency: each core writes only its own slot; a reset during a partial
  write leaves `MAGIC` unset so the next boot ignores it. The boot consumer
  (`targets/meditamer-inkplate/src/system.rs`) drains before allocator and
  scheduler startup.
- Interrupt path adds bounded volatile reads plus a conditional record: no
  allocation, UART, retry, or panic. The record shares `.rtc_slow.persistent`
  with other NOLOAD residents as a distinct symbol.
- IRAM: pinning `task_switch` to IRAM and perturbing release codegen can move
  a small number of flash-rodata literals into the IRAM handler pool (one
  diagnostic build showed +2 in the timer handler). The earlier "90 vs 87"
  count was a snapshot of that single diagnostic ELF, not a standing
  invariant; IRAM qualification belongs to
  `scripts/ci/check_iram_flash_refs.sh` and its baseline, which a later
  default-release artifact passed (48 vs 87).

## Provenance and qualification boundary

This document qualifies the vendored source only. Artifact proof (firmware
builds, IRAM/image/stack gates) and hardware proof (desk and device runs,
radio qualification) remain parent-owned aggregate-baseline gates; passing
builds or runs do not re-qualify this source.

Incident context lives in maintained documentation, not in a `docs/notes`
snapshot: `docs/references/compile-time-features.md` ("Temporary reset
diagnosis … These records supply context; they do not identify a writer or
fix a crash"), consumed at Inkplate boot via the `context-probe` feature
selected by `cpu-load`. Remove this patch and the feature once the context
writer is identified and the actual defect is repaired.
