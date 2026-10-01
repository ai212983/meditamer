# Source Tree Reorganization Plan

- Status: Done — all workstreams landed and verified 2026-08-10
- Last-reviewed: 2026-08-10
- Owner: Firmware + Host Tools
- Scope: Module layout across first-party Rust sources under `src/`, `packages/`,
  and `tools/`
- Depends on: Code size policy in `AGENTS.md`; this plan replaces the thresholds
  inherited from `rust-loc-refactor.md`

## Goal

Make the module tree describe the system. Today it partly describes the order in
which features arrived and the workarounds used to satisfy a line-count advisory
that never blocked anything.

W1-W5 are intended to preserve runtime behavior, but module-boundary and ownership
changes still carry compile-time, feature-matrix, and integration risk. W6 and the
file-size guard deliberately change CI policy. Each step lands independently.

## File-size policy used by this plan

Use **600 SLOC as an advisory** and **1000 SLOC as a hard ceiling** for first-party
non-test Rust files. Generated code and vendored sources are excluded; test modules
remain exempt.

The hard ceiling is an emergency boundary, not a decomposition target. A cohesive
700-line module is preferable to several smaller modules with shared state or false
boundaries. A file at or above 600 SLOC deserves an explicit cohesion review; a file over
1000 SLOC cannot be introduced or made larger. Existing hard-ceiling offenders may
be temporarily baselined only while an active plan names how they will be removed.

The ceiling is now wired into `scripts/ci/lint_code_analysis.sh`,
`config/rca-baseline.json`, `scripts/ci/check_rust_loc.sh`, hooks, and `AGENTS.md`.
The 2152-SLOC backend that this plan baselined is gone (W4); the `file_sloc`
baseline is now empty and no first-party non-test file exceeds the ceiling. New or
regressed production files above 1000 SLOC fail the ratchet.

## Current state

Measured after the dead-code removal that cleared ~7.5k unreachable LOC from the
Wi-Fi subsystem.

| Subsystem | LOC |
| --- | --- |
| `firmware/storage` | 12376 |
| `firmware/runtime` | 6240 |
| `firmware/touch` | 4105 |
| `drivers` | 3232 |
| `firmware/ui` | 2718 |
| everything else | < 1500 each |

Four problems, in dependency order:

1. **144 `include!` sites across 34 parent files** paste hand-written source into
   a parent instead of declaring modules. No module boundary, no privacy, no
   independent `use` list, degraded rust-analyzer across the seam.
2. **Connectivity lives under storage.** `firmware/storage/upload/wifi/` is the
   network stack, filed under a storage concern because it arrived with the
   asset-upload feature.
3. **Paths reach nine components** and stutter: `connect/error/error_recovery/`,
   `connect/prepare/prepare_start/`. Mostly a symptom of 1 and 2.
4. **`firmware/ui/lvgl/backend.rs` is 2152 SLOC** — more than twice the enforced
   hard ceiling and the single largest non-test outlier in the tree.

## Workstreams

### W1 — Convert `include!` to real modules — implemented 2026-08-10

Highest value. Do this first: it is the precondition for judging real file sizes,
and it removes most of the pressure that produced problems 3 and 4. The edits are
structurally repetitive, but they are not a textual search-and-replace.

For each parent or small cohesive cluster:

1. Declare real child modules and give each child its own imports.
2. Preserve the parent's existing outward API with explicit re-exports where
   callers currently rely on flat paths.
3. Rewrite `self` / `super` references against the new module boundary and use
   the narrowest workable visibility.
4. Compile and test the affected crate before converting the next cluster.

The child stops inheriting the parent's imports, which is the point. The compiler
will expose hidden dependencies, but it cannot decide the intended facade or
visibility contract.

Where the pieces turn out not to be separable, that is the answer: recombine them
into one file and leave it. A split that does not create a module boundary is not
a split.

Group by crate for ownership, but land large crates as several parent- or
facade-sized commits rather than one all-or-nothing conversion:

| Group | Parents | Sites |
| --- | --- | --- |
| `tools/hostctl` | 12 | 48 |
| `src/firmware` | 11 | 47 |
| `packages/sdcard` | 6 | 30 |
| `tools/scene_maker` | 2 | 8 |
| `tools/scene_viewer` | 3 | 11 |

Largest single parents: `tools/hostctl/src/workflows/flash_capture/mod.rs` (12),
`packages/sdcard/src/fat/engine/mutate/mod.rs` (10),
`tools/hostctl/src/scenarios/tests.rs` (8), `packages/sdcard/src/fat/mod.rs` (8).

The one legitimate first-party site is `src/firmware/event_engine/config.rs`,
which includes build-script output from `OUT_DIR`. It stays.

`scripts/ci/check_include_usage.sh` now inspects all tracked first-party Rust under
`src/`, `packages/`, and `tools/` and reports all 144 hand-written sites. It stays
advisory during W1. When it reports zero, flip `INCLUDE_USAGE_ENFORCE=1` in
`lefthook.yml` so the practice cannot return.

### W2 — Extract `firmware/net/` — implemented 2026-08-10

Move `firmware/storage/upload/wifi/` and generic Embassy network-stack setup to
`firmware/net/`, leaving `upload/` as a consumer rather than their owner.

Keep the upload-specific HTTP server under `firmware/storage/upload/http/`. Its
routes translate directly to `SdUploadCommand` and `sd_bridge`; moving it into
`net/` would make the generic network layer depend back on storage. Only reusable
TCP/server-loop infrastructure should move later, if a second consumer proves a
real abstraction.

Suggested shape:

```
src/firmware/net/
  wifi/          driver, scan, connect, state, policy
  runtime        network-stack setup and runner
src/firmware/storage/
  upload/
    http/        upload protocol, routes, and connection handling
    sd_bridge/   SD command bridge
  sd_task/
```

Do W2 after W1. Converting includes first means the moved tree is real modules,
so the move is a directory rename plus path fixes rather than a re-split.

### W3 — Collapse depth and stuttering names — implemented 2026-08-10

Once W1 lands, several directories will have one real module in them. Fold those
in and rename the stutters:

- `connect/error/error_recovery/` → `connect/recovery/`
- `connect/prepare/prepare_start/` → `connect/prepare/start/`
- `http/connection/body/` → flatten if W1 leaves fewer than three modules

Target: nothing deeper than seven path components, no directory whose name
repeats its parent.

### W4 — Split `firmware/ui/lvgl/backend.rs` — implemented 2026-08-10

2077 SLOC, more than twice the proposed 1000-SLOC hard ceiling. Unlike most work
here this is a genuine decomposition, not a move, so it carries real risk and
should land alone, last, with the panel and refresh CI guards green.

Split on responsibility, not on line count, and name the pieces for what they do.
Do not split it to hit a number — if the natural boundaries yield three files of
700 lines, that is a better outcome than nine files that share state.

### W5 — Rename test shards — implemented 2026-08-10

`touch/core/tests/part2.rs` and `touch/normalize/tests/part{1,2,3}.rs` are named
after their position, not their contents. Rename to the behavior each covers, or
recombine — test files are now exempt from the LOC advisory, so the split that
motivated them no longer buys anything.

### W6 — Guardrail against recurrence — implemented 2026-08-10

The ~7.5k LOC of dead Wi-Fi diagnostics survived because nothing detected a `.rs`
file that no crate target reached. `scripts/ci/check_orphan_modules.sh` now makes
that condition blocking in hooks and the quality lane.

Derive roots from every tracked first-party Cargo manifest and target rather than
hard-coding the firmware pair. Roots include package `src/main.rs` / `src/lib.rs`,
declared bins, `src/bin/`, build scripts, examples, and integration tests. Traverse
`mod`, hand-written `include!` during migration, and `#[path]`; generated `OUT_DIR`
content is a terminal edge rather than a tracked source file. Conventional
fixture, snapshot, and testdata directories are data and are excluded from source
candidates. Explicit cross-crate `#[path]` use counts as live reachability.

This is a source-reachability check across supported configurations, not a single
active build. It must follow both halves of a `#[cfg]` + `#[path]` pair —
`firmware/touch/mod.rs` declares `debug_log` twice under opposite cfgs, and a walker
that takes only the first branch reports the other supported file as dead.

## Sequencing

All workstreams are complete. They landed in the order W1 → W5 → W2 → W3 → W4 on
`fix/wifi_connectivity`, with the blocking reachability guard green at each step.

W2 and W3 were deliberately landed while the Wi-Fi and upload work on that branch
was still in flight, against this plan's own advice, at the owner's direction.

## Verification

Validate each small step in the crate and configurations it changes.

All steps:

```
scripts/ci/check_software_baseline.sh source
RCA_ENFORCE=1 RCA_RATCHET=1 scripts/ci/lint_code_analysis.sh
scripts/ci/check_include_usage.sh
scripts/ci/check_orphan_modules.sh
scripts/ci/check_rust_loc.sh
```

Additional lanes:

| Changed area | Required validation |
| --- | --- |
| `tools/hostctl` | `scripts/ci/check_software_baseline.sh host` |
| `packages/sdcard` | `scripts/ci/check_software_baseline.sh host` |
| `tools/scene_maker`, `tools/scene_viewer` | locked tests now run in the host baseline; add strict `cargo clippy --all-targets` after the existing source findings are fixed rather than suppressed |
| `src/firmware` structural moves | `scripts/ci/check_software_baseline.sh firmware-clippy` plus affected host harnesses |
| Wi-Fi, network, or upload | full `scripts/ci/check_software_baseline.sh firmware` plus `guides/wifi-regression-gate.md` |
| LVGL backend | firmware, UI-shell host, static panel/refresh guards, then an identified-artifact device and physical panel check |

Passing compilation is necessary for W1-W3 but not sufficient: run the relevant
crate tests and feature lanes above. W4 additionally needs physical evidence
because host, serial, and build success do not prove panel behavior.

## Acceptance criteria

1. The repository-wide `check_include_usage.sh` reports zero hand-written-source
   violations and runs with `INCLUDE_USAGE_ENFORCE=1`.
2. No radio, Wi-Fi connection policy, or generic network-stack ownership remains
   under `firmware/storage/`; upload-specific HTTP remains under `storage/upload`.
3. No path deeper than seven components; no directory name repeating its parent.
4. The 600-SLOC advisory and 1000-SLOC hard production-file ceiling are aligned
   across policy, scripts, and baseline; no first-party non-test file exceeds the
   hard ceiling.
5. No test file named for its position in a split.
6. `check_orphan_modules.sh` covers every first-party Cargo target and reports zero
   unreachable tracked Rust files.
7. Every step passes the validation lane for each crate and configuration it
   changes.

## Outcome

| Workstream | Result |
| --- | --- |
| W1 | 144 hand-written `include!` sites across 34 parents converted to real modules. `check_include_usage.sh` reports zero and runs with `INCLUDE_USAGE_ENFORCE=1` in `lefthook.yml` and the quality lane. |
| W2 | `firmware/storage/upload/wifi/` and the Embassy stack setup moved to `firmware/net/{wifi,runtime}`. `upload/` keeps only `http/`, `sd_bridge/`, and the HTTP server task; it is now a consumer of `net`. `UploadHttpRuntime` became `NetRuntime`. |
| W3 | `connect/error/error_recovery/` folded to `connect/retry/`; `connect/prepare/prepare_start/` to `connect/prepare/start/`; `hsm/core/` and `sdcard/runtime/` (one module each) folded into their parents. No directory name repeats its parent, and no file name repeats its directory. |
| W4 | `ui/lvgl/backend.rs` (2151 SLOC) split by responsibility into `backend.rs` (data model, 580) plus `init`, `frame`, `cycle`, `navigation`, and `overlay`. No first-party non-test file exceeds the 1000-SLOC ceiling. |
| W5 | `touch/core/tests/part2.rs` → `recontact.rs`; `touch/normalize/tests/part{1,2,3}.rs` → `presence.rs`, `filtering.rs`, `continuity.rs`. |

### Deviations

- **`connect/recovery/` was already taken.** W3 prescribed renaming
  `connect/error/error_recovery/` to `connect/recovery/`, but `connect/recovery.rs`
  already exists and owns radio-level recovery (disconnect, stop, reinit). The
  error-recovery tree became `connect/retry/` instead — same intent, no collision.
- **`http/connection/body/` and `.../routes/` were left alone.** W3 said to flatten
  `body/` "if W1 leaves fewer than three modules"; it left five, and `routes/` has
  three. Both stay by the plan's own condition.
- **Acceptance criterion 3 is partially met.** The nine-component paths are gone and
  the firmware tree's deepest is now eight. Eight-component paths remain at
  `net/wifi/connect/prepare/start/` (the shape W3 itself prescribes),
  `upload/http/connection/{body,routes}/` (excluded above), and under
  `tools/hostctl/src/`, where the crate prefix costs three components before the
  module tree starts. Reaching seven everywhere would need edits this plan does not
  describe.
- **W4 has no physical evidence yet.** See Remaining.

### State at the end of the 2026-08-10 session

`scripts/ci/check_software_baseline.sh all` passes — 39 lanes, exit 0.

Three failures predating this work were cleared along the way, none of them in
this plan's scope: `lint_host_tools.sh` did not set `DEP_LV_CONFIG_PATH` for
`ui_shell_host_harness` (and was aborting before it ever reached the
`touch_replay` lane, which hid a `clippy::erasing_op`); the event-config
snapshot lagged a deliberate `debounce_ms` change in `config/events.toml`; and
`wait_ack_since` used a trailing `\b` that can never match when a concurrent
writer runs the ack straight into the next line.

The four code-analysis ratchet offenders were also cleared, so
`config/rca-baseline.json` now carries **zero** baselined offenders in all four
categories — the ratchet no longer holds anything open. The largest first-party
non-test file is `src/firmware/ui/shell/model.rs` at 908 SLOC, a warning well
under the 1000 ceiling.

## W4 device evidence — 2026-08-10

Identified release artifact, full flash to the CH340 device `usbserial-2110`.
Artifacts under `logs/flash_capture_w4_backend_split_20260810/`.

- Release ELF `dd2eaa00f6ea84a563c137ef9e2275d130424674264a7ee30126f57eeeb85a27`,
  app `4e04bc6d938c060f657d8343e606c51f0f607e0131a8f8ebdcc08c9819de607e`. Full
  flash succeeded; the app-only fallback recorded in the UI/app ledger was not
  needed.
- Boot capture: touch ready, `startup_refresh=full refresh_ms=2554`, Home
  entered, `RUNTIME_READY app_state=ready display=ready`, `shell_aligned=true`,
  `integrity_ok=true`, all four fault flags false, and zero panic, Guru
  Meditation, watchdog, or abort. The refresh time matches the 2555 ms recorded
  for the pre-split baseline in the UI/app ledger's E-0002.
- 30-cycle UI lifecycle run (`hostctl test ui-lifecycle --cycles 30
  --max-baseline-drift-bytes 256`): **passed, zero violations**. 90/90 candidate
  and 90/90 settled checkpoints, exact `launcher → diagnostics → home` route on
  every cycle, high-water plateau held over the final 30 transitions, global heap
  current use constant, live-block counts constant per surface at 195/181/201,
  max transition 33455 us, min CPU0 stack headroom 96232.
  Log `1b24ff33…`, report `44c2b2d9…`.
- The 256-byte band is the characterization the UI/app ledger's E-0006 already
  established for this 128 KiB arena, not a threshold relaxed for this run. The
  observed spans were 124 used / 120 usable-total, **tighter than the 176/188
  E-0006 recorded**. Every other check ran fail-closed at zero tolerance.

- **Panel observation: pass.** The owner observed the device on the identified
  artifact and reported no visible or touch regression. This discharges the last
  Verification row and closes W4.

## Resolved questions

**Is `drivers/` vs `firmware/` still the right top split once `net/` exists?**
Yes — and `net/` confirms the split rather than straining it. Resolved
2026-08-10.

No layering rule was written down anywhere, so the criterion was recovered by
measuring the tree:

| | `drivers/` | `firmware/` |
| --- | --- | --- |
| References the other tree | 0 files | 7 files reference `crate::drivers` |
| Embassy tasks | 0 | 12 |

The dependency arrow is clean and one-way, and the executor stops at the
boundary. So the seam is not "hardware vs. software" but **passive board
abstraction vs. the running system**: `drivers/` is the Inkplate board — panel,
touch, IMU, GPIO — with no tasks, no policy, and no telemetry; `firmware/` is
everything that runs and decides.

`net/` lands unambiguously on the `firmware/` side of that line: an Embassy
task, a connect state machine, a retry-policy ladder, and 22 files reporting
telemetry. Its actual *driver* is `esp-radio`, a third-party crate behind the
feature seam in `net/wifi/backend.rs` — so the radio driver already sits outside
`src/drivers/`, at the crate boundary, which is where an external HAL belongs.
What this repo owns for Wi-Fi is policy, and policy is firmware.

Two things worth recording rather than acting on:

- **The criterion above is not stated in `AGENTS.md` or any ADR.** It holds
  today only because nothing has violated it. If it is worth keeping, it wants
  a line in `AGENTS.md` and ideally a guard, in the spirit of
  `check_orphan_modules.sh`.
- **`drivers/` is really `drivers/inkplate/` plus two support files.** The name
  promises more generality than it delivers, and `board/` would read truer. That
  is a rename with no structural gain, so it is deliberately not done here.
