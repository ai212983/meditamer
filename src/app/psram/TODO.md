# PSRAM and Allocator Implementation Plan

## Scope
- Add a dedicated `psram` module for heap/allocator initialization and runtime checks.
- Prepare the firmware for large dynamic buffers (UI tiles, temporary image buffers).
- Keep current behavior stable when PSRAM is unavailable or not yet enabled.

## Current State
- The project is `no_std` and currently uses mostly static memory and `heapless` containers.
- No global allocator setup is present yet.
- Display framebuffers are currently static BW buffers in the Inkplate HAL.

## Goals
- Deterministic boot-time allocator initialization.
- Explicit memory placement policy for large buffers.
- Clear failure behavior when allocation fails.
- Feature-gated rollout to avoid regressions.

## Phase 1: Module Skeleton
- Create `src/app/psram/mod.rs`.
- Add a small public API surface:
  - `init_allocator()`
  - `allocator_status()`
  - `log_allocator_status()`
- Keep implementation as stubs first; return explicit status values.

## Phase 2: Allocator Integration
- Add `esp-alloc` dependency and feature-gate it (for example `psram-alloc`).
- Register global allocator in one place and initialize at boot.
- Wire initialization from `app::run()` before tasks start.
- Add clear panic/log path if allocator init fails.

## Phase 3: Memory Policy
- Define categories:
  - Internal RAM: timing-critical/interrupt-adjacent buffers.
  - PSRAM: large render/dither/work buffers.
- Add helper constructors for large buffers to centralize policy.
- Avoid ad-hoc allocations spread across modules.

## Phase 4: Instrumentation
- Add serial logs for:
  - allocator init result
  - total/available heap after boot
  - high-water marks after render paths
- Add a lightweight command/report hook in serial task for allocator diagnostics.

## Phase 5: Validation
- Boot smoke tests with allocator feature off and on.
- Soak test with repeated UI redraw workloads.
- Verify SD + touch + display tasks remain stable under memory pressure.
- Confirm no regressions in existing hardware test matrix runs.

## Risks
- Target/hardware mismatch (ESP32 vs ESP32-S3 PSRAM assumptions) can block rollout.
- PSRAM latency/cache behavior may hurt hot-path rendering if policy is too broad.
- Allocation failures can become non-deterministic without strict ownership rules.

## Exit Criteria
- Allocator initializes reliably on target hardware.
- Large-buffer allocation path exists and is centralized.
- Existing display/touch flows pass current smoke and soak checks.
- No boot instability introduced by allocator setup.
