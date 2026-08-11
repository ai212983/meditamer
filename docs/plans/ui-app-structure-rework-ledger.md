# UI and App Structure Rework Implementation Ledger

- Status: Done
- Last-reviewed: 2026-08-11
- Started: 2026-08-09
- Completed: 2026-08-11
- Plan: [UI and app structure rework](ui-app-structure-rework-plan.md)
- Decisions: [ADR-0006](../architecture/0006-flash-overlay-app-modules.md),
  [ADR-0007](../architecture/0007-ui-and-application-structure.md),
  [ADR-0008](../architecture/0008-app-catalogue-and-launcher.md),
  [ADR-0009](../architecture/0009-ab-firmware-update-foundation.md),
  [ADR-0010](../architecture/0010-durable-ui-settings.md)

This is the execution record, not a decision document. Amend Proposed ADRs or create a successor ADR when a durable decision changes; use this ledger for phase state, evidence, deviations, failures, and the next gate.

## Phase status

| Phase | State | Completed | Gate result | Next action |
| --- | --- | --- | --- | --- |
| 0. Decision and execution baseline | Complete | 2026-08-09 | Pass | Begin Phase 1 only. |
| 1. Shell contract and pure state model | Complete | 2026-08-09 | Pass | Phase 1 evidence is E-0002. |
| 2. Static navigation vertical slice | Complete | 2026-08-10 | Pass | Physical and serial evidence is in E-0006. |
| 3. Lazy lifecycle and resource proof | Complete | 2026-08-10 | Pass | Debug/release resource and release physical evidence is E-0006. |
| 4. Composition and input arbitration | Complete | 2026-08-10 | Pass | Static-provider runtime evidence is E-0010. |
| 5. Compiled catalogue and launcher | Complete | 2026-08-10 | Pass | Physical and serial evidence is in E-0013. |
| 5A. A/B firmware-update foundation | Complete | 2026-08-10 | Pass | Accepted by ADR-0009; implementation and device evidence are E-0014. |
| 5B. Serial recovery and A/B throughput | Complete | 2026-08-10 | Pass | FIFO-safe binary update and faster exact full flash are E-0015. |
| 6. Durable settings and optional resume | Complete | 2026-08-11 | Pass | Implementation, recovery, and identified-panel evidence are E-0016 through E-0018. Optional RTC resume remains absent because its retained-memory prerequisite is not configured. |
| 7. Native-loader spike and decision | Parked by capacity gate | 2026-08-10 | No non-overlapping region | ADR-0009 consumes the remaining application flash; reopen only through a successor capacity decision. |
| 8. External catalogue and installation | Not planned | — | Phase 7 condition unmet | Do not start. |

## Baseline observations

| ID | Observation | Evidence | Implication |
| --- | --- | --- | --- |
| B-001 | `home` and `gesture_test` import each other and own navigation callbacks. | `src/firmware/ui/lvgl/home.rs`; `gesture_test.rs` | Phase 2 must delete the sibling ownership, not wrap it. |
| B-002 | Both screen trees are created in `Backend::initialize`. | `src/firmware/ui/lvgl/backend.rs` | Phase 3 needs a measured lazy lifecycle. |
| B-003 | The display task owns LVGL and awaits panel refresh inside its event cycle. | `src/firmware/runtime/display_task/lvgl.rs` | Measure service gaps separately from shell transition time. |
| B-004 | The active path is partial L8 rendering converted to binary dirty regions. | `src/firmware/ui/lvgl/dither.rs` | Preserve actual dirty output and current refresh ownership. |
| B-005 | Durable device lifecycle uses the final flash sector. | `src/firmware/app_state/store.rs` | Phase 5A must reserve explicit app-state storage and migrate the legacy record before a second slot may cover that sector. |
| B-006 | The existing release artifact was 1,615,504 bytes in a 4,128,768-byte application region. | ADR-0006 review baseline | Static apps remain viable until remeasurement disproves it. |
| B-007 | Deep-sleep RTC retention needs a custom configuration. | `docs/development/dram-budget-rom-stack.md` | Do not implement RTC resume as ordinary persistence. |
| B-008 | The accepted Phase 5A release application is 1,831,392 bytes. The no-factory 4 MiB layout provides two 2,031,616-byte (`0x1f0000`) slots, leaving 200,224 bytes or 10.9% current-image growth per slot and enforcing a 131,072-byte minimum reserve. | E-0014; ADR-0009 | A/B is accepted and consumes the practical flash region assumed by the native-loader proposal. |
| B-009 | The app-only recovery workflow writes the application at fixed offset `0x10000`. | `tools/hostctl/src/workflows/flash_capture/flash.rs` | Host flashing must become partition-aware before any layout cutover. |
| B-010 | A 254-byte binary frame corrupts intermittently at 115200, 230400, and 460800 because it exceeds UART0's 128-byte receive FIFO; a 126-byte frame completed 16,404 consecutive frames at 460800 without retry. | E-0015 | Bound binary frame size by the hardware FIFO before tuning baud. |

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
| D-017 | 2026-08-10 | Insert a bounded A/B firmware-update decision after the compiled catalogue and before durable settings; do not couple its flash/boot ownership to the launcher or assume network transport. | User direction; E-0012 |
| D-018 | 2026-08-10 | Use one fixed-capacity compiled catalogue for launcher, ambient-picker, and overlay-settings views; keep picker/settings selection non-durable until Phase 6 and preserve the base ambient fallback independently of loader work. | ADR-0008; Phase 5 |
| D-019 | 2026-08-10 | Adopt the signed no-factory A/B map, pinned rollback bootloader, alternating app-state sectors, base-owned update transaction, and software-health confirmation boundary; park native modules because no non-overlapping region remains above the firmware capacity floor. | ADR-0009; E-0014 |
| D-020 | 2026-08-10 | Use stub-assisted 460800 exact full flash with an exact ROM-only fallback; use negotiated 112-byte CRC-framed binary A/B payloads at 460800 while retaining Phase 5A hex compatibility. | Phase 5B; E-0015 |
| D-021 | 2026-08-10 | Store device lifecycle and durable UI settings in separate fields and APIs inside one version-5 alternating-sector transaction; bound settings lists by the compiled catalogue, debounce/coalesce writes, and keep navigation resume absent until RTC retention is explicit. | ADR-0010; E-0016 |

## Risks and open questions

| ID | State | Question or risk | Required resolution |
| --- | --- | --- | --- |
| R-001 | Resolved | What fixed capacities fit the recovered DRAM and stack budgets? | E-0002 and `dram-budget.md`; remeasure live pool use in Phase 2. |
| R-002 | Resolved for current base slice | Does synchronous LVGL deletion remove every callback, timer, and user-data edge? | E-0006: exact live-block counts and instance routes survive 100 cycles; current provider-timer set is empty. Reopen when timers are introduced. |
| R-003 | Resolved for base passive slice | Can a passive overlay reliably pass pointer input through LVGL? | E-0005 plus E-0007 identified-artifact physical passthrough; reopen for interactive/provider overlays. |
| R-004 | Resolved | Where should durable UI settings live without colliding with app state, OTA metadata, firmware slots, or future overlay flash? | ADR-0010 assigns non-overlapping fields inside the atomic `app_state` envelope; the partition map does not change. |
| R-005 | Parked | Can ESP32 native modules meet all ADR-0006 proof gates after the firmware layout is fixed? | ADR-0009 leaves no non-overlapping region without violating the firmware capacity floor. Reopen only through a successor capacity decision. |
| R-006 | Resolved for statically linked providers | Does production provider removal preserve exact runtime/shell token alignment and remove every owner reference? | E-0010 proves two exact-generation Backend/LVGL removals. Reopen before any external/native provider can be unloaded. |
| R-007 | Resolved | Can the exact shipped bootloader and no-std application provide power-safe A/B selection, health confirmation, and automatic rollback on the 4 MiB ESP32? | E-0014 pins the artifacts and proves inactive-slot staging, normal confirmation, interruption safety, and automatic rollback. |
| R-008 | Resolved | Do the Phase 5 catalogue launcher and the new picker/settings presenters render legibly and preserve touch navigation on the identified panel? | The user confirmed the exact E-0013 artifact works across all requested presenter and touch paths. |

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

- Date/scope: 2026-08-10; source, target-build, full-flash, bounded boot evidence, and subsequent user
  physical confirmation. Relevant-source SHA-256 is
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
- Physical addendum: the user subsequently reported that the flashed control works on the panel. This
  closes the requested manual functional check; no separate serial transcript or quantitative physical
  timing or image-quality measurement was supplied.

### E-0012: A/B firmware-update feasibility and plan insertion

- Date/scope: 2026-08-10; read-only inspection and documentation amendment. No partition table,
  bootloader, updater, flash workflow, or firmware source changed.
- Current artifact: `target/xtensa-esp32-none-elf/release/meditamer` SHA-256
  `dd2eaa00f6ea84a563c137ef9e2275d130424674264a7ee30126f57eeeb85a27`; `espflash save-image
  --chip esp32 --flash-size 4mb` produced a 1,685,632-byte application image with SHA-256
  `4e04bc6d938c060f657d8343e606c51f0f607e0131a8f8ebdcc08c9819de607e`.
- Candidate capacity: after bootloader/table metadata, NVS, two-sector OTA data, PHY data, and an
  explicit app-state sector, aligned slots at `0x20000` and `0x210000` can each use `0x1f0000`
  bytes and end at `0x400000`. Each slot leaves 345,984 bytes, or 20.5% growth over the measured
  application. A third factory image does not fit; this map is evidence for a spike, not an accepted
  partition decision.
- Existing conflicts: `AppStateStore` owns the final flash sector, which the candidate second slot
  would cover; the app-only host recovery command writes fixed offset `0x10000`; and the exact current
  bootloader has not proven configured automatic rollback. These are Phase 5A blockers, not follow-up
  hardening.
- Plan result: Phase 5 remains the next implementation step. Phase 5A now gates Phase 6 storage and
  forces Phase 7 to re-establish a non-overlapping flash budget before any native-loader code. Passing
  Phase 5A requires a dedicated ADR; this amendment does not accept A/B or a production transport.

### E-0013: Phase 5 compiled catalogue and presenters

- Date/scope: 2026-08-10; fixed-capacity compiled catalogue, catalogue-backed launcher, ambient-picker
  presenter, overlay-settings presenter, backend navigation bindings, shell-ownership guard, resource
  ledger, release full flash, boot capture, and two acknowledged lifecycle cycles. No SD discovery,
  native installation, durable settings, recency ordering, or A/B firmware behavior was added.
- Contract: capacity is eight entries and eight entries per derived view. One namespaced stable id is
  shared across capability-filtered views; source presence, residency, compatibility, and health remain
  independent axes. Ordering is pin, default rank, then id. Registration rejects duplicates and
  exhaustion atomically, and construction requires a built-in ready ambient fallback. Surface-entry
  failure is retained as a current-boot health fault rather than reordering or removing the entry.
- Presentation/runtime: the launcher derives its actions from the compiled catalogue. Ambient-picker
  and overlay-settings screens render the same catalogue through capability filters and show health
  badges. Their rows are intentionally read-only in this phase: durable selection, enablement, and
  startup composition belong to Phase 6 after the flash-layout decision.
- Validation: catalogue model 5/5 and complete shell/LVGL host suite 42/42 pass. Strict target Clippy
  passes for default and all features. Release default plus debug minimal, slim, telemetry, and
  all-features builds pass. Source, static-source, ownership, orphan-module, hand-written-include,
  stack, linker, panel, IRAM, and enforced code-analysis ratchet checks pass. The broad quality lane's
  rust-analyzer summary retained its pre-existing non-blocking diagnostic exit while the lane continued
  through a clean zero-offender code-analysis result.
- Resources: the release `DefaultCatalogue`, derived view, `IntentBindings`, callback route table,
  callback queue, and display pool are 328, 324, 192, 1,004, 304, and 5,024 bytes. Release sections are
  `.data` 14,972, `.bss` 68,844, `.stack` 112,244, and `.dram2_uninit` 104,392. Relative to E-0011,
  `.data + .bss` grows by 776 bytes and `.stack` shrinks by the same amount. The 1,695,024-byte app
  image is 9,392 bytes larger than the E-0012 pre-catalogue image.
- Artifact/device evidence: release ELF SHA-256 is
  `3162ed668027355ad46741123b0a54d4c9b6572482cc8867e21bc9870d449b4d`; app image SHA-256 is
  `53070f21a733166945a3f1cd281dde9d912b7ef7c5da501122a0860ca6822250`. Canonical full flash without
  fallback on `usbserial-2110` preserved the identified artifacts in
  `logs/flash_capture_20260810_153651/` and reached `RUNTIME_READY`. An initial touch bootstrap
  arbitration loss recovered to `touch: ready`; no panic, watchdog, shell-alignment, allocator,
  navigation, composition, or lifecycle-audit fault was observed.
- Lifecycle evidence: two Home → catalogue launcher → diagnostics → Home cycles passed all six
  acknowledged checkpoints. Raw/report SHA-256 values are
  `76068781cc6a7b772edbb9d8bbd2053f8f11d533795695814916d0da042dcdb5` and
  `662518c731b9a805dcd9117e888f4db7db55c8fe9a247d9677e8dab036ea1aa1`; maximum transition was
  37,538 us, minimum CPU0 stack headroom was 95,144 bytes, and violations were zero.
- Physical addendum: the user confirmed that the catalogue launcher, ambient-picker, overlay-settings,
  touch navigation, and return paths all work on the flashed E-0013 artifact.
- Gate: Pass. Software, resource, full-flash, boot, identified-artifact lifecycle, and physical
  panel/touch evidence are complete. Phase 5 and R-008 close; Phase 5A is the next implementation step.

### E-0014: Phase 5A signed A/B firmware-update foundation

- Date/scope: 2026-08-10; exact flash preservation, pinned bootloader and partition build, lifecycle
  migration, partition-aware recovery, signed inactive-slot staging, normal candidate confirmation,
  withheld-confirmation rollback fixture, device resets, host tests, target release build, resource
  measurement, and ADR-0009. Production network transport and update UI remain out of scope.
- Device/toolchain identity: ESP32 revision 3.1 with 4 MiB flash on `usbserial-2110`, MAC
  `e8:6b:ea:fb:d5:54`; ESP-IDF v5.5.2 source commit
  `30aaf64524299d3bde422ca9a2848090d1bc5d0f`; rollback bootloader SHA-256
  `bd990a7a8870d200bc038f36e0f5af62ac6307b453da8cc4aded44cf7f28f04f`; partition-table SHA-256
  `6cf01bb10d56434cca2a28e6d66b3d23a0146c6ce25ac3ab45e077e05658aeea`.
- Preservation/migration: before cutover, the complete 4,194,304-byte flash image was archived at
  `target/phase5a-preserve/pre-cutover-flash.bin`, SHA-256
  `df263f1207e9ae930ed42429886aed3bd36cabce7a98bd59328a9e23e9c9a295`; the legacy final-sector
  image SHA-256 is `65fc2cd68b776e971e223eefc9985e9dfdd6929b1af2cdaa373220c528bcdb7c`.
  The identified cutover in `logs/phase5a_a2_cutover_20260810/` migrated its version-2 record to
  version 4 safe defaults at `0x12000`, booted valid `ota_0`, and reached `RUNTIME_READY`. Repeated
  migration reads the new alternating-sector record and does not rewrite the legacy source.
- Accepted transaction: the service derives the inactive label, accepts 48-byte FIFO-safe wire
  chunks, coalesces 240-byte internal-RAM flash writes split at 256-byte program-page boundaries,
  erases only the inactive slot, validates the ESP image, verifies the Ed25519 domain/length/digest
  signature and full-flash read-back digest, and only then writes alternating OTA metadata. The
  cooperative shared panel-bus-client suspension and one-time core-1 transaction park bound
  cache-disabled ownership.
  Only a timeout may resend the immediately previous identical chunk, which firmware accepts
  idempotently; explicit errors and activation acknowledgements are never retried.
- Normal update proof: `logs/phase5a_a9_to_b9.log`, SHA-256
  `581d72974535746d402f440c03f3546724247a35f2383dcece462f2dd399b617`, stages the 1,831,936-byte
  `phase5a-b9` image from valid `ota_0` into `ota_1`, verifies SHA-256
  `6ca21ae037946696c7de503613798e3b6dfa498ae0363b00e965a12c2a68bc1d`, boots it as
  `pending_verify`, reaches `RUNTIME_READY`, and confirms `ota_1` valid. Maximum observed sector
  erase/write calls were 76,782/711 us; full read-back was 991,034 us; the serial transaction took
  799,007 ms.
- Interruption and rollback proof: interrupted staging logs include
  `logs/phase5a_a4_to_b4.log` and `logs/phase5a_a6_to_b6.log`; neither reaches activation and the valid
  running slot remains selected on the next boot. Before activation, erase, write, and verification
  touch only the inactive slot and cannot change boot selection; activation writes one whole older
  OTA-data sector while retaining the other valid sector. `logs/phase5a_b9_to_rollback_fixture.log`,
  SHA-256 `77c305950a2da34d2604b4023eeef2fc117b041e19ff3967939b9e848a8fb67f`, stages and boots a signed
  `ota_0` fixture that deliberately withholds confirmation. The reset capture
  `logs/phase5a_rollback_reset.log`, SHA-256
  `2f5c724a817f8311c8519f075e6e8de675a31199c2e7a2eb9c20cd7c8f92476e`, marks that candidate
  `aborted`, loads prior valid `phase5a-b9` from `ota_1`, reaches `RUNTIME_READY`, and accepts
  `REPAINT`. The device is left on this valid rolled-back B9 image.
- Final capacity/resources: the production confirmation-enabled release ELF SHA-256 is
  `9a01fd65312f6d220c8599dd70e5842757cbedebb67c667ad3a0610e5c6d41f6`; its application image is
  1,831,392 bytes with SHA-256 `dc903bbd5fceb6ea47823a9da5bb24f6b105a1a5e0ef1176703b6c3ce4480dd5`.
  A `0x1f0000` slot retains 200,224 bytes (10.9%), above the enforced `0x20000` reserve. Release
  sections are `.data` 15,852, `.bss` 68,884, `.stack` 111,324, and `.dram2_uninit` 104,392; the
  Wi-Fi data subsection is 540 bytes. The update session holds one 240-byte internal-RAM batch plus
  fixed hash/signature/metadata state and allocates no image-sized RAM buffer.
- Gate: Pass. ADR-0009 is accepted, R-007 closes, Phase 6 becomes the next implementation phase, and
  the lack of a non-overlapping native-module region parks Phase 7. `RUNTIME_READY` and `REPAINT` are
  device/software evidence; they are not a new physical panel or touch-quality observation.

### E-0015: Phase 5B serial recovery and A/B throughput

- Date/scope: 2026-08-10; exact full-flash command/fallback policy, negotiated binary framing,
  pre-erase, FIFO and flash-batch sizing, bounded retry recovery, host tests, strict target build, and
  identified-device timing trials. Compression, network transport, and update UI remain out of scope.
- Full flash: the primary path writes the pinned bootloader, partition table, initial OTA data, and
  application with the ESP-IDF stub at 460800. The conservative fallback repeats that complete set at
  115200 with `--no-stub`; automatic fallback never writes only `ota_0`. In
  `logs/flash_capture_phase5b_fast_full_20260810/`, the 1,838,096-byte application compressed to
  1,106,512 bytes and wrote in 27.8 seconds at 529.1 kbit/s with hash verification and no fallback,
  versus 121.9 seconds for the Phase 5A application payload.
- Protocol: new firmware advertises `bin1@460800`; old firmware without that capability keeps the
  48-byte hex path. Binary frames carry a version, kind, sequential offset, length, 112-byte maximum
  payload, and CRC32 in at most 126 bytes. Two payloads coalesce into a 224-byte aligned internal-RAM
  flash batch. The receiver pre-erases only the aligned image range before streaming, and the host
  restores 115200 before finish/status/activation commands. CRC or timeout retries are identical and
  bounded; failure restores host baud and resets without changing OTA selection.
- Falsification: 240-byte payloads produced 254-byte frames and recurrent CRC failures at 460800,
  230400, and 115200; bounded recovery reset before activation in every failed run. Evidence includes
  `logs/firmware_update_phase5b_binary_20260810.log` and
  `logs/firmware_update_phase5b_230_retry_20260810.log`. Reducing only the frame to the FIFO-safe size
  removed all CRC failures. A 115200 control completed the full signed cycle in 384,301 ms, and a
  230400 trial completed in 264,499 ms, both without retry. Their logs are
  `logs/firmware_update_phase5b_fifo_20260810.log` and
  `logs/firmware_update_phase5b_230_fifo_20260810.log`.
- Protocol-race falsification: direct SD retry, PSRAM high-water, and resumed-multitouch diagnostics
  could splice into line acknowledgements; verified images were never activated when the host saw an
  ambiguous digest or status. The final transport quiet gate covers those observed owners, keeps the
  lease through the digest acknowledgement, queues client resume without waiting on sensor recovery,
  and reopens diagnostics only after the verified status response. A synthetic `SDPROBE` immediately
  before preflight also prevented `PONG` until reset; no update transaction began, and the accepted
  cycle therefore does not treat that separate SD-control fault as firmware-update evidence.
- Accepted update: `logs/firmware_update_phase5b_460_final_o_20260810.log`, SHA-256
  `caa2f6319abbc00429ae72ee8b5916417efc593470c66e0ee05dd2670984d568`, stages the 1,837,200-byte
  `phase5b-460-o` image with SHA-256
  `a707be0502af6d6040238df10623ae9c233964ce09a6d176f902b753bec2b1e6` from valid `ota_0` into
  `ota_1`. All 16,404 frames completed at 460800 without retry; signature and full read-back passed,
  the digest and verified-status acknowledgements were intact, and `ota_1` booted pending-verify,
  reached `RUNTIME_READY`, and was confirmed valid. Maximum erase/write calls were 69,614/653 us,
  read-back was 993,969 us, and the transaction took 196,084 ms: 4.07 times faster than E-0014's
  799,007 ms and 1.35 times faster than the 230400 trial. The exact recovery install in
  `logs/flash_capture_phase5b_final_o_20260810/` also used full 460800 flash with no fallback.
- Final capacity/resources: the accepted image leaves 194,416 bytes in each `0x1f0000` slot, above
  the `0x20000` floor. Release sections are `.data` 15,812, `.bss` 69,020, `.stack` 111,236, and
  `.dram2_uninit` 104,392. The additional task state is one 126-byte frame reader; the update session
  retains one 224-byte internal-RAM flash batch and no image-sized RAM allocation.
- Gate: Pass. The device is left on confirmed-valid `phase5b-460-o` in `ota_1`. Full flash is 4.4
  times faster for the measured app payload and signed A/B update is 4.07 times faster end to end.
  These are host/serial/software results, not a new physical panel or touch observation. Phase 6
  remains next.

### E-0016: Phase 6 durable UI settings

- Date/scope: 2026-08-10; ADR-0010, version-5 UI-settings schema, version-4 migration, alternating
  copy-forward storage, catalogue sanitization, bounded write controller, picker/settings actions,
  startup resolution, DRAM measurement, strict target builds, signed updates, and identified-device
  boot evidence. RTC navigation resume remains deliberately absent because retained RTC memory is not
  configured. Pins and startup fields are stored and resolved, but product editing UI for those fields
  is not added in this phase.
- Storage contract: one 128-byte generation/CRC32 envelope begins each of the two existing 4 KiB
  `app_state` sectors. Lifecycle bytes and UI-settings bytes do not overlap, while every write
  copy-forwards both owners before erasing the older sector. The schema holds optional ambient and
  startup entry ids plus fixed-capacity-eight pin, enabled-overlay, and startup-overlay lists. Version
  4 migrates to generation plus one in the opposite sector; version 2/3 direct-upgrade support remains.
  The partition CSV, OTA metadata, and both `0x1f0000` firmware slots are unchanged.
- Resolution and wear: boot de-duplicates ids and requires the requested compiled-catalogue
  capability plus ready health/compatibility/residency. Unknown or unavailable ambient ids fall back
  to the built-in ambient entry; invalid startup entries and overlays are omitted. The shell creates
  Home before applying valid startup composition and never installs a provider. UI mutations debounce
  for 1.5 seconds, coalesce, admit at most one write attempt per five seconds, and back off 30 seconds
  after failure. Loss before a deferred write retains the previous committed generation.
- UI/runtime: ambient and overlay rows now emit exact-instance settings intents through the bounded
  callback queue. The ambient choice returns Home. The compiled refresh-control overlay can be
  disabled or enabled only after its shell/LVGL lifecycle transaction succeeds, then returns to the
  launcher and schedules persistence. Volatile navigation, modal, instance, and provider-generation
  state enters neither the record nor `AppStateSnapshot`.
- Validation: the shell host suite passes 47/47, including unknown/unavailable/duplicate fallback,
  compiled defaults, write debounce, coalescing, and failure backoff. Minimal and all-feature strict
  target Clippy, default release, format, UI ownership, orphan-module, hand-written-include, stack,
  code-analysis, and diff checks pass. The final default release uses display-task pool 5,392 and
  sections `.data` 15,804, `.bss` 69,420, `.stack` 110,836, and `.dram2_uninit` 104,392. A resource
  check caught and removed a temporary 1,976-byte async-pool inflation caused by carrying the active
  surface label across an await boundary.
- Migration evidence: the first signed Phase 6 update staged the 1,855,376-byte image SHA-256
  `05772555ac62e77d34fef1de5e7ef956823b23550fccd0714cc5914e63ddf93e` from valid
  `phase5b-460-o` in `ota_1` to `ota_0`. The device preserved version 4 into version 5 at `0x13000`,
  booted `phase6-settings-a` pending, reached aligned Home and `RUNTIME_READY`, then confirmed valid.
  The log SHA-256 is `734f2fc11025571e7a18fc7f22a3cb5e59d252db0c591fc6abc25370d0bae6c7`.
- Final exact artifact: release ELF SHA-256 is
  `b06a7320f0ff5ecb6bfe7ae4876f17ba030939a4a0d0b91fb43aa11e30ceb9c5`; the 1,855,360-byte app
  image SHA-256 is `7f15d5560a862686e856d03210effdd171bf299039729fe0d3f5268b1603ad73`, leaving 176,256 bytes above
  the used image and 45,184 bytes beyond the required `0x20000` slot reserve. It staged from valid
  `phase6-settings-a` in `ota_0` to `ota_1` with no stream retry, verified the full read-back, booted
  pending, logged `UI_SETTINGS_BOOT base=home`, reached `RUNTIME_READY`, and confirmed valid. The update
  log SHA-256 is `ed28aca0e40e8b40162ddedc87a13a446dadf41efa58459e26bf5d9e0a520be2`.
- Deviation/recovery: the host did not receive `FWACTIVATE` and correctly stopped without retrying the
  ambiguous activation. The captured boot nevertheless shows the final image in `ota_1` reaching its
  health gate and `FIRMWARE_CONFIRM`; a separate status capture, SHA-256
  `c3c3a01f5832c25de6bd89a776f669e1ca9d3fdee34a1ddd1e3c5c49b13e9db1`, confirms build
  `phase6-settings-final`, booted/selected `ota_1`, state `valid`, phase `idle`.
- Gate: Software, schema migration, exact-artifact update, cold startup, and serial health pass. The
  device is left on valid `phase6-settings-final` in `ota_1` with compiled-default settings. Physical
  ambient selection, overlay disable/enable, deferred-save persistence, and panel legibility remain
  the final Phase 6 gate; serial readiness is not physical panel or touch evidence. Deep-sleep resume
  is not claimed because the optional retained-memory prerequisite is absent.

### E-0017: Phase 6 storage recovery host harness

- Date/scope: 2026-08-10; host-executable recovery evidence for the production version-5 app-state
  transaction. No record field, sector address, partition, firmware-update protocol, UI behavior, or
  device flash changed.
- Seam: the store algorithm now receives a private fixed-capacity storage interface. The production
  adapter remains a zero-sized owner that delegates to the same `flash::read` and `flash::replace`
  primitives. The host adapter owns two independent 4 KiB sectors plus the 32-byte legacy source and
  can fail one replacement or verification read deterministically; it adds no target static or heap
  allocation.
- Recovery evidence: zero-byte-after-erase and 37-byte partial replacements retain the prior valid
  generation; a CRC-corrupt newest record falls back to its predecessor; corrupt-write and ambiguous
  verification-read failures leave at least one recoverable generation; version-4 migration targets
  the opposite sector and retries after a failed first write; and lifecycle/UI writes each copy
  forward the other owner's fields. Existing generation-wrap, version, CRC, count, unsupported-flag,
  reserved-byte, legacy-version-2, settings-codec, catalogue-resolution, and write-rate tests also run
  through the same host crate.
- Validation: the dedicated harness passes 20/20, including 10 store tests. Strict host-tool Clippy,
  minimal and all-feature Xtensa Clippy, minimal debug and default release builds, full formatting,
  include-usage, tracked-module reachability, code-analysis, and diff checks pass. The release sections
  are `.data` 15,764, `.bss` 69,436, `.stack` 110,868, and `.dram2_uninit` 104,392; total linked
  `.data + .bss` is 24 bytes smaller than E-0016 and the stack remainder is 24 bytes larger. Store
  recovery tests live in a real test-only module, leaving the production store at 542 raw lines and
  below the 600-line advisory.
- Gate: host recovery hardening passes. This evidence does not simulate ESP32 electrical flash
  behavior, a physical power cut, panel interaction, or cold boot. The existing Phase 6 physical gate
  and the Phase 7 capacity park remain unchanged.

### E-0018: Phase 6 identified-panel persistence acceptance

- Date/scope: 2026-08-11; physical acceptance of the exact Phase 6 firmware already installed on the
  identified device. Preflight `FWSTATUS` reported build `phase6-settings-final`, booted and selected
  `ota_1`, state `valid`, phase `idle`, and binary update capability `bin1@460800`. No firmware was
  built, flashed, or activated during this gate.
- Ambient and touch path: the user opened Launcher, selected the only compiled ambient entry,
  `Meditamer home`, returned Home, and confirmed the screen remained legible and touch navigation
  remained operable. Because only one ambient entry exists, this exercises selection and return but
  does not claim a changed ambient binding survived reboot.
- Disable persistence: the user opened Overlay settings, disabled Refresh control, waited at least
  five seconds for the 1.5-second debounce and five-second write-admission window, then disconnected
  USB power for three seconds. After reconnect and cold boot, Refresh control remained absent; Home
  was legible and touch still opened Launcher.
- Re-enable persistence: the user reopened Overlay settings, confirmed Refresh control was disabled,
  enabled it, confirmed the sticky control reappeared and touch navigation worked, and waited at least
  five seconds. After a second three-second USB power disconnect and cold boot, Refresh control was
  present again; Home remained legible and touch navigation remained operable. The device is left in
  the normal enabled state.
- Evidence boundary: `logs/phase6_physical_gate_20260811/preflight-status.log` retains the exact
  build/slot status. The first interaction capture was garbled and the later high-rate touch telemetry
  truncated the deferred-save lines, so neither capture is accepted as save-completion evidence. The
  two user-observed cold boots are the persistence proof; host recovery behavior remains separately
  covered by E-0017.
- Gate: Pass. Phase 6 is complete for durable settings, cold-boot persistence, panel legibility, and
  touch operation. Optional RTC navigation resume remains deliberately absent and unclaimed because
  retained RTC memory is not configured. Phase 7 remains parked by ADR-0009's capacity gate, and
  Phase 8 remains unplanned.
