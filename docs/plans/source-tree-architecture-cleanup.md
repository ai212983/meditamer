# Source Tree Architecture Cleanup Plan

- Status: Done
- Last-reviewed: 2026-08-13
- Completed: 2026-08-13
- Owner: Firmware
- Scope: Structural cleanup of first-party modules under `src/`; extraction and redesign use follow-ups
- Predecessor: [Completed source-tree reorganization](../archive/refactors/source-tree-reorganization.md)
- Related decisions: [Stepped FAT engine](../architecture/0004-dma-stepped-fat-engine.md), [UI structure](../architecture/0007-ui-and-application-structure.md), [A/B update foundation](../architecture/0009-ab-firmware-update-foundation.md), [durable UI settings](../architecture/0010-durable-ui-settings.md), [BLE service foundation](../architecture/0011-bounded-ble-service-foundation.md)

## Objective

Make `src/` express stable ownership independently of historical arrival order and Embassy task placement.
Use ownership-specific names while preserving runtime protocols, peripheral ownership, task topology, and product behavior.

A move qualifies when it gives code a clearer existing owner. Behavioral and dependency corrections
become follow-ups, promoted to prerequisites when a concrete move depends on them.

## Proportionality Rule

- A path-only rename needs source checks, affected host tests, and target compilation.
- A task/static/cfg move additionally needs the supported firmware matrix and memory/task comparison.
- A hardware-owner, startup-order, or protocol change needs its focused physical/device gate.
- An unrelated semantic redesign becomes a separately reviewed follow-up.

Prefer focused checks while iterating and one aggregate gate after a coherent batch. Repeated need and
maintenance value justify separate analyzer or hardware-workflow proposals.

## Comparison Baseline

At activation, the `fix/wifi_connectivity` checkout was a mixed staged, unstaged, and untracked tree.
The campaign preserved it with workstream-level administration:

1. Captured the full tree with source HEAD, tree hash, included source, exclusions, toolchain, profiles, and known gate results.
2. Selected workstream cohorts on an accepted upstream commit and recorded their names and patch/tree hashes.
3. Used owner confirmation for semantic cohorts and patch/content equivalence for structural cohorts; finer disposition was reserved for ambiguous overlap.
4. Built comparison artifacts from the clean cohort tree and retained the full snapshot as preservation evidence.

## Preserved Ownership

1. `firmware::system` owns bootstrap/resource/task composition. Domains consume scheduling and
   service-mode policy through neutral firmware services.
2. `platform` contains passive chip/board mechanics and depends on HAL and board-local primitives.
3. One display task owns LVGL, `InkplateHal`, refresh/lease/power, dirty history, frontlight, and
   SD-power servicing. Cross-domain client suspension remains in neutral `panel_bus` coordination.
4. ELAN touch initialization completes before concurrent IMU/display use of the shared I2C bus. The
   existing mutex, resource ownership, initialization order, task priority, and spawn order remain.
5. Wi-Fi/net and BLE/radio coordination remain firmware runtime policy with their current package and
   radio/update authority boundaries.
6. Shared flash, firmware update, PSRAM, app state, and durable UI settings retain their existing
   transaction boundaries.
7. Active self-test, passive observability, and subsystem operational status remain distinct concepts,
   even where current status ownership needs a later correction.
8. Existing serial, upload, diagnostic, telemetry, and update wire protocols remain compatible.
9. Queue capacity, mutex/message type, allocation, task priority, timeout, retry, and failure semantics
   retain baseline behavior. Changes graduate to separate semantic review.

## Target Shape

```text
src/
  lib.rs
  main.rs
  platform/                       renamed existing passive drivers boundary
    gpio_fast.rs
    inkplate/
    platform.rs
  firmware/
    system/                       bootstrap, resources, buses, task composition
    app_state/
    event_engine/
    input/                        GPIO36 wake-button input
    imu/
    touch/
    power/                        battery timing/events
    display/                      foreground task and Inkplate owner
      panel/                      refresh, lease, power, dirty history
      presentation/               LVGL-facing task orchestration and probes
    ui/{shell,screen,overlay,widget,theme,lvgl}
    net/                          stack and Wi-Fi runtime
    ble/                          retained pending accepted radio ownership decision
    storage/                      SD task plus upload HTTP/SD protocol
    serial/                       task, parser, commands and output
    update/
    scheduling.rs                 neutral scheduling/readiness service
    service_mode.rs               neutral cross-domain product policy
    panel_bus.rs                  neutral shared-client suspension
    flash.rs                      current shared firmware flash facade
    psram/                        current allocator/runtime service
    config/                       residual entries awaiting a later ownership audit
    types/                        residual entries awaiting a later ownership audit
    self_test/                    bounded active diagnostics
    observability/               counters, snapshots and log filtering
    assets/                       retained until ownership/provenance is resolved
```

HTTP upload remains storage-owned; a second consumer triggers review of reusable server plumbing.

## Core Cleanup Slices

Each slice has one primary ownership change. Temporary facades name their removal slice. A requirement
for a new queue, callback protocol, static owner, or hardware authority promotes the work to a semantic
follow-up.

### S0 — Capture the Baseline and Affected Boundaries

- Create the comparison snapshot and workstream classification above.
- Inventory boundaries touched by the next slice: path consumers, public re-exports, task/static
  identity, status/channel users, and non-Rust assets that move with it.
- Record the task table once: executor/core, pool count/size, `TaskClass`, priority, and spawn order.
- Record the existing `TaskClass::Diagnostics` collision as a follow-up; path moves preserve current
  registration and spawn order.
- Retain `assets/suminagashi_blue_noise_600.bin` through S5, which assigns a UI owner or records a
  separate deletion decision.
- Reconcile each proposed ADR before the first slice that relies on it.

**Exit:** snapshot/baseline failures, cohort overlap, and ownership transfers are fully classified.

### S1 — Clarify Misleading Names

Land these as separate path-focused changes:

- `runtime/diagnostics` -> `self_test`;
- `telemetry` -> `observability`;
- telemetry diagnostic-mask names -> logging vocabulary, retaining `TELEM`/`TELEMSET` wire names;
- `net/wifi/diag.rs` -> a status-oriented name;
- split HTTP listener gating from debug memory logging when the change stays module-local.

Keep the established `net` owner and its status writer/reader behavior in S1.

**Exit:** self-test, observability, logging, and operational status are visibly different concepts;
task/static identity and external protocol output match the baseline.

### S2 — Establish the Display Boundary

- Move `runtime/display_task` to `display` with the same task, executor, resource owner, initialization
  point, priority, and spawn order.
- Rename task-side LVGL orchestration to `presentation`; retain `ui/lvgl` as the toolkit adapter.
- Put refresh, lease, power, dirty tracking, and SD-power servicing in `display/panel`, a sibling of
  presentation. Shared touch/IMU suspension stays in neutral `panel_bus`.
- Move frontlight timing/policy to `display/frontlight` because it uses display-owned `InkplateHal`.
- Each event/probe grouping forms a real module with its own imports and privacy.
- Update existing `#[path]` harness consumers transactionally while preserving production visibility.

SD-power one-in-flight, timeout-while-display-busy, late-response, queue-drain, retry, and
busy-publication order retain their baseline semantics.

**Exit:** initial render, touch delivery, refresh selection, frontlight, SD power, runtime-ready
publication, task pool/layout, and physical panel behavior match the baseline.

Run `HOSTCTL_PORT=<port> cargo run --locked --manifest-path tools/hostctl/Cargo.toml -- test
ui-lifecycle --cycles 2 --output <log>` on the exact artifact, then confirm touch, full/partial refresh,
and frontlight hold/fade/off; the workflow reports lifecycle, refresh, and write success. A path-only
SD-power move uses existing tests; handler body/order changes graduate to a device protocol gate.

### S3 — Reduce Runtime Through Independent Moves

| Slice | Move | Preserved constraint |
| --- | --- | --- |
| S3a | serial task/parser/commands/output -> `serial` | existing flash/update/panel authority |
| S3b | battery-tick task -> `power` | cadence, event delivery, task identity and spawn order |
| S3c | update module plus `firmware_health_task` -> `update` | flash/transaction/readiness behavior |
| S3d | scheduling and service mode -> neutral firmware modules | current synchronous calls and policy |
| S3e | bootstrap/resources/bus construction/spawning -> `system` | resource and startup order |
| S3f | remove temporary runtime facades | after all consumers move |

Public API, scheduling publication, service mode, global channels, and scheduler identity retain their
baseline contracts throughout S3.

**Exit:** explicit domain modules replace generic `runtime`; `system` contains composition concerns.

### S4 — Rename the Existing Platform Boundary

- Rename passive `drivers` to `platform` while preserving its internal layout.
- Keep `gpio_fast.rs`, `platform.rs`, and `inkplate/` in their current relative ownership.
- Preserve the shared-I2C mutex, ELAN-before-IMU/display initialization, and resource/spawn order.
- Update path-sensitive panel, waveform, linker, IRAM, and host-harness checks transactionally.

ESP32/TEMPERA repartition, fast-GPIO relocation, flash/PSRAM policy, update quieting, core parking, and
app-state persistence continue as architectural follow-ups.

**Exit:** platform remains passive and one-way; target builds and static placement match the baseline.

### S5 — Realize the Existing UI Decision

After reconciling ADR-0007 status, move product screens, overlays, widgets, and theme out of `ui/lvgl`.
Keep toolkit bindings, raw object ownership, input/output adaptation, and dithering in `ui/lvgl`.
Preserve shell ownership, navigation, provider teardown, settings mutation/debounce, and display-task
ownership. Run the existing app-state migration/interruption and UI-shell suites. Assign the retained
blue-noise asset to a real UI owner or record its retained status for a later deletion decision.

**Exit:** product surfaces live in product modules, the shell remains the navigation owner, and shell
lifecycle plus identified-device interaction evidence pass.

### Cross-Slice Rule — Clean Only Touched `types` and `config`

When S1-S5 move a module, relocate the commands, results, constants, aliases, or channels whose
owner becomes unambiguous in that slice. Leave a narrow compatibility facade when needed and schedule
its removal. Remaining `types` and `config/channels` entries form a later ownership audit.

### S6 — Package-Boundary Decision

Retain `packages/sdcard` provisionally. Assess SD and Wi-Fi boundaries against:

1. one lifecycle/hardware owner and one-way dependency;
2. self-contained behavior relative to Meditamer globals and product logging/wire/session policy;
3. a small stable public contract;
4. independent build/test/dependency value;
5. a domain that survives another consumer.

Record whether SD probe/RW session formatting belongs in firmware and whether a pure internal Wi-Fi
policy seam is useful. The current package set and workspace layout stay stable through S6.

**Exit:** an accepted decision records passed/failed gates and implementation follow-ups; S6 produces
documentation evidence.

## Independent Architectural Follow-Ups

The structural cleanup established the target ownership boundaries. Further contract or API changes
remain independently scoped:

- move Wi-Fi/link/IP/listener operational truth out of observability;
- break the `app_state -> scheduling` dependency with a proven synchronous contract;
- finish domain ownership for the remaining global channels and mixed types/constants;
- decide whether shared flash/PSRAM policy needs a new service boundary;
- contract the public crate surface to `meditamer::run()`;
- decide whether the retained, unreferenced `assets/suminagashi_blue_noise_600.bin` has a future UI owner;
- retain the ADR-0012 package boundary until a future consumer triggers reassessment.

The scheduler-identity correction completed with this campaign. The remaining entries change
behavior, API, runtime topology, or ownership contracts and receive their own decisions and tests.

## Validation

Production-code slices run the following; S0/S6 use their applicable source/document subset:

```sh
scripts/ci/check_software_baseline.sh source
git diff --cached --check
INCLUDE_USAGE_ENFORCE=1 scripts/ci/check_include_usage.sh
scripts/ci/check_orphan_modules.sh
RCA_ENFORCE=1 RCA_RATCHET=1 scripts/ci/lint_code_analysis.sh
scripts/ci/check_markdown_loc.sh --staged
scripts/ci/check_markdown_links.sh
```

Focused and aggregate gates:

| Change | During iteration | Batch/final gate |
| --- | --- | --- |
| S1 path/module names | affected host harnesses, `firmware-clippy` | `host` + `firmware` after S1 |
| S2 display/frontlight | UI-shell/touch-replay tests, `firmware-clippy`, panel source guards | `firmware` plus two-cycle UI/device smoke |
| S3a-S3f runtime moves | affected parser/host tests, `firmware-clippy` | task/pool/section comparison; `all` after S3 |
| S4 platform rename | affected harnesses, `firmware-clippy`, source guards | `firmware` + `static`; build/static evidence for path-only diff |
| S5 UI | UI-shell/app-state tests, `firmware-clippy` | `firmware`, two-cycle UI lifecycle, physical interaction |
| Touched contracts | owner-specific host tests and `firmware-clippy` | covered by owning slice's batch gate |
| S6 audit | standalone/root `sdcard` metadata, tests and Clippy | decision review |
| Final | focused gates already green | `scripts/ci/check_software_baseline.sh all` |

Any task/static/cfg/linker move compares executor/core, pool count and `::POOL` size, `TaskClass`,
priority, spawn order, `.data`, `.bss`, `.stack`, `.dram2_uninit`, and application size in affected
profiles. An unexplained delta stops a supposedly structural move.

Run the [Wi-Fi regression gate](../guides/wifi-regression-gate.md) for changes to `net`, radio
coordination, upload-network behavior, or their cfg/resource placement. BLE-specific runtime limits
apply to BLE/radio work according to ADR-0011 status.

Physical evidence follows affected properties. Reuse applies outside the affected hardware boundary
when task/static/section/config ownership retains its baseline form.
For display/UI moves, run the focused smoke on the exact artifact. A compile, host test, or
`RUNTIME_READY` alone is not physical proof; behavioral callback or provider-lifecycle work receives
the long lifecycle gate.

## Completion Record

1. S5 passed its 15/15 SD baseline and two-cycle UI lifecycle revalidation on one release artifact.
2. Final integration passed `scripts/ci/check_software_baseline.sh all`, live-reference validation, enforced include usage, and orphan-module reachability.
3. [Cold-boot validation](cold-boot-validation.md) records 5/5 reset cycles while retaining the true power-rail limitation explicitly.
4. `firmware_health_task` has a distinct `TaskClass::FirmwareHealth` slot. The supported firmware
   matrix, expected `.data` delta, identified-device boot, scheduler-profile cycle, and UI smoke passed.

Final source identity: HEAD `803077a0816b191342f80c7ee1d2edaba6eafb1c`, tree `6e2ed2fc0d727c4a4df423deaaad8bcfd224997f`,
snapshot `10127687409249987778d5828b4341e12435836c` tagged `snapshot/source-tree-cleanup-closed-2026-08-13`,
and `src/` subtree `714b21366370f00dc451727c523780dc5210e5c3`.

The identified UI evidence includes user-confirmed panel/touch behavior. The content-identical frontlight move passed target/device gates; no new subjective hold/fade/off observation is claimed.

## Completion Criteria

1. `runtime` is replaced by explicit display, serial, power, self-test, and system owners.
2. Product UI concepts are outside the LVGL adapter; panel policy is outside presentation.
3. The passive platform boundary depends on HAL and board-local mechanics.
4. Active self-test and passive observability have distinct names and ownership.
5. Every moved task, static, hardware owner, channel, and protocol retains its recorded behavior.
6. Temporary facades introduced by this campaign are removed or have an explicit follow-up.
7. Final host/target/static gates and applicable physical milestones pass against an identified source snapshot/artifact.
8. Archived documents remain frozen; live paths and harnesses are updated transactionally.
