# RFC: Rust LOC Refactor Plan (>280 LOC Inventory, <300 LOC Target)

- Status: Proposed
- Last-reviewed: 2026-08-10
- Owner: Firmware + Host Tools
- Date: 2026-03-02
- Scope: Rust files currently above 280 LOC across firmware, packages, and tools

## 1. Summary

This RFC proposes a grouped refactor campaign to keep Rust modules maintainable by enforcing:

1. Hard limit: no Rust source file above 300 LOC.
2. Soft guardrail: avoid growing files above 280 LOC for newly touched modules.
3. Grouped execution by module/folder boundaries to reduce cross-file churn and broken APIs during split work.

## 2. Motivation

The codebase currently has 36 Rust files above 280 LOC, including 24 files above 300 LOC. Large files are concentrated in a few subsystems (`packages/sdcard`, `tools/hostctl`, scene tools, runtime/storage firmware). Refactoring one file at a time increases integration risk because concerns are spread across adjacent files that evolve together.

We should split by cohesive module groups, not by isolated file size.

## 3. Decision Drivers

- Keep compile/test/debug cycles predictable.
- Reduce merge conflicts in high-change files.
- Improve ownership boundaries and reviewability.
- Avoid partial splits that immediately regress due to neighboring oversized files.
- Preserve runtime behavior and protocol compatibility.

## 4. Current Inventory (All Rust Files >280 LOC)

### 4.1 Over 300 LOC (must split)

- `packages/sdcard/src/fat/api_mutate.rs` (449)
- `packages/sdcard/src/fat/names_lfn.rs` (402)
- `packages/sdcard/src/fat/dir_scan.rs` (336)
- `packages/sdcard/src/fat/cluster_utils.rs` (331)
- `packages/sdcard/src/fat/api_read_write.rs` (312)
- `tools/hostctl/src/workflows_runtime_modes.rs` (442)
- `tools/hostctl/src/scenarios.rs` (420)
- `tools/hostctl/src/serial_console.rs` (314)
- `tools/scene_maker/src/pipeline.rs` (494)
- `tools/scene_maker/src/cli.rs` (393)
- `tools/scene_viewer/src/cli.rs` (367)
- `tools/scene_viewer/src/render.rs` (310)
- `tools/scene_viewer/src/render/flow.rs` (335)
- `tools/touch_replay/src/main.rs` (320)
- `src/firmware/runtime/serial_task/tests.rs` (469)
- `src/firmware/runtime/serial_task/commands.rs` (340)
- `src/firmware/telemetry/recorders.rs` (462)
- `src/firmware/storage/upload/http.rs` (453)
- `src/firmware/psram/mod.rs` (409)
- `src/firmware/storage/sd_task.rs` (398)
- `src/firmware/runtime/diagnostics.rs` (379)
- `src/firmware/runtime/display_task/app_events.rs` (320)
- `src/firmware/storage/sd_task/receive.rs` (309)
- `src/firmware/storage/upload/wifi/connect/error/error_recovery.rs` (303)

### 4.2 281-300 LOC (near-limit, protect from growth)

- `packages/sdcard/src/fat/fat_mount.rs` (296)
- `packages/sdcard/src/runtime/fat_mutation.rs` (298)
- `packages/sdcard/src/api.rs` (296)
- `tools/hostctl/src/workflows_wifi_discovery/runtime.rs` (299)
- `src/firmware/storage/sd_task/upload/stream.rs` (300)
- `src/firmware/event_engine/features.rs` (297)
- `src/firmware/runtime/display_task/touch_loop.rs` (284)
- `src/firmware/touch/normalize/tests/part2.rs` (299)
- `src/firmware/touch/core/tests.rs` (295)
- `src/drivers/inkplate/control/touch.rs` (284)

## 5. Grouping Strategy

Split by subsystem boundaries with shared behavior/contracts, using one PR/commit sequence per group.

### Group A: SD/FAT core package (highest leverage)

Folder scope:

- `packages/sdcard/src/fat/*` + near-limit neighbors in `packages/sdcard/src/*`

Refactor intent:

- Separate public API surface from low-level helpers.
- Isolate path/LFN parsing from dir scan/mutation logic.
- Isolate cluster write/zero-fill helpers.

Suggested target modules:

- `fat/api/{mutate,read_write}.rs`
- `fat/path/{parse,lfn}.rs`
- `fat/dir/{scan,ops,reserve}.rs`
- `fat/cluster/{write,zero,alloc}.rs`

### Group B: Host workflow engine/runtime

Folder scope:

- `tools/hostctl/src/{scenarios.rs,workflows_runtime_modes.rs,serial_console.rs}`
- include near-limit `tools/hostctl/src/workflows_wifi_discovery/runtime.rs`

Refactor intent:

- Split workflow execution core from task-kind handlers.
- Split runtime-mode orchestration from mode-specific steps and context transforms.
- Split serial console transport from regex/parsing/wait helpers.

### Group C: Scene tools (`scene_maker`, `scene_viewer`)

Folder scope:

- `tools/scene_maker/src/{cli.rs,pipeline.rs}`
- `tools/scene_viewer/src/{cli.rs,render.rs,render/flow.rs}`

Refactor intent:

- Put CLI option parsing/model definitions in separate files.
- Split render/pipeline orchestration from pixel/channel transforms and output writers.

### Group D: Firmware storage/upload flow

Folder scope:

- `src/firmware/storage/sd_task.rs`
- `src/firmware/storage/sd_task/receive.rs`
- `src/firmware/storage/upload/http.rs`
- `src/firmware/storage/upload/wifi/connect/error/error_recovery.rs`
- include near-limit `src/firmware/storage/sd_task/upload/stream.rs`

Refactor intent:

- Separate state-machine orchestration from transport/protocol helpers.
- Split connection lifecycle, request parsing/body handling, and response/error mapping.

### Group E: Runtime and diagnostics

Folder scope:

- `src/firmware/runtime/serial_task/{commands.rs,tests.rs}`
- `src/firmware/runtime/diagnostics.rs`
- `src/firmware/runtime/display_task/app_events.rs`
- include near-limit `src/firmware/runtime/display_task/touch_loop.rs`

Refactor intent:

- Split serial command families into submodules.
- Split tests by command domain (state, sd, psram, ping, etc.).
- Split diagnostics session orchestration from check executors.
- Split display event handlers by event category.

### Group F: Firmware infra/utility heavy modules

Folder scope:

- `src/firmware/telemetry/recorders.rs`
- `src/firmware/psram/mod.rs`
- `tools/touch_replay/src/main.rs`

Refactor intent:

- Separate metric encoding/atomic record helpers from recorder facade.
- Separate psram allocator state machine, metrics, and buffer types.
- Separate touch replay parser, validation, and CLI entrypoint.

## 6. Refactor Rules

1. No file >300 LOC after each group lands.
2. Prefer directory modules (`foo/mod.rs` + focused files) for large splits.
3. Keep existing public APIs stable where possible; use re-export facades if needed.
4. Move tests with the code they verify.
5. Do not weaken diagnostics/errors to make splits easier.
6. Keep imports and module visibility explicit (`pub(crate)` by default).

## 7. Execution Plan

1. Complete Group A first (largest concentration and broad dependency impact).
2. Complete Group B next (host orchestration paths used in regression workflows).
3. Complete Group C (tooling quality-of-life and easiest to validate).
4. Complete Group D (firmware storage path; medium/high runtime risk).
5. Complete Group E (runtime command/diagnostic flows).
6. Complete Group F and remaining near-limit cleanups.

Each group should be split into small commits:

- commit 1: mechanical move/split with re-exports
- commit 2: internal cleanup and naming
- commit 3: test adjustments and assertions

## 8. Validation Requirements Per Group

- `cargo fmt --all`
- group-local `cargo check` target(s)
- run available unit tests for touched crate/module
- no LOC regressions above 300 in touched paths

Global guard check command:

```bash
while IFS= read -r f; do wc -l "$f"; done < <(rg --files -g '*.rs') | awk '$1>300'
```

Expected result: no output.

## 9. Risks and Mitigations

- Risk: behavioral regressions from moved logic.
  - Mitigation: preserve function signatures first, then clean internals.
- Risk: visibility/module-cycle issues.
  - Mitigation: split into layered modules (`types` -> `helpers` -> `run`), avoid cross-import loops.
- Risk: long-lived branch drift.
  - Mitigation: ship by group in independent commits/PRs.

## 10. Acceptance Criteria

1. All current files above 300 LOC are reduced below 300 LOC.
2. Refactors preserve behavior (`cargo check` and tests pass for touched crates).
3. New code follows soft guardrail (avoid introducing new 281-300 files when splitting).
4. Module boundaries are clearer (orchestration vs helpers vs types).

## 11. Open Questions

- Should we enforce a CI guard for max LOC (`>300` hard fail, `>280` warning)?
- Should test files have a separate threshold from production code?
- Should we normalize on `foo.rs` + `foo/` hybrid facades for major modules?
