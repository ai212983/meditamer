# Medinote Hourglass App Implementation Ledger

- Status: Done — all five phases accepted; final committed-device evidence is E-0006. Duration
  selection, arbitrary-angle hardware input, long-soak regression, and runtime CPU-frequency policy
  are explicit follow-ons rather than open Hourglass V1 work.
- Last-reviewed: 2026-08-24
- Archived: 2026-08-24
- Started: 2026-08-23
- Plan: [Medinote hourglass app](medinote-hourglass-app.md)
- Depends on: [Product and target axis completion](product-target-axis-completion.md) phases 1–3
  (passed, see [its ledger](product-target-axis-completion-ledger.md))

## Ledger contract

Same contract as the dependency plan's ledger: this ledger records implementation state, source
baselines, deviations, validation commands, and evidence produced while executing the linked plan.
Evidence entries are append-only; corrections use a new ID and name the entry they supersede.

## Scope decisions made before implementation (not resolved by the plan text)

The plan assumes shell/catalogue infrastructure and a power-mode policy that do not already exist for
Medinote (unlike Meditamer). Two decisions were confirmed with the user before writing code:

1. **Shell scope**: minimal. `platform/shell`'s `SurfaceRegistry`/`types`/`catalogue` are already
   linked by Medinote and reused as-is; no navigator/compositor/overlay machinery is added. The
   hourglass registers one `SurfaceRole::AppRoot` surface with `SurfaceCapabilities::LAUNCHABLE`, same
   shape the current bring-up code already uses, plus one real `CompiledCatalogue` entry (also
   `AMBIENT`, since it is the sole surface and its own fallback).
2. **Power mode**: Hourglass expresses only runtime intent: `HighPerformance` while Running and
   `Normal` while Ready, Paused, or Complete. It never enters Sleep or Deep Sleep. Those modes are
   explicit Home actions whose ESP32-S3 mechanics live in target-local `sleep.rs`; a later Inkplate
   implementation gets its own target module before any shared contract is extracted.

## Phase status

| Phase | State | Evidence | Next action |
| --- | --- | --- | --- |
| 1. Product foundation | Passed (app scaffold + catalogue only) | E-0001 | Shell entry/exit through the target is Phase 4's job. |
| 2. Rotation and deterministic mass model | Passed | E-0001 | — |
| 3. Physics and presentation | Passed | E-0001 | — |
| 4. Target integration | Passed on target | E-0002, E-0005 | Preserve through the final software gate. |
| 5. Device validation | Passed | E-0004, E-0005, E-0006 | — |

## Evidence entries — append only

### E-0001 — Phases 1-3: host-testable app (rotation, particle solver, mass ledger, catalogue)

- Date/phase: 2026-08-23; Phases 1 (product foundation, minus the target-side shell wiring), 2
  (rotation and deterministic mass model), and 3 (physics and presentation), executed together as one
  pass since they share no host/target boundary.
- Source baseline: `a53d714` (the plan's own proposing commit) plus this session's uncommitted worktree.
- Scope: added `products/medinote/src/apps/hourglass/{fixed,rotation,particles,model,presentation}.rs`
  and `products/medinote/src/catalogue.rs`, all `no_std`/host-testable, zero LVGL or hardware
  dependency. Added `heapless` and `shell` (path dependency on `platform/shell`) to
  `products/medinote/Cargo.toml`.
  - `fixed.rs`: Q16.16 `Fx`/`Vec2`/`Turns`, a 65-sample quarter-wave sine LUT (no `libm` anywhere in
    this repository, confirmed by grep before writing this -- `core::f32::sin` would fail to link on
    the real Xtensa target), `rotate`/`rotate_inverse`, and an integer-Newton `isqrt` for `Vec2::length`.
  - `rotation.rs`: timestamped `RotationCommand`/`RotationCommandKind::RotateBy`, and `RotationModel`
    integrating the plan's smoothstep segment (`s(u) = 3u^2 - 2u^3`) purely from a tick counter (never
    wall-clock time), so angle/angular-velocity are independent of render or command-arrival cadence by
    construction, not by a determinism test alone.
  - `particles.rs`: a fixed-capacity (96, see deviations) position-based granular solver in
    glass-local coordinates -- a symmetric bowtie boundary (two trapezoidal bulbs meeting at a
    zero-height throat), a bounded uniform-grid broad phase, particle-particle and particle-boundary
    contact projection, and restitution/friction against the wall's local velocity term.
  - `model.rs`: `SessionState`, `MassLedger` (per-bulb mass plus an EWMA observed crossing rate in real
    wall-clock time), `Calibration` (a one-time, cached reference discharge solve producing `k =
    T_ref/T`), and `HourglassModel` composing rotation + particles + ledger into one `tick()`.
  - `presentation.rs`: `FrameData::capture` and hand-rolled `format_remaining`/`status_label`, matching
    `medinote::presentation`'s existing fixed-buffer style.
  - `catalogue.rs`: one `shell::catalogue::CompiledCatalogue` entry (`Hourglass`, both `LAUNCHABLE` and
    `AMBIENT` -- with exactly one app, it is its own ambient fallback), built from an already-registered
    `SurfaceRef` rather than owning registration itself (that stays in `targets/medinote-waveshare`,
    matching the existing pattern main.rs already used pre-Phase-4).
- Scope decisions confirmed with the user before writing code (see this ledger's header): a minimal
  shell (`platform/shell`'s existing registry/catalogue, no navigator/compositor/overlay machinery
  added), and the hourglass replacing the always-on sensor UI as the default boot target (Phase 4).
- Material decisions and deviations found while executing:
  - **Physics runs entirely in glass-local coordinates**, derived directly from the plan's own
    `v_wall = angular_v * J * (x - c)` term: substituting `x = c + R(a) r` and using that `J` and `R(a)`
    commute (both are rotations) shows the local-frame wall velocity at a local point `r` reduces to
    `angular_v_rad * J * r` with **no rotation matrix at all** -- the boundary itself never rotates in
    local frame; only `gravity_local(a) = R(-a) * gravity_world` and this velocity term carry the
    angle. This is a derivation, not a simplification of the plan's model.
  - **The throat is one open polygon edge, not a wall**: each bulb is described as a closed
    quadrilateral (needed so the winding-agnostic convex-region signed-distance test has a consistent
    boundary to derive from), but the edge running along `y = 0` is excluded from containment
    (`ConvexRegion::open_edge`). Missing this the first time sealed the throat shut as an ordinary wall
    and stalled flow almost completely (13 of 96 grains crossed in 3000 ticks; a diagnostic run
    isolated it before the fix, after which the same trace fully drains in under 2000 ticks). Recorded
    here because nothing about the plan text names this as a pitfall -- it is a consequence of modelling
    each bulb as its own convex region.
  - **Solver tuning departs from the plan's suggested ranges** after the same diagnostic investigation:
    throat half-width 8 (not 6) -- roughly 4x grain diameter rather than 3x, since even with the open
    throat, 3x still arched and stalled flow -- and 6 solver iterations (not the plan's 3-5), for the
    same reason. Both are solver-quality knobs, not determinism concerns; `stepping_is_deterministic_for_a_fixed_trace`
    covers the latter directly.
  - **`PARTICLE_CAPACITY = 96`**, not the plan's suggested 384-768: chosen for fast, deterministic host
    tests, explicitly flagged in the source as provisional pending device timing/DRAM measurement this
    session cannot do (Phase 5 out of scope). The full test suite (43 tests, including several thousand
    physics ticks each) runs in ~11s on host at this count.
  - **The upright reference discharge (`T_ref`) is computed by an actual bounded simulation at
    `Calibration::for_duration` time** (cached process-wide via an `AtomicU32`, not per-`HourglassModel`),
    rather than a hardcoded constant -- this is what the plan's "obtain upright discharge time `T_ref`
    from a deterministic reference solve" literally asks for, and keeps the calibration correct
    automatically if solver tuning changes later instead of silently drifting from a stale constant.
    Converges in well under the 12,000-tick (240 model-second) safety cap in every test that exercises it.
  - **`Fx::div` renamed to `Fx::divide`**: clippy's `should_implement_trait` flagged the inherent `div`
    as shadowing `core::ops::Div::div`; no operator overload was added in its place; divide-by-zero
    should read as a real method call at each call site, not a `/` that panics silently.
  - **`SessionState::Complete` is current-state, not historical**: defined as `mass_in(Bulb::Upper) ==
    0`. A reversed flow after completion (flipping the glass) can refill `Upper` and leave `Complete`
    again -- documented in `model.rs` as a deliberate simplification matching an hourglass that has
    genuinely been turned over, not a bug.
  - **`DEFAULT_DURATION_S = 180`**: a placeholder three minutes: no duration-selection surface exists in
    this delivery scope (not named in any of the plan's five phases), so one fixed default stands in for
    it.
- Validation, in the repository's `archbox` distrobox with the xtensa toolchain on `PATH` (per this
  session's stored build-verification guidance):
  - `cargo test -p medinote`: 43/43 pass, including mass-conservation across upright and mixed-rotation
    traces, zero-flow at horizontal, boundary non-penetration, determinism for a fixed trace, and the
    catalogue's single-entry launcher/ambient views.
  - `cargo clippy -p medinote --all-targets -- -D warnings`: clean, on host.
  - `cargo fmt -p medinote --check`: clean.
  - Not yet run: a target build of `medinote` with `--features lvgl` against `targets/medinote-waveshare`
    (nothing in this module tree touches LVGL, so it is expected to be unaffected, but this is
    confirmed in Phase 4's evidence, not asserted here).
- Evidence class: source-verified, host-tested, clippy- and fmt-clean; not target- or
  hardware-verified (that is Phase 4's and Phase 5's scope respectively).
- Next action: Phase 4 (target integration) -- `boards/waveshare-rlcd42/src/buttons.rs` (`KEY` GPIO,
  flagged as an unverified pin assumption pending device access), `targets/medinote-waveshare/src/
  hourglass_task.rs` and `ui/hourglass_widget.rs`, and rewiring `main.rs`'s non-deep-sleep boot path.

### E-0002 — Phase 4: target integration (KEY input, Embassy task, LVGL widget, boot rewiring)

- Date/phase: 2026-08-23; Phase 4 (target integration), executed in full for the parts reachable
  without hardware; device-side validation is Phase 5 and out of scope this session.
- Source baseline: E-0001's staged tree (still uncommitted).
- Scope:
  - `boards/waveshare-rlcd42/src/buttons.rs`: `DebouncedEdge`, a hardware-free debounce/edge-detect
    state machine (3 consecutive same-state polls at the 50 Hz physics cadence, ~60ms, before a
    transition is trusted), and `KeyButton`, a thin wrapper polling one `esp_hal::gpio::Input` each
    physics tick. `KEY_GPIO_ASSUMPTION` documents the pin choice (GPIO0, the "BOOT" button convention
    common to ESP32/ESP32-S3 dev boards) as **unverified** -- this board's schematic was not available
    to this session (no device access); the constant and a boot-time `console::println!` both surface
    it rather than letting it read as a settled fact.
  - `boards/waveshare-rlcd42/src/panel_lvgl.rs`: two additions alongside the existing
    `set_active_panel`/`init` -- `set_power_mode` (the animated/idle transition the plan's rendering
    section asks for) and `force_full_refresh` (the completion cue, below), both thin wrappers over the
    same `ACTIVE_PANEL` raw pointer the flush callback already uses.
  - `targets/medinote-waveshare/src/hourglass_task.rs`: one Embassy task, not two -- physics
    (`HourglassModel::tick`) runs unconditionally every fixed-`PHYSICS_HZ` loop iteration via
    `embassy_time::Ticker`, `KEY` is polled and turned into a timestamped `RotateBy` command each
    iteration, and render (`hourglass_widget::render` + `lv_timer_handler`) runs every
    `RENDER_EVERY_N_TICKS` (2, ~25 Hz) iterations -- every physics step is preserved regardless of this
    divisor, matching the plan's "a delayed or skipped render frame changes animation smoothness, not
    orientation or transferred mass". Registers the hourglass surface through the same
    `shell::registry::SurfaceRegistry` pattern the pre-Phase-4 code used, and builds (but does not yet
    read back through a launcher, since there is only one app) the compiled catalogue from `E-0001`.
    Tracks `SessionState` transitions to switch `PowerMode::High`/`Low` and to fire the completion cue.
  - `targets/medinote-waveshare/src/ui/hourglass_widget.rs`: one `lv_canvas` widget with a
    statically-allocated 140x200 L8 buffer this module paints by hand every render pass (bowtie
    silhouette via a Bresenham line rasterizer, grains as small filled squares -- which, at this
    resolution, is the "stipple" texture the plan's rendering section asks for, not a separate dither
    step), plus a heading/status/remaining-time label. One `lv_obj_invalidate` per render pass, per the
    plan's "invalidate the hourglass bounding rectangle once per frame".
  - `targets/medinote-waveshare/src/main.rs`: the non-`DEEP_SLEEP` boot branch now spawns
    `hourglass_task` (with a `GPIO0` `Input` built from `buttons::key_input_config()`) instead of the
    former `sensor_task`/`ui_task` pair, which -- along with their now-unused `UI_DIRTY` signal -- were
    deleted outright rather than kept behind `#[allow(dead_code)]`, matching this repository's own
    precedent (E-0003 deleted the Phase 1 bring-up self-test the same way once superseded) over
    accumulating dead code that only git history needs to remember. `DEEP_SLEEP`'s sibling `cycle_task`
    (the deep-sleep sensor appliance path) is completely untouched.
- Deviations and material decisions:
  - **Completion cue is visual only**: a `panel_lvgl::force_full_refresh()` call fired exactly once on
    the transition into `SessionState::Complete`, distinguishable from the ordinary partial-refresh
    animation cadence, plus the status label's own text change to "Complete". No sound cue: this board
    has no buzzer/speaker confirmed anywhere in this repository (grepped before writing this), and the
    plan itself marks the sound cue "optional" -- left unimplemented rather than guessed at.
  - **`KEY` GPIO is an assumption, not a finding**: see scope above. Nothing in the rotation or physics
    code depends on which pin it turns out to be; only `main.rs`'s one `Input::new` call and the
    constant in `buttons.rs` would need to change.
  - **`static_mut_refs` (2024-edition lint)**: `hourglass_widget::render`'s background clear was
    initially written as `CANVAS_BUFFER.fill(0xFF)`, which briefly reborrows the whole `static mut` as
    `&mut [u8]` and is flagged under `-D warnings`. Rewritten as a per-element raw-pointer write
    (`(*(&raw mut CANVAS_BUFFER))[index] = 0xFF`), matching `set_px`'s existing indexed-write shape,
    which does not trigger the lint (confirmed by the clippy run below going clean) since it never forms
    a reference to the static, only a raw pointer plus per-element place projections.
  - **Two other real clippy findings, unrelated to the above**: `tick_index % RENDER_EVERY_N_TICKS ==
    0` rewritten as `tick_index.is_multiple_of(RENDER_EVERY_N_TICKS)` (`manual_is_multiple_of`), and a
    redundant `as u32` cast on an already-`u32` expression removed (`unnecessary_cast`). Both caught only
    by clippy against the real `xtensa-esp32s3-none-elf` target, not by a plain build.
- Validation, in the repository's `archbox` distrobox with the xtensa toolchain on `PATH`, new/changed
  files staged first (`git add -A`) so `git ls-files`-based checks see them (the same gotcha E-0002 of
  the dependency plan's ledger recorded for this exact tooling):
  - `cargo test -p medinote`: 43/43 pass (unaffected regression from E-0001 -- this phase touches no
    host-testable module).
  - `cargo clippy -p medinote --all-targets -- -D warnings`: clean, on host.
  - `targets/medinote-waveshare/build.sh` and `boards/waveshare-rlcd42/build.sh`: both link a real
    release binary end to end. `medinote-waveshare` total ELF size 859,785 bytes
    (`xtensa-esp32s3-elf-size -A`); no established DRAM budget exists yet for this board to compare
    against (noted already in the dependency plan's E-0007 -- Phase 5's job, not this one's), so this
    number is recorded as a baseline, not evaluated against a budget.
  - `cargo clippy --release -- -D warnings` against the real `xtensa-esp32s3-none-elf` target: clean for
    both `targets/medinote-waveshare` and `boards/waveshare-rlcd42`, after the three fixes above.
  - `cargo fmt --all --check` (root workspace) and `--check` against `targets/medinote-waveshare`'s own
    manifest: clean.
  - `scripts/ci/check_orphan_modules.py`: 663 tracked files (up from 651 pre-Phase-4), 57 target roots,
    31 manifests, zero unreachable.
  - `scripts/ci/check_rust_loc.sh`: 592 files; `targets/medinote-waveshare/src/main.rs` is now 782 lines
    (down from the 1021 recorded when Phase 2 of the dependency plan moved it, since `sensor_task`/
    `ui_task` were deleted, not added to); no new file crosses the 600-line warning band and no new
    high-attention (>1000-line) file.
  - `scripts/ci/check_script_surface.py`: clean (47/47), unaffected -- this phase adds no scripts.
  - `scripts/ci/check_markdown_links.sh` (this ledger and its parent plan) and
    `scripts/ci/check_markdown_loc.sh --staged`: both clean (this session's host environment has
    `lychee`, unlike `archbox`, so this is host-run rather than deferred).
  - Not run: any device-attached check -- flashing, the live Wi-Fi regression gate (not applicable to
    Medinote), `KEY` press behaviour, panel power-mode transitions, completion cue timing, or DRAM/stack
    headroom. All deferred to Phase 5 with real hardware.
- Evidence class: source-verified, host- and cross-toolchain-verified (build, clippy, fmt, orphan-module
  reachability); explicitly **not** hardware-verified -- every behaviour this phase adds (button
  debounce timing, panel power-mode switching, the completion cue, actual render/physics cadence under
  real SPI flush latency) is a real-hardware question this session cannot answer.
- Next action: Phase 5 (device validation), once device access is available -- flash and run end to end;
  confirm or correct the `KEY_GPIO_ASSUMPTION`; measure `.data`/`.bss`/stack remainder against a DRAM
  budget (none exists for this board yet, per the dependency plan's E-0007); measure physics/render
  runtime, missed frame deadlines, and cue latency; validate calibration, the 90-degree endpoints, and
  horizontal zero-flow pause on real sand-equivalent grain rendering; record results in the
  [hardware test matrix](../reference/hardware-test-matrix.md).

### E-0003 — Phase 5 preparation: board pin correction, bounded metrics, and target flash path

- Date/phase: 2026-08-24; Phase 5 preparation, before the first hourglass device run.
- Source baseline: branch commit `a3730524` plus the changes named below; no hardware result is claimed
  by this entry.
- Scope and corrections:
  - Corrected the provisional Phase 4 button assumption using Waveshare's board pin assignment: the
    active-low `KEY` input is GPIO18; GPIO0 is the separate `BOOT` input. `buttons.rs`, `main.rs`, and
    the boot diagnostic now agree on GPIO18.
  - Aligned the plan with the accepted minimal one-app shell: Medinote boots directly into its
    registered compiled hourglass surface. A launcher and leave/return lifecycle remain future scope.
  - Added bounded, allocation-free `HOURGLASS_METRICS` counters for physics, rendering, scheduling
    lateness, panel flush payload, and the one-shot completion cue.
  - Added `targets/medinote-waveshare/flash.sh`, a target-local ESP32-S3 build/flash/boot-capture path
    which records the exact ELF hash and Git source identity. It intentionally does not reuse the
    Inkplate target's ESP32 single-production bootloader and partition workflow.
- Validation: pending. The source correction and instrumentation must pass host/cross checks, then be
  flashed from a clean tracked commit before they can produce identified-device evidence.
- Evidence class: source correction and validation preparation only; not build-, target-, or
  hardware-verified in this entry.
- Next action: run the focused software gates, commit the exact source, and execute the Phase 5 device
  scenarios on the identified Waveshare board.

### E-0004 — Phase 5 software gate and ESP32-S3 DRAM baseline

- Date/phase: 2026-08-24; Phase 5 software gate, before device flashing.
- Source baseline: E-0003 worktree content.
- Validation:
  - `scripts/host-test.sh test medinote`: 43/43 tests pass.
  - `scripts/host-test.sh lint medinote`: strict host Clippy clean.
  - `targets/medinote-waveshare/build.sh --locked`: release firmware links successfully.
  - Target release Clippy with the build wrapper's ESP32-S3 cross/bindgen environment and
    `-D warnings`: clean.
  - `boards/waveshare-rlcd42/build.sh --locked`: release board library build clean.
  - Target and product `cargo fmt --check`, orphan-module reachability, script surface, Markdown links,
    shell syntax, and `git diff --check`: clean. Markdown LOC reports only advisory existing/expected
    long documents.
- Linked memory baseline (`xtensa-esp32s3-elf-size`): `.data` 36,832 bytes, `.bss` 94,172 bytes,
  `.stack` 200,180 bytes, and `.rwdata_dummy` 10,576 bytes. These exactly fill the generated
  341,760-byte `dram_seg`, as expected because `.stack` is its linker remainder. The Embassy
  hourglass task pool is 2,104 bytes within `.bss`; the largest app-owned buffers are the 49,152-byte
  LVGL work arena, 28,000-byte canvas, 16,000-byte draw buffer, 15,001-byte framebuffer, and
  11,201-byte panel state.
- Evidence class: source-, host-, cross-toolchain-, and linked-memory-verified; not yet flashed or
  hardware-verified.
- Next action: commit this exact source, flash that clean identity to the identified Waveshare board,
  and retain the bounded boot/metrics artifact bundle.

### E-0005 — Phase 5 physical tuning and first complete 15-minute run

- Date/phase: 2026-08-24; Phase 5 interactive validation on the identified Waveshare
  ESP32-S3-RLCD-4.2.
- Source baseline: dirty working-tree iterations based on `5b73f441`; this is physical design and
  behavior evidence, not a clean committed source identity.
- Physical evidence:
  - Home, launcher, Hourglass entry, physical `KEY` start, and repeated clockwise quarter-turn input
    all worked after correcting the target input lifecycle.
  - The selected cellular backend ran 3,072 visible one-pixel cells at 30 Hz physics and 10 Hz
    rendering. The PBD backend remains available as the compile-time A/B control.
  - Device-guided tuning produced curved collision walls, an effective 1.5-pixel wall-release band,
    varied grain mobility and roll behavior, weighted packets for the 15-minute model, and one-pixel
    throat tracers. The resulting pile, wall release, flow speed, sand amount, and tracer visibility
    were accepted qualitatively on the physical panel.
  - One uninterrupted 15-minute session reached completion. During separate rotation checks, 90- and
    270-degree orientations stopped throat flow and allowed bulk sand to settle; flow resumed after
    rotation. A 180-degree mid-session turn rebased the countdown to the source mass in the newly
    upper bulb rather than continuing the old 15-minute countdown.
  - The display stayed at its higher-contrast ST7305 low-power drive setting during animation; panel
    drive mode is intentionally independent from the runtime CPU-performance intent.
- Corrections to E-0002/E-0003: the target now boots to Home and resolves Hourglass through a real
  launcher; `runtime_ui.rs` replaced the deleted one-app `hourglass_task.rs`. Production uses the
  cellular backend rather than the PBD solver described by the original plan, and the fixed V1
  duration is 15 minutes rather than three minutes. A duration picker is explicitly deferred.
- Evidence class: identified physical-device qualitative validation plus one complete-duration run.
  No retained final `HOURGLASS_METRICS` bundle, clean committed ELF identity, repeat-session series,
  or one-hour soak is claimed.
- Remaining Phase 5 gate: retain bounded metrics from the final source, confirm exactly one completion
  cue in those counters, repeat complete sessions and horizontal holds, run the one-hour soak, rerun
  the full software/documentation gate, and commit the exact tested source.

### E-0006 — Final committed-device gate and plan closure

- Date/phase: 2026-08-24; Phase 5 final gate and closure.
- Source identity: commit `3115773f8c80cd24863757337e50192e9854ca56`, flashed with
  `tracked_dirty=false`; production ELF SHA-256
  `913ca14a2c89e3ddd858c1770ce98c991c49fc49fb1a7731ba86b4ed91dd4381`.
- Identified target: Waveshare ESP32-S3-RLCD-4.2, ESP32-S3 revision v0.2 with 16 MiB flash. The clean
  boot reported the 240 MHz CPU clock, Home surface, GPIO18 KEY, cellular backend, 3,072 visible
  packets, and 900-second duration.
- Physical result:
  - The user entered through Home and the compiled launcher and started Hourglass with physical KEY.
  - The session changed to `Complete` after exactly 27,000 running physics ticks (the cumulative
    metric was 27,054 because 54 Ready ticks elapsed between app entry and start), matching 900
    seconds at 30 Hz.
  - At the completion transition, maxima were `physics_max_us=22063`, `render_max_us=57318`,
    `work_max_us=57967`, and `lateness_max_us=21357`, with `missed_periods=0`. A later post-completion
    sample raised only `lateness_max_us` to 23,368 us and still reported zero missed periods. Physics
    therefore stayed below its 33.3 ms period, and the combined render work stayed below its 100 ms
    cadence.
  - The completion transition emitted exactly one cue: `cue_count=1`, `cue_max_us=13135`. Runtime
    intent returned from `HighPerformance` to `Normal`; the ST7305 remained in its accepted
    high-contrast low-power drive mode.
- Final linked-memory baseline: `.data` 37,612 bytes, `.bss` 125,232 bytes, `.stack` 168,128 bytes,
  and `.rwdata_dummy` 10,784 bytes. As required by the S3 linker model, these consume the generated
  DRAM segment with `.stack` as the measured remainder; 168,128 bytes remain available to the runtime
  stack allocation.
- Software gate:
  - Cellular/default host suite: 76/76 tests passed, including fixed golden fingerprints for upright
    progress and a two-turn rotation trace; strict Clippy passed.
  - Retained PBD A/B control: 69/69 tests passed and strict Clippy passed.
  - Default and `hourglass-pbd` ESP32-S3 release builds linked; strict target Clippy passed for the
    production target, and the Waveshare board library build passed.
  - Formatting, staged Markdown links, source reachability, include usage, script-surface inventory,
    secrets scan, and diff checks passed. LOC output contained advisory-only warnings.
- Accepted closure deviation: the originally proposed one-hour repeated-session soak was not run.
  Earlier iterative hardware work completed one full 15-minute session plus rotation/settling checks;
  E-0006 adds a second full session from a committed binary with retained metrics. The user directed
  finalization on that evidence. Long-duration repetition remains in the hardware test matrix as an
  ongoing regression lane, not unfinished V1 implementation.
- Deferred follow-ons, outside this closed plan: duration picker; a hardware adapter for stable
  arbitrary-angle commands; runtime `Normal`/`HighPerformance` CPU-frequency differentiation; a final
  Sleep/Deep Sleep picker; and retained Deep Sleep display hardware biasing.
- Evidence class: committed-source, host-, cross-toolchain-, linked-memory-, and identified physical
  device-verified. All Hourglass V1 completion criteria are accepted; plan status is `Done`.
