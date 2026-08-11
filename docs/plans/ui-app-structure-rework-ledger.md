# UI and App Structure Rework Implementation Ledger

- Status: Active
- Last-reviewed: 2026-08-10
- Started: 2026-08-09
- Plan: [UI and app structure rework](ui-app-structure-rework-plan.md)
- Decisions: [ADR-0006](../architecture/0006-flash-overlay-app-modules.md),
  [ADR-0007](../architecture/0007-ui-and-application-structure.md),
  [ADR-0008](../architecture/0008-app-catalogue-and-launcher.md)

This is the execution record, not a decision document. Amend Proposed ADRs or create a successor ADR when a durable decision changes; use this ledger for phase state, evidence, deviations, failures, and the next gate.

## Phase status

| Phase | State | Completed | Gate result | Next action |
| --- | --- | --- | --- | --- |
| 0. Decision and execution baseline | Complete | 2026-08-09 | Pass | Begin Phase 1 only. |
| 1. Shell contract and pure state model | Complete | 2026-08-09 | Pass | Phase 1 evidence is E-0002. |
| 2. Static navigation vertical slice | Complete | 2026-08-10 | Pass | Physical and serial evidence is in E-0006. |
| 3. Lazy lifecycle and resource proof | Complete | 2026-08-10 | Pass | Debug/release resource and release physical evidence is E-0006. |
| 4. Composition and input arbitration | Complete | 2026-08-10 | Pass | Static-provider runtime evidence is E-0010. |
| 5. Compiled catalogue and launcher | Ready | — | — | Begin the compiled catalogue and replace the fixed launcher presenter. |
| 6. Durable settings and optional resume | Blocked by Phase 5 | — | — | Do not start. |
| 7. Native-loader spike and decision | Blocked by Phases 1-5 | — | — | Do not start. |
| 8. External catalogue and installation | Conditional on accepted Phase 7 | — | — | Do not start. |

## Baseline observations

| ID | Observation | Evidence | Implication |
| --- | --- | --- | --- |
| B-001 | `home` and `gesture_test` import each other and own navigation callbacks. | `src/firmware/ui/lvgl/home.rs`; `gesture_test.rs` | Phase 2 must delete the sibling ownership, not wrap it. |
| B-002 | Both screen trees are created in `Backend::initialize`. | `src/firmware/ui/lvgl/backend.rs` | Phase 3 needs a measured lazy lifecycle. |
| B-003 | The display task owns LVGL and awaits panel refresh inside its event cycle. | `src/firmware/runtime/display_task/lvgl.rs` | Measure service gaps separately from shell transition time. |
| B-004 | The active path is partial L8 rendering converted to binary dirty regions. | `src/firmware/ui/lvgl/dither.rs` | Preserve actual dirty output and current refresh ownership. |
| B-005 | Durable device lifecycle uses the final flash sector. | `src/firmware/app_state/store.rs` | UI settings need an explicit non-overlapping storage decision. |
| B-006 | The existing release artifact was 1,615,504 bytes in a 4,128,768-byte application region. | ADR-0006 review baseline | Static apps remain viable until remeasurement disproves it. |
| B-007 | Deep-sleep RTC retention needs a custom configuration. | `docs/development/dram-budget-rom-stack.md` | Do not implement RTC resume as ordinary persistence. |

## Recorded decisions

| ID | Date | Decision | Authority |
| --- | --- | --- | --- |
| D-001 | 2026-08-09 | Build the shell and catalogue independently of any native loader. | ADR-0007, ADR-0008 |
| D-002 | 2026-08-09 | Keep statically linked providers as the product baseline. | ADR-0006 |
| D-003 | 2026-08-09 | Require provider id plus generation on live and queued references. | ADR-0007 |
| D-004 | 2026-08-09 | Separate volatile navigation, durable UI settings, device lifecycle, and residency. | ADR-0007, ADR-0008 |
| D-005 | 2026-08-09 | Use stable launcher ordering; do not reorder automatically by recency. | ADR-0008 |
| D-006 | 2026-08-09 | Gate external catalogue and installation on a fully passing loader spike. | ADR-0006 |
| D-007 | 2026-08-09 | Shell-issued nonzero `u32` generations remain remembered after provider removal. | Phase 1 |
| D-008 | 2026-08-09 | LVGL callbacks will enqueue bounded owned intents; Phase 2 drains them after callbacks return. | Phase 1 |
| D-009 | 2026-08-09 | Phase 2 adds a fixed one-entry launcher so diagnostics retains the `SystemRoot` role. | ADR-0007, Phase 2 |
| D-010 | 2026-08-09 | Failed LVGL activation restores the origin before dropping the uncommitted navigation plan. | Phase 2 |
| D-011 | 2026-08-09 | Phase 3 uses `DropOnLeave`; retained model payload capacity remains zero. | Phase 3 |
| D-012 | 2026-08-09 | Two callback-route slots cover origin plus candidate; cleanup failure blocks new transitions until synchronous retry or fail-stop recovery. | Phase 3 |
| D-013 | 2026-08-09 | A failed teardown is a navigation fault, never reclamation evidence; post-delete route-audit failure carries no freed instance. | Phase 3 |
| D-014 | 2026-08-09 | Every node in a passive overlay subtree must be non-clickable; clearing only its root is insufficient in LVGL. | Phase 4 prototype |
| D-015 | 2026-08-10 | Callback actions are FIFO and instance-owned; four routes bound the screen/modal handoff, while route and delete audits latch fail-stop. | Phase 4 base slice |
| D-016 | 2026-08-10 | Overlay request ownership is distinct from definition ownership; base modals preempt provider modals, and provider removal is detach then audited finalize. | ADR-0007, Phase 4 |

## Risks and open questions

| ID | State | Question or risk | Required resolution |
| --- | --- | --- | --- |
| R-001 | Resolved | What fixed capacities fit the recovered DRAM and stack budgets? | E-0002 and `dram-budget.md`; remeasure live pool use in Phase 2. |
| R-002 | Resolved for current base slice | Does synchronous LVGL deletion remove every callback, timer, and user-data edge? | E-0006: exact live-block counts and instance routes survive 100 cycles; current provider-timer set is empty. Reopen when timers are introduced. |
| R-003 | Resolved for base passive slice | Can a passive overlay reliably pass pointer input through LVGL? | E-0005 plus E-0007 identified-artifact physical passthrough; reopen for interactive/provider overlays. |
| R-004 | Open | Where should durable UI settings live without colliding with app state or future overlay flash? | Phase 6 storage ADR or documented local decision. |
| R-005 | Open | Can ESP32 native modules meet all ADR-0006 proof gates? | Phase 7 spike; no assumption of success. |
| R-006 | Resolved for statically linked providers | Does production provider removal preserve exact runtime/shell token alignment and remove every owner reference? | E-0010 proves two exact-generation Backend/LVGL removals. Reopen before any external/native provider can be unloaded. |

## Evidence entries

### E-0001: Phase 0 decision and execution baseline

- Date/scope: 2026-08-09; ADRs, execution plan, ledger, and superseded TODO marker only.
- Decision result: the shell and catalogue are loader-independent; the native loader is feasibility-gated.
- Validation/gate: affected links, Markdown LOC, diff checks, and absolute-path audit passed. Phase 0 passed.

### E-0002: Phase 1 shell contract and pure state model

- Date/scope: 2026-08-09; pure fixed-capacity shell state, content SHA-256 `e63995766a42e15700de88a82a15220e18d6968b01f9f4db2dcea4a0d02d14b1`.
- Contract: shell-issued generations, atomic registry updates, prepared navigation and provider removal,
  validated composition references, and a non-evicting owned-intent queue; no LVGL ownership changed.
- Initial capacities: providers 8, surfaces 16, navigation 8, live overlays 4, queued modals 4, intents 8,
  retained payload 0. Later exact target measurements supersede the initial host estimates.
- Validation/gate: shell 8/8, focused Clippy, debug/Clippy target builds, format, diff, dependency and
  documentation guards passed. Phase 1 passed; unrelated dirty baseline failures were recorded separately.

### E-0003: Phase 2 static navigation vertical slice

- Date: 2026-08-09
- Phase: 2
- Source identity: branch `fix/wifi_connectivity`, HEAD `41df54eb2c3dfa1c05b937be2c9e1a675147148f`;
  dirty-status SHA-256 `74ca4b772eae60014213341ea4dfd954ee02d8641f2e5e5908927e347378e154`;
  Phase 1 and 2 implementation-scope content SHA-256
  `6c7185c15f482ef1f1c1cf2dced4118dd807e6e15e82d0e735f329f3647f897f`.
- Scope: Display-task-owned navigation for the three eager Phase 2 surfaces. Lazy construction,
  synchronous destruction, retained models, overlays, catalogue generation, settings, and native
  loading remain deferred to their gated phases.
- Changes:
  - instantiated the fixed-capacity shell in the LVGL backend and made it the navigation authority;
  - added a minimal fixed launcher, preserving the role topology `Ambient` → `Launcher` →
    diagnostics `SystemRoot` without pre-implementing the Phase 5 catalogue;
  - replaced sibling screen calls and screen-root globals with backend-owned surface roots and
    callback-enqueued owned intents drained only after LVGL input callbacks return;
  - centralized `lv_screen_load` in the backend and retained display-task refresh ownership;
  - made destination activation transactional: failure restores the origin and does not commit the
    prepared shell stack;
  - added an ownership guard and host regressions for topology and failed-entry rollback.
- Validation:
  - shell host harness: 10 passed, including destination-failure origin restoration;
  - touch core, replay, multitouch, dirty-region, refresh-tracking, and panel-lease host suites: pass;
  - source, static-source, UI ownership, formatting, diff, metadata, secret, and stack guards: pass;
  - target matrix: release default; debug minimal, slim, telemetry, and all-features; Clippy minimal
    and all-features: pass;
  - independent ownership/lifecycle re-reviews found no remaining Phase 2 source blocker.
- Target evidence: app-only flash to the CH340 device `usbserial-2110` succeeded with repository workflow
  output under `logs/flash_capture_ui_shell_phase2_final_20260809/`. Release ELF SHA-256 is
  `cba582ac4145a256c513942744d99c96378b0cb176d9ac6c8601cd7ae098cd29`; app image SHA-256 is
  `a43b091727db9f38fb851390889a287869f9c5d359c893f05ff8a673b2f60f07`. Captured boot reports touch
  ready, a 2555 ms full startup refresh, Home entered, and `RUNTIME_READY`, with no panic or watchdog.
- DRAM evidence: the identified release ELF has `display_task::POOL` 3952 bytes, `.data` 13932,
  `.data.wifi` 540, `.bss` 67620, `.stack` 114508, and `.dram2_uninit` 104392. The callback mailbox,
  stable bindings, and overflow flag use 208, 76, and 1 bytes. There is no same-worktree pre-cutover
  artifact, so no isolated Phase 2 delta is claimed.
- Failures or deviations: full-image board probing and flashing timed out without modifying the
  device; the documented app-only fallback succeeded. Broad host-test and host-lint baseline failures
  remain the unrelated dirty event debounce snapshot and `touch_replay` `ROW_BYTES * 0` finding
  recorded in E-0002.
- Gate: Pass on 2026-08-10. E-0006 exercised the identified release artifact for 50 cycles, and the
  user observed the panel sequence with no wrong screen, full refresh, severe ghosting, or touch issue.

### E-0004: Phase 3 provisional lazy lifecycle implementation

- Date/phase: 2026-08-09; Phase 3, provisionally implemented before the Phase 2 physical gate.
- Source: branch `fix/wifi_connectivity`, HEAD `41df54eb2c3dfa1c05b937be2c9e1a675147148f`; dirty/scope SHA-256 `142dd69a5d67c92b17948762c833df20e0270aaa6f52b88da5a610bbddb1e868`/`5d13ac7e64f6d813115df1fa26f2a583904726f78c103274b288594546753e22`.
- Changes: one owned active surface, bounded handoff, generated routes, stale-intent rejection, lazy factories, synchronous delete/audit, cleanup retry, ambient recovery, exact alignment, and telemetry.
- Resource policy: `DropOnLeave`, retained payload zero, no provider timers, two callback routes, and no retained heap arena.
- Validation: shell 23/23, related host 146/146, guards/format/Clippy, release/debug builds, and all-features target Clippy.
- Artifacts: release/debug ELF SHA-256 `63493abc4583102ff017a2187ba785d93b3f87e5d108fe3ef235ab818759d597`/`ac5d1f3aa44d1c51f68b25045d186e2a0be5eb8808b12bcc349293142c009c05`.
- Static capacity: release pool 4128; callback 272/212; `.data` 14036, `.bss` 67876, `.stack` 114156, `.dram2_uninit` 104392; no blocker.
- Deviation: implementation began by explicit direction before Phase 2 physical closure; R-006 remains open.
- Target addendum: E-0006 supplies debug/release cycles, LVGL/stack/timing data, and release physical observation. Phase 3 passes; R-006 remains.
### E-0005: Pinned-LVGL overlay input semantic prototype

- Date/scope: 2026-08-09; host-only prototype SHA-256 `eb39d6f1e205f345d1866fdfb8754e4ec7f85768c08fc9fb7d3d94cc40d83481`; exact LVGL 9.5.4 host harness and warnings-denied Clippy pass.
- Evidence: real pointer Down/Up passes through a recursively passive system subtree, a modal captures, pressed-target deletion suppresses stale release, allocator integrity holds, and the next underlay tap works.
- Gate: semantic prototype only; identified-device evidence is E-0007/E-0008.
### E-0006: Acknowledged UI lifecycle evidence lane

- Date/scope: 2026-08-09 through 2026-08-10; host implementation plus identified release and debug app-only flashes, boot captures, and 50-cycle runs.
- Contract: each command advances Home → Launcher → diagnostics → Home once through the real shell and replies only after panel refresh; an ambiguous timeout stops without retry.
- Evidence gate: zero drift remains the default; the characterized 128 KiB arena may explicitly use 256 bytes while live-block count, heap current use, fragmentation, high-water plateau, route/count, shell/integrity, refresh, panic, and watchdog checks remain fail-closed.
- Deviation/fix: strict characterization exposed bounded allocator settling (maximum used/usable-total spans 176/188 bytes); the first bounded report passed but YAML fell through to failure, so success now terminates explicitly and fresh runs were required.
- Release: ELF `12f9257ce2a799fb0f5dd0f13d6811eb072d5427b436ba05c28f67a4129bb9d9`, app `211d40e6c9762a1dda66a782880d047cc4991177479112d4167ce8e7a4dd5e8a`; final raw/report hashes `7c4971faa562e39e82881b7c32e6a3885d74ecda88e4ae4927a6e7e0c1a18d24`/`20099efdac362465d83fc24502597f41c78dba16e843333acdeadeed7f028a79`; 150/150 checkpoints, transition max 32792 us, stack min 101064, zero violations.
- Debug: ELF `ee9480c7b78d31d550a4aca870e6c38b06c0e60b519dac55725ba420a48e1612`, app `188b9acc57390116af42625e642357beee028c48b0e38233646897069b1d62bf`; final raw/report hashes `eccd5500f97a7eb01f533f914c1e036bb09f925b00c4a3a4b3d98c4856aeb17f`/`351f005d298402f939a858b7ea3d613a997e07ddfec17624fae4241a656668d6`; 150/150 checkpoints, transition max 37432 us, stack min 101304, zero violations.
- Gate: Pass. The user observed the release cycles without visible or touch regression; debug/release evidence has stable surface block counts 178/166/169, high-water plateau, constant global heap current use, and zero lifecycle violations.
### E-0007: Phase 4 bounded composition model and base passive runtime

- Date/scope: 2026-08-10; fixed-capacity shell state plus callback-free base transient and sticky LVGL overlays.
- Changes: unique instances, deterministic ordering, passive coexistence, modal FIFO model, transient/sticky cleanup, exact-owner purge, atomic navigation cleanup, and backend-owned synchronous LVGL deletion/audit.
- Validation/capacity: shell/LVGL 30/30, focused Clippy, ownership/stack guards, release/debug builds; target shell 924, composition 168, display pool 4176 bytes (+48); capacities unchanged. Pure-model release/debug hashes are `74d581d6e77e12fd175edcac1b5b31b72c003484f93a2ae6b645f071f047f91d`/`343f861afdca1162fdd1a47de002f8f8660140bc823968fd49bdbafb88da1a56`.
- Physical addendum: release ELF `a8b518acbb9e0de67bfe8f7cfe8afd556c7bdae1713fb258d859ad334548ca72` received Down/Up at `(330,88)` inside the passive cue `(210..389,42..105)`, committed Home to Launcher, deleted the transient, and reported aligned shell/runtime, intact LVGL, and no cleanup/navigation fault.
- Gate: Base passive slice passes host semantics and identified-device passthrough; E-0008 advances the base modal gate.
### E-0008: Phase 4 base modal runtime

- Date/scope/identity: 2026-08-10; transactional base confirm and modal arbitration; release/debug ELF SHA-256 `276a4f57f002295dbf7fee1aa8f86c81c72373a687155cdff400201dc6ef47f0`/`7a072def62ca2fe9780bdf81d2f49f96ec2f65f968009f2a9082345ff55bf56d`; boot/physical log hashes `0eaceaa3bbdd5ec0e1bdbc00a9a086d9f0ab20c75c142463efb483a526aea1fd`/`f9d80409c1683b2f876e04cf0a80c37aeb12993c9d2b821f13be900c65dc6cb2`.
- Validation/capacity: shell/LVGL 33/33, host/target Clippy, debug/release builds and guards pass; display pool 4448, callback storage 304/548, `.data` 14428, `.bss` 68260, `.stack` 113372, `.dram2_uninit` 104392.
- Device evidence: app-only flash booted `RUNTIME_READY`; user confirmed passthrough, modal outside capture, dismissal, immediate next touch, sticky preservation, and no visible ghosting or unexpected refresh. Serial recorded exact dismissal, aligned Home, integrity true, CPU0 stack minimum 98976, and no cleanup/navigation/composition/lifecycle-audit fault.
- Gate: Base modal slice passes. Phase 4 remains open for R-006 and generic provider overlay promotion.
### E-0009: Provider composition and removal source contract

- Date/scope: 2026-08-10; host-model and target-build evidence only, not flashed. Release/debug ELF SHA-256 `73fb43eb2416e3287c438276024b7355be198488513f179219b3437cc5d9408a`/`6e3aa622c536bd5adae8f16909e9365c4b616861fd19a5825bde66dc22a5a17c`.
- Contract: exact request/definition ownership, protected base-modal preemption and capacity, target-aware queue purge, one exact provider-detach diff, mutation lock, and audited finalize-before-unregister.
- Validation/capacity: shell/LVGL 37/37, host/target Clippy, builds and guards pass; target shell 1008, composition 232, display pool 4592, `.data` 14484, `.bss` 68388, `.stack` 113196, `.dram2_uninit` 104392; no DRAM blocker.
- Gate: Source contract passes. R-006 stays open because target DWARF contains no instantiated provider-removal transaction; a statically linked non-base Backend/LVGL fixture must prove exact runtime and callback-route cleanup before provider release.

### E-0010: Statically linked provider removal runtime proof

- Date/scope: 2026-08-10; non-default `ui-provider-fixture` exercises the shared Backend/LVGL removal
  executor. Relevant-source SHA-256 is `f4272724b8a1ed9315a1cd2a789bc146ed9b4dcd278ef1f3f2ceb6c771ba5e59`.
- Artifact: release/debug ELF SHA-256 `0f71a779eb08843a86b27a05faeec730577936d14f6f7660fb8812afb7492379`/
  `9fd0f68ab97d92f4d4ceda456f64c55b2ab067f7dee993e1e5d473fb6f98454f`; flashed app SHA-256
  `0cd33cf0176e0c3ae676c31a29ca834384ed942aaf4aff90326d8e41994d5f55`.
- Device evidence: `logs/flash_capture_20260810_112424/` booted `RUNTIME_READY`. The transcript and
  marker-only evidence hashes are `33d9aa9a52fc08c96c2bfc615e494a53a8d5f5419eaf1b21e46e48e94f1726d2` and
  `5515ff5b48e8bb37280f207eb77740129ccbd9f135bdd8bd2bb3d45a15f089d2`.
- Result: generations 1 and 2 each staged a provider-requested base modal plus a queued provider modal,
  purged exactly one queued callback action, then finalized with definitions 2, live overlays 1, and
  queued overlays 1. Both returned to aligned Home with integrity true and every fault latch false.
- Resources: final CPU0 stack minimum 98384; LVGL use 9628/9668 bytes with 193 live blocks and 3%
  fragmentation; external heap was constant. Release pool is 4648 and sections are `.data` 14484,
  `.bss` 68468, `.stack` 113116, and `.dram2_uninit` 104392.
- Validation/gate: shell/LVGL 37/37, host and target Clippy, release/debug builds, ownership and stack
  guards pass. User-observed Phase 4 input, modal, sticky, refresh, and ghosting behavior remained correct.
  Phase 4 and R-006 pass for statically linked providers; external/native unload must reopen R-006.

### E-0011: Sticky interactive full-repaint control

- Date/scope: 2026-08-10; source, target-build, full-flash, and bounded boot evidence. Physical
  observation remains pending. Relevant-source SHA-256 is
  `716b5e3b8f281a56582904e50bb099881d32dd73b6a44141d0face3be153fede`.
- Contract: the base sticky overlay is non-modal and interactive only within its widget bounds. Its
  `Clicked` callback emits an exact-source full-repaint intent; the display task consumes that intent,
  skips the ordinary release partial refresh, and runs the existing full-refresh transaction. Upload
  mode rejects the request explicitly and retains ordinary UI refresh behavior.
- Validation: shell/LVGL 37/37, panel lease 5/5, refresh tracking 3/3, minimal debug build, minimal and
  all-features target Clippy, default release build, code-analysis ratchet, panel-waveform placement,
  IRAM/flash-reference, formatting of touched Rust files, and scoped diff checks pass. The release ELF
  SHA-256 is `d394a893a7ba3ecd53f80b448f6753d1d8c7ea089500444e8c41b6b1069c25c5`.
- Resources: callback action storage remains 304 bytes; five callback routes occupy 684 bytes and the
  coalescing full-repaint latch occupies 1 byte. Release sections are `.data` 14556, `.bss` 68484,
  `.stack` 113020, and `.dram2_uninit` 104392. `backend.rs` is 2151 lines, below its 2152-line ratchet.
- Gate: software, full-flash, and bounded boot pass. The archived release ELF matched the recorded
  SHA-256; full flash completed without fallback on ESP32 revision 3.1 with 4 MB flash. Boot capture
  reached touch ready, aligned LVGL lifecycle initialization, `LVGL init=ready`, and
  `RUNTIME_READY app_state=ready display=ready` without panic, watchdog, or fault signatures.
  Physical hit routing, the visible full waveform, post-refresh touch recovery, ghost cleanup, and
  artifact-identified panel behavior remain unverified.
