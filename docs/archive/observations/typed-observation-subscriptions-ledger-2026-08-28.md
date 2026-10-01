# Typed Observation Subscriptions Implementation Ledger

- Status: Active — subscription fault/recovery and coexistence checks remain open; general resource qualification deferred (E-0051).
- Last-reviewed: 2026-08-28
- Started: 2026-08-27
- Plan: [Typed observation subscriptions](typed-observation-subscriptions.md)
- Decision: [ADR-0018](../architecture/0018-typed-observation-subscriptions.md), [ADR-0019](../architecture/0019-provider-owned-periodic-demand-overrides.md)
- Current implementation: E-0050 Medinote active-fixture restoration and E-0049 sleep/rejection passed; E-0048 Inkplate diagnostics, E-0047 normal panel recovery; working tree based on `27b091b9`, including untracked sources/tests.

## Reading this ledger

The plan owns scope and acceptance; this ledger owns results and their limits. E-0029 compacts the
historical narratives at the user's request, preserving evidence IDs, material results, and provenance.
The complete pre-compaction documents are retained in
`logs/typed-observation-subscriptions_e0029_20260827/{plan,ledger}-before-compaction.md` with hashes.
These local artifacts are ignored by Git, like the existing device captures. Historical completion
claims are superseded by the current phase table and review below. Append new evidence with a new ID.

A phase is Passed only when its complete exit criteria have evidence; focused tests, injected faults,
and normal-device readings qualify only their recorded paths.

## Phase status and next action

| Phase | State | Evidence | Remaining work |
| --- | --- | --- | --- |
| 0. Bounded storage/memory checks | In progress | E-0036–E-0050; scope E-0051 | Storage measured; retain capacities/floor and change-specific checks. General runtime resource qualification deferred. |
| 1. Observation core | In progress | E-0032/E-0039/E-0040/E-0044 | Full projections implemented; complete contract coverage; predictive lead time deferred. |
| 2. Runtime/control | In progress | E-0030/E-0039–E-0047/E-0049/E-0050 | Normal Inkplate panel cycles and Medinote rejection/restoration passed; next Inkplate rejected-panel restoration, then physical/in-flight faults. |
| 3. Medinote | In progress | E-0031–E-0033/E-0040/E-0045/E-0046/E-0049/E-0050 | Deep timer wake, both cleanup rejections and active-lease restoration passed; startup timeout, light sleep, physical faults and UI responsiveness remain. |
| 4. Inkplate environment | In progress | E-0030–E-0032/E-0039/E-0044/E-0047 | Fresh fixture, periodic delivery and normal panel recovery passed; startup I2C incidents, batching, panel faults and upload coexistence remain. |
| 5. Inkplate battery | In progress | E-0041–E-0043/E-0047 | Fresh fixture, periodic trace and normal panel recovery passed; rejected-panel recovery, physical expander faults and coexistence remain. Inkplate sleep is outside scope. |
| 6. Both-target subscription validation | Needs revalidation | E-0043–E-0050; scope E-0051 | Complete subscription fault/contention/responsiveness evidence and applicable checks; broad resource qualification deferred, baseline issues reported separately. |

Control publication is synchronous; each acknowledged suspend/resume has a 2-second wait budget.
Five sequential clients can consume 10 seconds per phase, subject to executor scheduling. E-0030
adds a separate 2-second shutdown budget and retained recovery after timeout. These are not a
2-second whole-panel deadline. No phase advanced to Passed.

Next: extend the existing Inkplate panel-cycle fixture to test one-request cleanup rejection and
active-override closure/live-demand restoration for each observation provider (E-0051). Earlier
"Next" paragraphs are historical; E-0051 supersedes the connected-BLE resource-qualification step.

## Historical evidence, E-0001–E-0023

| ID | Result retained after later corrections |
| --- | --- |
| E-0001 | Introduced target-owned capacity/deadline design; concrete numeric limits and baseline measurements remained incomplete. |
| E-0002 | Added typed core and generic provider loop; target authority/admission and production interleaving were not completed. |
| E-0003 | Extracted Medinote SHTC3 provider/Home adapter and recorded readings; later review reopened lifecycle/cleanup requirements. |
| E-0004 | Extracted yielding Medinote ADC provider and recorded readings; timeout/cancellation recovery remains open. |
| E-0005 | Extracted Inkplate BME688 provider while preserving pre-touch initialization; build/measurement evidence, not initial device proof. |
| E-0006 | Extracted BQ27441 provider and shared expander owner, avoiding display-task wake deadlock; capture was insufficient for delivery proof. |
| E-0007 | Baseline investigation recorded pinned-linker, BLE stack-floor, and documentation-link failures; overall qualification remained incomplete. |
| E-0008 | Review reopened core/runtime/target work: delivery, timing, generation, lifecycle, cleanup, upload, expander consistency, capacity, and test gaps. |
| E-0009 | Trimmed fixed field arrays; BLE `.bss` fell 1920 bytes and stack rose from 28012 to 29932; floor still failed. |
| E-0010 | Fixed fresh-cache CacheThenRefresh entry and the interval swallowing its requested fresh result; regression tests added. |
| E-0011 | Fixed pre-await timestamp reuse and silent provider-generation wrap; completion-time clock tests added. Conversion lead time remains absent. |
| E-0012 | Fixed Medinote re-entry tracking/status and rejection of sleep after failed cleanup; owner renewal and device fault fixtures remained open. |
| E-0013 | Failed environment/battery cleanup acknowledgements now block Inkplate panel entry; resume remains required on rejection. |
| E-0014 | Added 2-second acknowledgement waits for the four observation providers; admission/resume and IMU/touch bounds were not covered until E-0027. |
| E-0015 | Medinote device: SHTC3 and ADC delivered at entry and periodically, revisions 1 to 2; re-entry/sleep faults and UI latency unqualified. |
| E-0016 | Inkplate production flash hash verified; garbled capture did not establish runtime delivery. |
| E-0017 | Clean capture after power cycle and correct serial setup; corrected the earlier unproved RTS/DTR diagnosis. |
| E-0018 | Panel-gating liveness evidence, but no direct provider-delivery proof yet. |
| E-0019 | Inkplate capture beyond 300s showed real BME688/BQ27441 entry and periodic delivery plus six clean suspend/resume cycles. Battery failure logging was missing. |
| E-0020 | Injected battery cleanup failure blocked panel entry; injection reverted and normal firmware restored. Not a physical bus fault. |
| E-0021 | Broad validation rerun; full gate still stopped at pinned-linker, with BLE-floor and 16 documentation-link failures remaining. Passing quality omitted product source coverage (E-0024). |
| E-0022 | BME688 cleanup injection capture missed startup. Original cause remains unknown; continuous IMU-suspend-hang localization is contradicted by later display-owned delivery (E-0024). |
| E-0023 | Documented UI-coordinator handoff. Deferring another hardware run was a session decision; authority/owner-generation requirements remained open. |

Recorded BLE linked-stack measurements, against the unchanged 33900-byte floor:

| Evidence/build | Stack bytes | Gap to floor |
| --- | ---: | ---: |
| E-0007 pre-migration | 31476 | 2424 |
| E-0007 initial migration | 28012 | 5888 |
| E-0009 after field trim | 29932 | 3968 |
| E-0021 later build | 29948 | 3952 |
| E-0027 latest reported BLE build | 29932 | 3968 |

These are different historical builds; E-0030 records the current implementation separately below.

### E-0024 — E-0022 follow-up: startup refresh recovery fixed and one-shot BME688 cleanup failure qualified

- 2026-08-27, Phase 4. Initial scan rejection previously trapped startup behind the readiness gate;
  later service full scans also failed to complete startup. Shared completion tracking now marks any
  successful full scan ready and retries with an existing backend at least 1000ms after failure,
  respecting upload repaint suppression. Environmental updates cannot bypass startup backoff.
- Six panel-tracking tests (three new), 55 observation tests, default debug/release/Clippy and scoped
  static checks passed. Direct metrics covered the product files omitted by the code-analysis script.
  Default-profile DRAM measurements did not qualify the BLE floor; IRAM references were 63/78.
- Inkplate MAC `e8:6b:ea:fb:d5:54`, `cu.usbserial-2030`; canonical app-only flash at `ota_0`/`0x80000`.
  Complete 20s capture: IMU ack in 28ms; one injected BME688 `CleanupFailed`; scan blocked; clients
  resumed; startup retry succeeded at 6453ms and runtime-ready followed the repaint at 10234ms.
- Normal image restored and runtime-ready observed near 9.30s. Separate `hostctl time-set` passed;
  two `time-status` calls timed out, while passive telemetry remained live. Clock-query failures are
  separate and unlocalized. Real bus faults, load, and all-client deadline qualification remained open.
- Artifacts: `logs/typed-observation-subscriptions_e0023_20260827/` (local marker differs from entry ID),
  including `injected_complete/`, `restored/`, exact ELFs, source hashes, and clock/passive logs.
  Injected ELF SHA-256: `323487235a4acd05065ff8b9f4f296b922d027cb055d64c9adef82f289077638`.
  Restored archived normal ELF: `ce5ea1f8e5bd930072b28d8e15ac0f7d30c7b7efb2160e6e37dc99f24ff712c7`;
  app: `ce41cfc0867771ff6b9edcfc13318a3159693d2ebe7909551cfddba11f6165c2`.
  A later normal rebuild differed by 32 app bytes; no byte-identical rebuild claim is made.

### E-0025 — Battery acquisition-failure logging

- Added one `BATTERY_ACQUIRE_FAILED error={:?}` emission for each wake/read/range failure, returning
  the original result and retaining prior values/backoff. All 55 observation tests and default
  debug/Clippy passed. No flash then; actual driver/serial coverage followed in E-0028.

### E-0026 — Plan and status reconciliation

- Documentation-only correction of stale delivery, whole-panel-bound, and full-baseline-pass claims.
  Added startup/battery acceptance cases, kept authority/resource/fixture work open, and preserved
  E-0022's unknown cause. Scoped links/whitespace passed; no new firmware/device evidence.

### E-0027 — Bounded panel-client waits, recovery incomplete on review

- Added shared `bounded(operation, deadline)`, `SuspendAck`, and five-client aggregation. All five
  clients now bound suspend admission/acknowledgement and resume; timeout/cleanup failure rejects
  initial panel entry, and each client's resume is attempted independently.
- Five host tests cover helper timeout, opposite-state stale-signal filtering, and aggregation.
  They do not execute production client/orchestrator sequences. E-0029 reproduces failures those
  tests miss, so the earlier claim that clients cannot be stranded is withdrawn.
- Recorded builds/lint/static checks passed within their lanes; full baseline still had pinned-linker,
  BLE-floor, and documentation-link failures. No hung-client device qualification was performed.
  The suggested E-0022 IMU-hang localization remains unsupported, as corrected by E-0024.
- Source: [bounded control](../../products/meditamer/src/firmware/bounded_control.rs),
  [panel bus](../../products/meditamer/src/firmware/panel_bus.rs), and
  [helper tests](../../tools/touch_replay/tests/bounded_control.rs).

### E-0028 — Real battery-driver host tests and injected serial evidence

- Split battery into `mod.rs`, `driver.rs`, and `types.rs`. Generic `BqDriver<I2C, Wake, Diag>` keeps
  the real wake/read/range/error path host-testable; production adapters retain the shared expander
  and console sink. Four tests exercise wake/read/range failure and 84%-failure-failure-91% recovery
  through the real `ProviderLoop` and driver. E-0029 found no functional defect in this refactor.
- Tests use fake wake/I2C/diagnostics, not real expander reconciliation or UART. They do not explicitly
  step during idle/backoff; the repeated-failure case only samples eligible acquisition times.
  IMU trace delivery is outside these tests.
- Recorded default debug/release/Clippy, touch lint, and host suites passed. A separate full baseline
  stopped on hostctl `direct_mode_repeated_transport_reset_retries_below_fallback_streak_limit`;
  repeated outcomes were inconsistent, so this remains a qualification blocker rather than a pass.
- Saved Inkplate capture shows four injected `WakeFailed` diagnostics, no `BATTERY_DELIVER`, runtime-ready,
  and no recorded crash signature. The restored capture shows runtime-ready and BME688 delivery, but
  no battery delivery. It demonstrates removal of the visible fault, not restored fuel-gauge success.
  Physical wake/read/range faults and a hung-client control capture remain unqualified.
- This run used raw `espflash write-bin 0x80000`, not the canonical wrapper; serial startup was partly
  missed and transport retries were recorded. Future qualification uses the canonical capture workflow.
- Artifacts: `logs/typed-observation-subscriptions_e0028_20260827/`. E-0029 rechecked both captures
  and both app hashes; this is the last recorded restored image, superseding E-0024's device state.
  Fault app `e0028_fault_injected.bin`: `e687c812c70f81d14bed152510c9fc1519372e8a1ad6d821233ebbf62d0035b2`.
  Normal app `e0028_restored.bin`: `d4a83a4bc2ef42b9f68408f77eb21a5ec2de1d82d99c6089b550a1a4a4c23d46`.

### E-0029 — Review, actual-control repros, and documentation compaction

- Date: 2026-08-27. Scope: E-0027/E-0028 implementation and evidence. Firmware stayed read-only;
  changed only the plan/ledger and added ignored review artifacts. No device operation or flash.

#### R1 — P1: a matching stale acknowledgement can authorize a new suspend

- [IMU control](../../products/meditamer/src/firmware/imu/tasks/acquisition_control.rs) accepts any
  `Suspended` value after sending, with no request identity; the same pattern exists in all clients.
  Repro: suspend A times out, its handler signals late, asynchronous resume A is queued, then suspend B
  consumes A's signal and returns `Quiesced` before B is handled. The participant then processes resume A
  while B is still queued. This can authorize a scan without the current request being quiescent.
- Fix acknowledgement correlation and test repeated same-state cycles against production controls.
  The existing late-ack test only checks that `Suspended` is not mistaken for `Running`.

#### R2 — P1: resume timeout can discard the only recovery intent

- [Resume orchestration](../../products/meditamer/src/firmware/panel_bus.rs) logs failure and returns
  without retaining retry intent. Repro using actual IMU functions: suspend A succeeds; while its task
  is delayed, resume A is queued and times out, suspend B fills the second slot and times out, then
  resume B times out at admission and is dropped. When the participant runs again it handles resume A,
  then suspend B, leaving it suspended with an empty queue and no pending recovery.
- Retain/reconcile desired running state or equivalent recovery work independently of another panel
  transaction. The existing partial-suspension test pre-signals Running and never tests resume admission.

#### R3 — P1: waveform-close failure does not gate panel finalization

- [close_touch_waveform_window](../../products/meditamer/src/firmware/panel_bus.rs) logs timeout and
  returns `()`. The cooperative [full](../../boards/inkplate-tempera/src/display/async_impl/full.rs)
  and [partial](../../boards/inkplate-tempera/src/display/async_impl/partial.rs) paths then finalize
  the panel as if closure succeeded. A touch task still using/holding shared I2C can therefore overlap
  or block the PMIC/expander phase after failed quiescence; the initial entry gate does not protect it.
- Propagate closure failure and define bounded abort/cleanup behavior for an already-powered panel.
  Confirmed by source tracing, not by a new hardware injection.

#### Verification and limits

- Repro crate path-includes the unchanged production IMU control and `bounded_control` modules, with
  real Embassy Channel/Signal and a controllable timer stand-in. `stale_ack` and `lost_resume` both
  reproduced the states above. This proves control logic, not physical timing or bus behavior.
- `scripts/host-test.sh test touch-core`: **140 passed**, including the 5 control-helper and 4 battery
  tests. This corrects E-0027/E-0028's 111/115 totals. `scripts/host-test.sh test observation`: **55 passed**.
  `scripts/host-test.sh lint touch-replay` and
  `CARGO_INCREMENTAL=0 targets/meditamer-inkplate/build.sh debug default` passed; existing `wifi_ipv4`
  and `proc-macro-error2` warnings remain. No full baseline or BLE measurement was rerun.
- Scoped Markdown links, changed-line whitespace/local-path checks, and document LOC advisory were
  checked after compaction. The existing hardware-matrix E-0024 anchor is preserved.
- Artifacts: `logs/typed-observation-subscriptions_e0029_20260827/` contains `control-repro/`,
  `stale-ack.log`, `lost-resume.log`, validation logs, source SHA-256 records, and complete
  pre-compaction documents. Rerun the compiled repro with `control-review-repro stale_ack` or
  `control-review-repro lost_resume`; its assertions capture the reviewed defects, not desired behavior. E-0030 replaces them with permanent regression coverage.
- Review result: battery diagnostics had scoped host/device evidence; R1–R3 blocked panel-control
  recovery. E-0030 resolves these software findings; device qualification and the wider plan stay open.


### E-0030 — Correlated control, retained recovery and safe waveform abort

- Date: 2026-08-27. Implements E-0029 R1–R3 in the existing dirty working tree; unrelated migration
  work is preserved. No flash or device operation; E-0028 remains the last recorded device image.
- **R1/R2 resolved in software:** all five participants use the shared production `Control` protocol.
  Each suspend has a non-reused identity; only a matching current acknowledgement authorizes access.
  Synchronous latest-state publication replaces the bounded command FIFO. Timed-out/dropped waits
  retain Running intent, so a delayed participant converges without another panel request. Reset
  intent stays pending until matching Running completion; counter exhaustion rejects new suspension.
- Touch retains separate replay events, control-first dispatch and post-resume controller work.
  Hard-park diagnostics observe the published atomic identity, including fire-and-forget resumes.
  Pipeline starts before acquisition; Running confirms reset enqueue, not pipeline consumption.
- **R3 resolved in software:** cooperative full/partial/gate-drain/fallback paths propagate failed
  waveform closure into GPIO-only isolation and an error, without PMIC/expander finalization.
  Partial state is invalidated and cleanup stays pending. Shutdown has a 2-second deadline covering
  expander/shared-bus lock admission and transactions; timeout drops the future before GPIO isolation.
  Cleanup retries after fresh all-client quiescence, at least 1000ms after the previous attempt
  completes, including startup, upload-suppressed and ButtonOnly loops without new display work.
- Regression tests exercise shared production code: 8 control tests cover same-state stale replies,
  lost-resume schedule, dropped waits, partial suspension/eventual recovery, sticky reset, mismatched
  acknowledgements and exhaustion. Five finalizer tests cover close failure, normal shutdown/hold,
  bus-lock timeout/recovery, waveform-plus-shutdown failure and in-flight cancellation/lock release.
  Two lease tests cover retained retry and completion-based backoff.
- Validation: **145 touch-core + 55 observation + 40 board tests = 240 passed**. Touch/board strict
  host lint, default debug/release/Clippy and BLE release builds passed. Panel-bus/placement gates
  passed; IRAM flash references **63/78**. Scoped RCA over 25 files: maximum SLOC **534**, function
  cognitive **40**, cyclomatic **28**, arguments **7**. Existing `wifi_ipv4`/`proc-macro-error2`
  warnings remain. Scoped Markdown links and whitespace checked; full baseline not rerun.

| Saved ELF | `.data` | `.bss` | `.stack` | `.dram2_uninit` |
| --- | ---: | ---: | ---: | ---: |
| Pre-edit default debug | 16640 | 72052 | 107372 | 113736 |
| E-0030 default debug | 16872 | 72060 | 107124 | 113736 |
| E-0030 default release | 16592 | 72020 | 107452 | 113736 |
| E-0030 BLE release | 16732 | 82764 | 29700 | 113736 |

- Debug stack remainder decreases **248 bytes** against the saved pre-edit ELF (not a pre-migration
  baseline). Task pools: touch acquisition **520→584**, environment **1032→1040**, battery **560→568**;
  pipeline **640**, IMU **368**, board runtime **72** unchanged. No new heap buffer or queue-depth growth.
- **BLE budget remains failed:** stack **29700/33900**, **4200 short**. Image **1963952/3661824** and
  board pool **72/72** pass. No floor change or exemption was applied.
- Limits: controls assume the existing single panel-operation owner; timeout cannot force a hung
  peripheral/output task to run. Host tests do not prove GPIO electrical safety, real I2C cancellation,
  PMIC rails off, or physical recovery. GPIO isolation explicitly leaves rail power unknown until
  confirmed cleanup. Expander cache reconciliation and the broader qualification blockers remain open.
- Artifacts: `logs/typed-observation-subscriptions_e0030_20260827/` contains pre-edit source copies,
  source/toolchain/hash manifest, saved debug/release/BLE ELFs, linked sections/symbols, and validation
  logs. Next software work: upload suppression and batched presentation; resource correction and
  identified-device fault/recovery qualification remain required before closing the plan.


### E-0031 — Presentation batching, driver recovery and upload-policy review

- Date: 2026-08-27. Continued the plan in the existing dirty checkout. No flash or device operation.
- **Upload policy corrected after review:** the old `BatteryTick` guard skipped display-owned reads.
  Current providers run independently; SD power, panel and sensor paths use the shared bus/expander
  owners. Source and the specified Wi-Fi/upload archives did not establish a remaining blanket
  sampling prohibition. Removed the provisional admission Watch and hooks entirely; provider bodies
  match E-0030. Sampling/cache delivery remain upload-independent; existing panel-repaint restrictions
  stay in place. Coexistence and throughput under load remain required device evidence, not assumed safe.
- Environmental delivery now only caches the combined reading in the backend. Normal rendering applies
  it with ready input/timer changes; service dirty state flushes after clock work, so a full Ambient Home
  repaint consumes the pending region without an earlier observation-only scan. Explicit touch
  press/release frames remain separate. Three product-adapter tests cover missing state, combined
  readings/cadence and fresh-result bypass; physical rendering/batching is not host-proved.
- SHTC3 acquisition has a 250ms deadline followed by a separate 100ms cleanup deadline. Failed cleanup
  rejects the new sample and retains both causes; suspend cleanup fails closed. ADC's 50ms conversion
  guard resets on timeout/drop through a narrow vendored HAL `cancel_oneshot` API that clears hardware
  and active-channel state. Nine tests use real provider code/Embassy virtual timers and fake peripherals;
  they prove logical cleanup/retention/recovery, not electrical cancellation or real deadline latency.
- The shared PCAL6416A cache invalidates before mutating awaits. Failure/drop requires all eight owned
  registers to reconcile before another write; input ports are excluded to preserve interrupt state.
  Reconciliation fills the invalid owner cache in place, with no second register buffer across awaits.
  Eight actual-owner tests cover partial/applied failures, cancellation, failed reconciliation and wake
  recovery. Host exposure and mock-time dependencies preserve target APIs and dependency features.
- Added battery idle/rate-limit/backoff steps to verify no repeated diagnostic or sample/revision change.
  Focused validation: **158 touch-core + 55 observation + 48 board = 261 passed**. Touch/board strict
  host lint, Inkplate default debug/release/Clippy and BLE release, and locked Medinote build pass.
  Panel source gate and IRAM **63/78** pass. Scoped code metrics remain below the ceilings. No full
  baseline, Wi-Fi/device gate, physical fault injection or plan-phase acceptance is claimed.
- Final BLE linked stack is **29460/33900** (**4440 short**), 240 bytes below E-0030; the resource
  gate remains failed. Image **1964960/3661824** and board pool **72/72** pass. No memory floor was
  lowered, and no PSRAM relocation was made. Final sections/symbols and ELF hashes are in the artifacts.
- Existing compiler warnings remain (`wifi_ipv4`, future incompatibility; Medinote allocator/RWX).
  All source changes and test-only dependencies are uncommitted; unrelated user work is preserved.
- Artifacts: `logs/typed-observation-subscriptions_e0031_20260827/` holds test/build logs, final image
  and source hashes, resource measurements and the discarded upload-policy proposal. Remaining work:
  conversion lead time, authority/owner lifecycle, fixtures, resource limits and device qualification.


### E-0032 — Freshness policy and bounded consumer lifecycle integration

- Date: 2026-08-27. Software work in the existing dirty checkout; no flash/device operation.
- Timing decision: `max_age` classifies freshness; it is not a delivery deadline. Defer predictive
  scheduling and guessed conversion margins. Drivers retain hardware waits; preserve actual timestamps
  and last good values, and measure latency as firmware/workloads change. Any future hard deadline
  requires an explicit admission/miss contract. This supersedes E-0031's mandatory lead-time follow-up.
- Consumer integration uses existing transport capacities; no new Watch, channel, queue depth or device
  fixture storage. Provider authority/request transport remains behind the open resource gate.
- Medinote now has a product Home adapter owning the existing two capacity-one subscription books.
  Committed entry/re-entry and retained-token rebuild renew a checked observation generation; departure
  and pre-sleep withdrawal clear demand. Failed candidates leave the existing owner untouched. Delivery
  validates owner keys and handles provider generation/health changes without inventing missing values.
- Inkplate delivery uses the backend's exact committed, renderable Ambient View token. Reconciliation
  around presentation preserves unchanged/overlay/rollback owners and resets replacement tracking.
  Health changes have a separate diagnostic path, including failure before the first valid reading.
  Cache-only presentation and existing final refresh batching remain; no blanket upload suppression.
- **Still incomplete:** Inkplate sampling demand remains permanently installed in the provider; owner
  departure currently stops delivery only. Neither target forces fresh-cache entry acquisition without
  request admission. No full authority or fresh-entry contract completion is claimed.
- **New control finding:** Medinote's untagged suspend acknowledgement can arrive after its 2-second
  UI timeout and satisfy a later attempt. The retained resume Signal is likewise uncorrelated. Replace
  this protocol before sleep qualification; lifecycle host tests do not prove that protocol safe.
- Inkplate default debug/release/Clippy, BLE release and locked Medinote builds pass. Touch/core/board
  host tests plus Medinote pass (**162 + 56 + 48 + 91 = 357**); strict touch/core/Medinote lint,
  panel/UI ownership source gates,
  identified-release IRAM **63/78**, scoped Markdown links and whitespace checks pass. Independent
  scoped source review found no additional defect.
- BLE stack is **29428/33900** (**4472 short**), down 32 bytes from E-0031; display task pool is
  **5680→5712**. Image **1966000/3661824** and board pool **72/72** pass. The floor remains unchanged
  and failed. Host adapters each add 32 bytes versus their replaced state; host layout is not Xtensa
  layout. No memory relocation, exemption or promotion is claimed.
- Scoped code analysis has valid-unit maximum SLOC **681**, cognitive **26**, cyclomatic **19** and
  arguments **6**. The parser represents `runtime_ui.rs` as a root function rather than a file unit;
  that target coverage gap remains open, so these numbers are not a whole-scope quality-gate pass.
- No full software baseline, device fixtures, physical sleep/recovery, Wi-Fi/coexistence, runtime stack
  or display batching qualification. Existing compiler warnings remain. Artifacts/source and ELF
  hashes are retained under `logs/typed-observation-subscriptions_e0032_20260827/`.


### E-0033 — Correlated Medinote sleep acknowledgement

- Date: 2026-08-27. Fixed E-0032's sleep-control finding in software; no flash/device operation.
- Replaced three untagged Signals with one target-local desired-state controller. Each suspension has
  a non-reused identity; stale cleanup cannot acknowledge a new attempt, and an old resume cannot
  undo it. Timeout/drop retains intent; recovery is always admitted. Counter exhaustion rejects sleep
  while retaining a terminal resume-only identity. Existing Inkplate controls are unchanged.
- Provider control runs before due sampling and wins the wait selection. Each new suspension retries
  cleanup, including after an earlier failure; the actor remains suspended until current resume intent.
  Existing acquisition/cleanup deadlines remain unchanged, and acquisition is not cancelled by control.
  Both light/deep sleep paths require explicit `Quiesced`; logs identify requests and accepted/stale acks.
- Validation: **171 touch-core tests passed**, including **18 Medinote driver/control tests** (nine
  existing driver cases, eight protocol interleavings and exhaustion). Strict touch lint, locked Medinote
  release build, UI ownership source guard and scoped Markdown links passed. Production actor/wiring
  review found no further defect. No full baseline or physical sleep/deadline qualification is claimed.
- S3 control storage is **40 bytes**, replacing **36** bytes of signals; environment task pool
  **768→776**, UI pool **2440** unchanged. Final S3 `.data=49224`, `.data.wifi=284`, `.bss=203600`,
  `.stack=55232` (80 below E-0032, including compiled layout changes). No capacities or memory floor
  were reduced. Current compiler warnings remain.
- Inkplate source/image is unchanged: E-0032 BLE `.stack=29428/33900`, **4472 short**. Read-only
  symbol/owner review identifies the internal **9600-byte LVGL draw buffer** as a PSRAM candidate;
  no relocation was performed. Pointer-only arithmetic suggests about **39024** stack bytes, but an
  actual implementation must prove allocation/lifetime/cache safety, rebuilt sections and device behavior.
- Artifacts: `logs/typed-observation-subscriptions_e0033_20260827/` contains pre-edit source copies,
  final source/image hashes, tests, build logs and section/symbol measurements. Next: discuss and agree
  the memory recovery before changing placement, then complete admission and device qualification.


### E-0034 — Explicit BLE roles and two-board memory measurement

- Date: 2026-08-27. Implemented the agreed feature-boundary recovery in the existing dirty checkout.
  No buffer relocation, heap/queue resize, linker change, flash or device operation.
- `platform/ble` now separates chip selection from explicit `central` and `peripheral` roles. Trouble
  is optional; controller-only consumers retain the probe and host-free primitives. Either role enables
  the bounded-pool adapter. Product `ble-foundation` selects peripheral; both targets' `shared-ble-runtime`
  select central with unchanged security, legacy pairing, packet and notification requirements.
- Inkplate's canonical diagnostic-peripheral image no longer inherits central/security state.
  Medinote's default CheerTok central retains pairing and eight notification entries. Combined roles
  still unify capabilities and need a separate budget; this does not remove future BLE requirements.

| Release image | `.data` | `.data.wifi` | `.bss` | CPU0 `.stack` |
| --- | ---: | ---: | ---: | ---: |
| Inkplate E-0032 BLE baseline | 16764 | 1872 | 83004 | 29428 |
| Inkplate E-0034 BLE | 16324 | 1872 | 80556 | 32308 |
| Medinote E-0033 baseline | 49224 | 284 | 203600 | 55232 |
| Medinote E-0034 default, including BLE | 49224 | 284 | 203600 | 55232 |

- **Inkplate recovers 2880 stack bytes; the gate remains failed at 32308/33900, 1592 short.**
  `HOST_RESOURCES` shrinks **3024→968**, BLE task pool **2992→2672**. Image **1804976/3661824**
  and board pool **72/72** pass. Its internal draw buffer stays **9600**, S3's stays **16000**;
  no display-speed result is inferred. The explicit combined-role ordinary release reserves **26084**
  stack bytes; its profile differs from canonical `ble-release`, and no acceptance is claimed for it.
- Keep physical regions and allocation kinds separate: Inkplate's **68736-byte internal heap** lives
  in almost-full internal `dram2_seg`, while its LVGL arena already uses PSRAM. Medinote's **49152-byte
  heap** and **49152-byte LVGL arena** are internal; it initializes no PSRAM. Its unused **73744-byte**
  post-boot linker region requires separate startup/sleep validation. S3 instruction/data aliases count
  the same SRAM once. Stack reservations are not runtime high-water/headroom evidence.
- Validation passed: **122 BLE host tests**, strict BLE host lint; canonical locked Inkplate default/BLE
  and Medinote releases; Inkplate central-only and combined-role releases including their probes.
  Both chip-only library checks pass with the existing RTOS IPC feature supplied (the first bare-crate
  check omitted that consumer requirement and failed in vendor queue exports). Medinote's default build
  also builds its controller, scan, connect and diagnostic-client probes.
- Extended the existing BLE controller guard to inspect six actual target dependency graphs. Four
  isolated negative cases reject central leakage, lost Medinote legacy pairing and unexpected Trouble
  in either chip-only graph. Network-owner source guard and identified BLE IRAM references **45/78**
  pass. Review caught and fixed the initial chip-only role omission; final scoped review has no findings.
- Scoped documentation links and whitespace pass. The DRAM reference remains above the 300-line
  high-attention Markdown advisory; the append-only ledger is exempt.
- Existing compiler warnings remain. No full software baseline, physical BLE/pairing/reconnection,
  sleep/wake, runtime heap/stack or Wi-Fi regression qualification is claimed. BLE promotion remains
  blocked; provider admission/fixture storage and the wider observation plan remain incomplete.
- Artifacts: `logs/typed-observation-subscriptions_e0034_20260827/` contains before/final source hashes,
  saved images, section/symbol measurements, feature graphs, guard negatives and test/build logs.
  Next: measure redundant SD staging removal and cold diagnostic ownership before considering draw
  buffer relocation; recover capacity for the final authority/fixture wiring, not just the present floor.


### E-0035 — Remove redundant SD fallback staging

- Date: 2026-08-27. CMD24 fallback now borrows each `[u8; 512]` directly from the input slice using
  `as_chunks`, available since the crate's Rust 1.88 minimum. Removed the intermediate copied sector
  across `await`. Input-length checks, LBA/error handling, yields and CMD25 fallback policy are unchanged.
  `write_sector` still stages data in the existing internal DMA frame; no allocation or placement change.
- **Measured recovery: 512 bytes.** BLE SD task pool **7064→6552**, `.bss` **80556→80044**,
  `.stack` **32308→32820**. `.data=16324`, `.data.wifi=1872`, `.dram2_uninit=113736` unchanged.
  The unchanged **33900** floor still fails by **1080 bytes**. Image **1804112/3661824** and board pool
  **72/72** pass. Medinote does not consume this driver; its E-0034 measurement remains unchanged.
- Canonical locked Inkplate default and BLE release builds pass; **24 SD/FAT host tests**, strict SD
  host lint, formatting, FAT stackless and network-owner source guards pass. The host suite uses a
  probe stub: it does not execute the changed hardware loop or prove real CMD24/DMA behavior.
- No flash, physical SD writes, Wi-Fi/upload regression run or throughput claim. Existing compiler
  warnings remain. The software change preserves the borrowed payload lifetime through each completed
  write; hardware qualification remains required before landing storage/upload changes.
- Artifacts: `logs/typed-observation-subscriptions_e0035_20260827/` retains before/final source hashes,
  both images, sections/symbols and validation logs. Next: cold diagnostic payload ownership and budget
  the unfinished authority/fixture storage; the stack deficit and broader plan remain open.


### E-0036 — Retire large boot frames before executor polling

- Date: 2026-08-27. Audited the exact E-0035 BLE ELF and its matching source snapshot before choosing
  larger ownership changes. The 32820-byte CPU stack is distinct from 26272 bytes in 14 permanent
  Embassy pools; SD/display pools are 6552/5712, while their CPU poll frames are 7120/9136 bytes.
- Found large boot frames retained beneath every main executor poll. `system::run` now calls outlined,
  returning `initialize` and `spawn_initial_tasks` helpers. The small handoff owns the existing external
  board resources plus radio tokens/static network reference. The executor remains in its non-returning
  stack owner. No new allocation category, buffer relocation, capacity, linker or floor change.
- Exact BLE entry frames: `system::run` **5136→48**, executor `run` **48→48**, executor `run_inner`
  **1936→32**. Permanent subtotal **7120→128**, removing **6992 bytes** beneath each poll. New init
  and spawn frames **5104/48** return before polling. Default-release permanent subtotal **5856→112**
  removes **5744 bytes**; its init frame is **3920**. These are disassembly measurements, not runtime
  high-water or worst-case stack proof; other calls, register-window spill and interrupts still count.
- BLE linked sections remain `.data=16324`, `.data.wifi=1872`, `.bss=80044`, `.stack=32820`,
  `.dram2_uninit=113736`. **The unchanged 33900 floor still fails by 1080.** Task-pool sizes remain
  unchanged; board pool **72/72** and image **1804096/3661824** pass. Default sections remain
  **16576/540/71732/107756/113736** in the same order. Medinote source/image is unchanged.
- Locked Inkplate default/BLE/minimal release builds and target all-features Clippy pass. Network-owner,
  BLE controller/role and fixed-array stack-risk source guards pass; identified BLE IRAM references
  remain **45/78**. Independent lifetime review found no defect: BLE→Wi-Fi→board ordering/priorities,
  cfg ownership, fatal failure paths and diagnostic skips are preserved. Diagnostic-only resource drops
  happen earlier, still before executor polling. Existing compiler/dependency warnings remain.
- Larger follow-up is now explicit: external SD runner with internal hardware anchors and separated
  FAT initialization, then external UI model with outlined init/install and cleanup by reference.
  FAT already persists externally but constructs a **4768-byte** value in the SD poll frame. UI also
  makes repeated Backend copies. Moving destinations alone does not eliminate these stack costs.
  Preserve queues, cancellation, allocation-failure cleanup and cache safety; keep actual DMA storage,
  panel scan context/LUT, draw buffers, CPU stack and ISR state internal. Both moves remain unimplemented.
- No flash, physical startup/sleep, stack high-water, display-speed or SD/upload qualification. No full
  software baseline or observation-plan completion is claimed. Scoped formatting, whitespace and
  Markdown links pass; the DRAM reference retains its existing high-attention LOC advisory.
- Artifacts: `logs/typed-observation-subscriptions_e0036_20260827/` saves before/final source hashes,
  three images, disassembly, entry-frame/section/pool measurements and build/guard logs. The preceding
  read-only audit is in `logs/typed-observation-ram-owners_20260827/`. Next: measure the SD ownership
  prototype, then final authority/request/fixture storage and identified-device qualification.


### E-0037 — External SD runner with internal hardware ownership

- Date: 2026-08-27. The SD task retains its probe and executor header internally; one pinned external
  runner borrows the probe and owns initialization, request/dispatch and upload-session state. FAT
  remains external. Outlined allocation helpers return pointers; runtime initialization mutates its
  existing owner. No request/queue capacity, retry, power policy, linker or heap-region change.
- Both allocations precede runner execution. Failure drops unpolled owned state before the existing
  reset/halt path. No per-request allocation was added. Independent review traced SPI/PDMA, timer and
  channel wake paths to internal static queues/wakers and the outer task header; no atomics move.
  Normal synchronous flash operations do not poll the runner while caches are unavailable.
- Isolated BLE task pool **6552→1456**, recovering **5096 internal bytes**. Sections are
  `.data=16324`, `.data.wifi=1872`, `.bss=74948`, `.stack=37916`, `.dram2_uninit=113736`.
  The unchanged **33900 stack floor now passes with 4016 bytes to spare**. Board pool **72/72** and
  image **1803264/3661824** pass. The external runner requests **3640 bytes**, plus allocator overhead;
  its existing **4768-byte FAT allocation** is unchanged. No internal heap capacity was consumed.
- BLE SD poll frame **7120→2144**. FAT/runner factory frames **4624/3712** occur only at startup;
  construction is still by value, not guaranteed in place. Nested request/callee frames remain relevant.
  Default release sections are **16560/540/66652/112852/113736** in the same order: stack also grows
  **5096**. Its outer SD poll is **64**, separate runner poll **1632**, FAT/runner factories **3296/3680**.
  Static frames and linked reservations are not measured runtime high-water or whole-call-chain bounds.
- Subsequent default-profile call-path review found SD task spawn construction grows **736→1440**
  bytes. This one-time cost remains in the startup budget; the steady-state reduction is not a claim
  that every individual frame shrinks.
- Locked default/BLE/minimal builds, target all-features Clippy, **24 SD/FAT host tests** and strict SD
  host lint pass. The host probe is a stub, not execution of this hardware task. FAT/network-owner
  source guards and identified BLE IRAM **45/78** pass. The FAT allocation assertion now checks the
  outlined factory; three negative cases reject an internal field, internal allocation and commented
  external allocation. Existing compiler/dependency warnings remain.
- No flash, physical SD writes, cancellation injection, Wi-Fi/upload regression or speed claim.
  Physical regression is required before landing this storage/upload ownership change. Medinote is
  unchanged; its separate BLE and RAM requirements remain. Authority/request/fixture storage is still
  unbudgeted, so passing today's linked floor does not close Phase 0.
- Artifacts: `logs/typed-observation-subscriptions_e0037_20260827/` preserves the isolated code snapshot,
  three images, source/image hashes, disassembly, sections/pools/frames and validation logs. Next:
  externalize UI model state and remove Backend construction/copy costs from ordinary polling.


### E-0038 — External UI model and outlined Backend initialization

- Date: 2026-08-27. DisplayLoopState is allocated once in external PSRAM before live LVGL resources
  or the boot-control await. Allocation failure logs and enters the existing reset/halt path.
  DisplayContext, scan LUT, draw buffer, callbacks, interrupt state and queue capacities stay internal.
- Backend initialization now installs into a caller-provided destination and returns only a small
  status. Errors leave the destination unchanged; initialization cleanup borrows Backend and removes
  its children in the same order. Independent source review found no model-address callback captures,
  migrated atomics, ownership, failure-cleanup or cache-off access defect.
- The first candidate left a large by-value Backend return between outlined functions. BLE optimized
  it away, but default startup frame subtotal regressed **9232→11600**. Candidate 2 removes that return
  boundary: default display/helper/Backend startup subtotal is now **416+304+4304=5024**. Both profiles
  were rebuilt after the correction; the rejected candidate and measurements remain in the artifacts.
- Isolated UI recovery against E-0037: BLE display pool **5712→3064**, linked stack **37916→40564**
  (**+2648**). Final sections are `.data=16324`, `.data.wifi=1872`, `.bss=72300`, `.stack=40564`,
  `.dram2_uninit=113736`. The **33900 floor passes by 6664 bytes**; board pool **72/72** and image
  **1800560/3661824** pass.
  SD plus UI recover **7744 internal bytes** against E-0036. Their new external payloads total **6288**
  bytes plus allocator overhead; FAT's existing external allocation is unchanged.
- BLE display poll **9136→688**; outlined init/install **4496**, state allocator **64**. Identified
  UI recovery and SD upload/FAT entry-frame paths fall **17136→8688** and **12800→7824** against E-0036.
  Default sections are **16544/540/64004/115516/113736** in the same order; linked stack gains **7760**
  overall, including layout differences. Its UI recovery subtotal falls **11504→9776** despite a
  **720-byte** transition-frame increase. The **704-byte** SD spawn increase is recorded in E-0037.
  These sums exclude the common executor prefix, deeper calls, register-window spill and interrupts;
  they are not device high-water measurements or proven worst-case bounds.
- Locked Inkplate default/BLE/minimal releases and all-features target Clippy pass. Host regressions:
  **171 touch-core**, **46 shell**, and UI-shell tests; strict touch/UI-shell lint passes. These suites
  exercise adjacent lifecycle/callback behavior, not the concrete Backend initialization or allocator
  failure path. Source ownership/panel/stack-risk checks and identified BLE IRAM **45/78** pass.
- No flash, physical allocation-failure, display-speed, startup/sleep or SD/upload qualification. No
  full software baseline or product-plan completion is claimed. Existing compiler warnings remain;
  scoped analysis reports six valid file units, max SLOC/cognitive/cyclomatic **681/39/28**. Unchanged
  `process_cycle` has aggregate argument metric **9** over the configured **8** (including nested
  closures), confirmed against the before-snapshot. This existing quality gap and Backend SLOC advisory
  remain; no full code-analysis gate pass is claimed.
- Plan and DRAM reference now record the floor pass, separate memory kinds and per-profile stack
  evidence. Next: budget final authority/request/fixture storage, then qualify the completed ownership
  changes and provider behavior on identified hardware. Medinote remains a separate unchanged budget.
- Artifacts: `logs/typed-observation-subscriptions_e0038_20260827/` preserves before/final source and
  image hashes, three final images, candidate history, disassembly, frame/pool/section measurements,
  review notes and validation logs. E-0037 retains the isolated SD measurements.


### E-0039 — Bounded observation budget and BME688 authority admission

- Date: 2026-08-27. Replaced permanent BME688 demand with the UI-owned subscription book. Exact
  committed surface identity renews a checked observation generation; departure withdraws periodic
  demand and delivery, while already-admitted one-shots may finish into cache until expiry. Re-entry
  admits a fresh request even with fresh cache. Five-minute freshness/delivery remain unchanged.
- Measured fixed layouts with isolated `no_std` object probes for both Xtensa chips. Each provider
  budgets two subscriptions and two total outstanding requests: live consumer plus bounded fixture.
  Shared credits count ingress and provider-pending work together; full, closed, invalid and expired
  requests reject explicitly. No silent drop or extra two-request allowance exists behind the Channel.
  One demand receiver suffices; UI state peeks do not spend a registered receiver slot.
- The three-provider/two-field/eight-byte-snapshot plus proposed fixture model is **2360 payload
  bytes**: Inkplate **1488 internal + 872 external**, Medinote **2360 internal**. It includes existing
  objects and excludes concrete drivers, acquisition/select futures, task headers, allocator costs
  and final alignment. It is not incremental linked use or an implemented fixture. Final implementations
  must be rebuilt and gated; an unspecified future driver or larger snapshot has no free allowance.
- Actual new BME688 synchronization statics: demand Watch **56** plus credited ingress **120** bytes.
  Authority grows **64→272**, adding **208 external UI payload bytes**. The requested queue capacity
  expands **40→72** bytes, but final compiler layout shrinks the provider pool **1040→976**; the shared
  battery loop shrinks **672→664**. Pool/layout savings cannot be inferred from component sums alone.

| Final release image | `.data` | `.data.wifi` | `.bss` | CPU0 `.stack` | Delta vs preceding image |
| --- | ---: | ---: | ---: | ---: | --- |
| Inkplate BLE | 16556 | 1872 | 72228 | 40412 | 152 fewer stack bytes vs E-0038 |
| Inkplate default | 16752 | 540 | 63940 | 115372 | 144 fewer stack bytes vs E-0038 |
| Medinote default, including BLE | 49208 | 284 | 203584 | 55264 | 32 more stack bytes vs E-0034 |

- Inkplate BLE keeps **6512 bytes above the unchanged 33900 floor**, board pool **72/72**, and image
  **1808880/3661824**. Its 14 pools total **18456**; display **3064** and SD **1456** remain unchanged.
  Internal `dram2` remains **113736**. Medinote retains central/security/pairing and its existing memory
  placement. No linker, heap capacity, draw buffer or BLE-role change was made.
- CPU frames are separate evidence: BME688 BLE/default poll grows **848→928 / 992→1344**; display is
  **704 / 416**. The permanent BLE boot subtotal remains **128**, and SD/init placement remains as in
  E-0037/E-0038. These entry frames and linked reservations are not whole-path or runtime peak bounds.
- Added completion-aware core admission: compatible requests arriving during a conversion reuse its
  result; incompatible ones stay queued. Expiry wakes release credits even during retry backoff.
  Successful completion before expiry remains valid if synchronous processing crosses the expiry tick.
  Reactive demand/request/resume wakes honor the retry deadline measured from failed completion.
- Control is checked before every step and immediately after conversion. Synchronous suspension
  closes ingress; retained control still drains pending work if its caller times out. A non-consuming
  request-ready wait prevents lost credits when control wins selection. A checked closure generation
  lets the UI retry a still-live entry after a suspension it did not observe; its original expiry is
  never extended. Fresh cache and failed revisions cannot spend the explicit-result delivery bypass.
- BME initialization and sample failures now log the concrete error once per attempted acquisition;
  retry backoff bounds repeat attempts. Success/cache retention and shared-bus/panel rules remain.
- Validation passed: **64 observation tests**, **195 touch-core tests** (including **31 actual
  authority/ingress/control presentation tests**), **91 Medinote tests**, and their registered strict
  lint suites. Inkplate default/BLE/minimal and Medinote releases, plus Inkplate all-features Clippy,
  pass. Existing dependency/compiler warnings remain. Independent source
  review found no remaining actionable admission/control/owner defect after fixes.
- Initial full tests exposed stale retry expectations in battery and Medinote driver regressions.
  They now assert no retry before the returned `retry_at` and advance exactly to it; diagnostic,
  cleanup and retained-reading checks remain. Initial failures and final passes are both retained.
  The expanded runtime tests moved as a whole into `runtime/tests.rs`, a normal private test module;
  production `runtime.rs` is 377 lines. Scoped analysis has max function cognitive/cyclomatic/arguments
  **12/15/8**, with no new enforced offender. Existing wider quality gaps remain in Phase 6.
- UI ownership initially rejected a test-only shell constructor under the production display tree.
  The regression now uses the existing host harness; the unchanged guard passes. Source reachability,
  include-usage, network-owner, panel-bus, BLE role/controller, fixed-array stack-risk and scoped
  Markdown-link guards pass; BLE IRAM references remain **45/78**. The plan is 214 lines; the DRAM
  reference retains its existing high-attention length advisory. No hardware
  flash, physical acquisition/sleep, stack high-water, allocation-failure, display-speed or SD/upload
  qualification was performed; no full software-baseline or overall plan completion is claimed.
- Plan and DRAM reference now distinguish implemented BME admission from remaining Medinote entry
  transport, persistent battery authority and actual device fixtures. Next: apply the bounded admission
  contract to Medinote's two existing Home consumers, then complete battery authority and qualification.
- Artifacts: `logs/typed-observation-subscriptions_e0039_20260827/` retains source snapshots/hashes,
  target layout probes, exact images, sections/pools/entry frames, initial failures, final test/build
  logs and scoped review evidence. Prior memory-placement device gates remain open.


### E-0040 — Medinote fresh-entry admission and two-provider sleep

- Date: 2026-08-28. Both Medinote Home consumers now admit a fresh observation even with fresh cache.
  Each provider has two subscription slots and two shared outstanding request credits (live plus
  reserved fixture). One-minute freshness/delivery and entry expiry remain. Committed/rebuilt Home
  renews checked ownership; ordinary removal withdraws demand/delivery but permits admitted work to
  finish into cache. Suspension-epoch retries retain the original expiry and log rejection transitions.
- Promoted E-0039's credited ingress into `platform/observation`; Inkplate retains its existing static
  ownership and re-exports the shared transport. Thirteen transport tests moved into the platform suite;
  the real Inkplate ingress/control regression remains in the existing presentation harness.
- Both S3 loops drain requests before acquisition and against completed results, release expired credits
  during backoff, reread current demand before each step, and use non-consuming request readiness.
  The UI uses one state peek per provider for admission and delivery, then merges presentation normally.
- ADC now participates in correlated sleep cleanup. Both ingresses close synchronously before either
  waiter is polled; acknowledgements share one absolute 2-second deadline. Only both current successful
  cleanups permit sleep. Timeout/drop retains intent; both resumes precede Home renewal after wake or
  rejected sleep. Terminal revision exhaustion prevents further acquisition while servicing cleanup.
- Review caught same-millisecond ADC completion aliasing the cached timestamp. Combined state now
  publishes the last successful revision separately from attempt revision; entry compares generation
  plus successful revision and still checks the completion timestamp against admission/expiry. Tests
  cover same-tick success, failure-only revision, success followed by failure before UI polling, and
  provider restart. Failure cannot consume the fresh-result obligation.
- Review also found post-step control could bypass terminal handling, and shared state mutated before
  discovering revision exhaustion. Terminal handling now precedes post-step control; success/failure
  checks the next revision before any state mutation or merge callback. Existing state remains coherent.
- Target layout probes measure Home authority **216→488** (+272 internal payload), each combined state
  **40→48**, each state Watch **72→80**, ingress **120** each, and new ADC control **40**. Named static
  growth is **296** bytes. Successful-revision tracking adds an 8-byte local per provider; queue payload
  grows 32 per provider. Actual future layout, not these component sums, determines linked cost.

| Final release image | `.data` | `.data.wifi` | `.bss` | CPU0 `.stack` | Delta vs E-0039 |
| --- | ---: | ---: | ---: | ---: | --- |
| Inkplate BLE | 16556 | 1872 | 72228 | 40412 | Unchanged stack reservation |
| Inkplate default | 16712 | 540 | 63932 | 115412 | 40 more stack bytes |
| Medinote default, including BLE | 49600 | 284 | 204096 | 54368 | 896 fewer stack bytes |

- Medinote SHTC3/ADC/UI pools grow **776→888 / 368→480 / 2440→2712**; total pools **23280→23776**.
  Poll entry frames are **288→368 / 272→320 / 1440→960** respectively. The existing hourglass tick
  frame remains **24752**. These are entry-frame measurements, not connected-path or physical peaks.
- Inkplate BLE remains **6512 above the unchanged 33900 floor**, board pool **72/72**, image
  **1809008/3661824**, and IRAM references **45/78**. Its 14 pools remain **18456**, display **3064**,
  SD **1456**, permanent boot subtotal **128**, and internal `dram2` **113736**. No BLE role, linker,
  heap capacity, draw-buffer placement or radio/ISR storage changed on either board.
- Rebudgeted the three-provider/eight-byte-snapshot plus proposed fixture model: **2408 common bytes**,
  including successful revision in full state Watches and target locals. E-0039 omitted provider
  controls; adding three gives **2540 Inkplate (1668 internal + 872 external)** and **2528 S3 internal**.
  These include existing objects, exclude actual drivers/futures/task headers/allocator/alignment, and
  are not incremental linked costs or an implemented fixture. Common envelopes and controls were
  probed on both chips; the Medinote Home authority uses its actual S3 layout.
- Validation passes: **77 observation**, **103 Medinote** (including **22 adapter**), and **192 touch-core**
  tests (including **28 real Medinote driver/control** cases), plus registered strict lint. Inkplate
  default/BLE/minimal releases, all-features Clippy and Medinote release pass; default image restored
  after the minimal build. Existing dependency/compiler warnings remain.
- The first expanded ADC regression incorrectly expected a health transition never to redeliver cache;
  existing delivery policy permits that. Its corrected assertion verifies retained entry admission
  across failure and subsequent closure/resume. Initial failure logs remain alongside passing results.
  Scoped analysis caught argument aggregation in the new admission method; a shared sample-identity
  helper removes the duplicate expression and the violation. S3 `runtime_ui.rs` already parses as a
  malformed function unit in the analyzer (reproduced before/after), so no full analysis pass is claimed.
- Network-owner, panel-bus, BLE role/controller, stack-risk, UI ownership, tracked-source reachability,
  generated-only include and scoped Markdown-link guards pass. Plan remains under 220 lines; the DRAM
  reference retains its existing high-attention advisory. No flash, physical sampling/sleep, stack or
  heap high-water, display-speed, allocation-failure, SD/upload gate, or full software baseline ran.
- Plan/reference now identify persistent Inkplate battery authority as the next implementation, followed
  by actual fixture consumers and both-board qualification. E-0037/E-0038 placement/device gates stay open.
  Artifacts: `logs/typed-observation-subscriptions_e0040_20260828/` retains before/final source and ELF
  identities, target layout probes, measured sections/pools/frames, review findings and validation logs.


### E-0041 — Persistent Inkplate battery authority

- Date: 2026-08-28. Moved battery subscription and IMU trace delivery out of the fuel-gauge provider
  into a persistent product consumer in external `DisplayLoopState`. It starts before initial panel
  rendering, uses a checked owner generation and is independent of navigation, backend recovery and
  upload mode. Trace publication is nonblocking and schedules no display repaint.
- The authority owns two subscription slots (live trace plus reserved fixture), startup
  `CacheThenRefresh` admission and five-minute freshness/delivery. Removal, stale owner keys, foreign
  providers and older generations cannot mutate current delivery state. Closure retries preserve the
  original startup expiry; successful-sample identity protects the one-time result bypass through
  cache delivery, same-tick completion and an unseen success followed by failure.
- Battery now publishes a queryable latest-state Watch with provider/generation, attempt revision,
  health, last-attempt time, successful-sample time/revision and percentage. UI polling peeks once,
  retains the last valid trace level on failure and never replaces the unknown value with a fabricated
  zero sample. `BATTERY_DELIVER` retains its existing prefix and adds health and timestamps; health
  transitions are logged separately. The real driver's once-per-failure diagnostics remain unchanged.
- Demand and two-credit observe-now ingress are separate from correlated panel control. A host-testable
  `BatteryRuntime` wraps the actual provider: current demand is reread before acquisition, compatible
  arrivals reuse conversion completion, and expiry releases credits during retry backoff. Empty demand
  idles; admitted one-shots may finish after withdrawal. No extra two-request queue allowance exists.
- Suspension closes admission before awaiting, discards queued/pending requests before acknowledgement,
  and retries cleanup for every current suspend. Stale completion/resume cannot reopen admission;
  timeout/drop retains intent. Revision exhaustion marks the actor terminal inside `step`, before
  post-step control, while remaining responsive to cleanup. The shared PCAL expander wake and real
  fuel-gauge I/O stay in the provider and never wait for the UI task to mediate hardware access.
- Exact ESP32 payloads: new state Watch **88**, single-field demand Watch **48**, ingress **120** =
  **256 internal static bytes**; authority **240 external bytes**. The existing control remains **44**.
  Actor state without the hardware driver is **168**, including successful revision and transport
  references; demand/receiver/sender add **32**. Complete measured battery contract is **740 bytes**
  (**500 internal + 240 external**), within the prior **804-byte** generic provider envelope. This
  includes existing objects, excludes real driver/future/task/allocator overhead, and is not linked
  growth. Keep the planning envelope until actual fixtures and workloads are qualified.

| Final release image | `.data` | `.data.wifi` | `.bss` | CPU0 `.stack` | Delta vs E-0040 |
| --- | ---: | ---: | ---: | ---: | --- |
| Inkplate BLE | 16816 | 1872 | 72196 | 40188 | 224 fewer stack bytes |
| Inkplate default | 17000 | 540 | 63900 | 115156 | 256 fewer stack bytes |
| Medinote default, including BLE | 49600 | 284 | 204096 | 54368 | Unchanged |

- Battery pool falls **664→632** in both Inkplate profiles; display **3064** and SD **1456** stay fixed.
  BLE's 14 pools total **18424**, default **15752**. Battery poll entry frames are **304→352 BLE /
  368→352 default**; display is **704→784 BLE / 416 unchanged default**. Default's new battery delivery
  callee has a **304-byte** entry frame. Permanent boot subtotals remain **128 BLE / 112 default**.
  These individual frames and linked reservations do not establish connected-path or runtime peaks.
- BLE keeps **6288 above the unchanged 33900 floor**, board pool **72/72**, image **1815872/3661824**,
  and IRAM references **45/78**. Internal `dram2` remains **113736**. No linker, heap capacity, BLE role,
  draw buffer or hardware driver changed. The rebuilt Medinote ELF is byte-identical to its baseline.
- Validation passes: **77 observation** and **208 touch-core** tests, including **26 combined presentation**
  and **13 battery runtime/driver/control** cases; the original five real-driver fault tests remain.
  New cases cover persistent navigation, owner/identity rejection, bounded startup refresh, real BQ
  acquisition arrivals, full queues, withdrawal, expiry/backoff, delayed/stale cleanup, fresh suspend
  retry, suspend during wake and terminal resume. Counter exhaustion uses the actual actor transition;
  it does not simulate billions of acquisitions. Shared core counter tests remain separate evidence.
- Registered observation/touch-replay strict lint, Inkplate default/BLE/minimal releases and all-features
  Clippy, Medinote release, ownership/panel/BLE/stack-risk/reachability/include guards and scoped Markdown
  links pass. Default release is restored after the minimal build. Initial builds exposed two unused
  re-exports; they were removed and final builds retain only existing compiler/dependency warnings.
  Scoped analysis reports eight valid units, max SLOC **339**, cognitive/cyclomatic/arguments **13/16/7**,
  with no new offender. Independent review found no actionable defect in final authority/runtime wiring.
- Plan and RAM reference are current; the plan is 219 lines. Next: complete fixture-facing metadata in
  the other providers' lean projections, implement real fixture consumers/scenario, then qualify both
  boards. No flash, restored physical battery capture, expander fault injection, sleep/display/SD/upload
  qualification or runtime high-water measurement ran. Earlier placement and full-baseline gaps remain.
- Artifacts: `logs/typed-observation-subscriptions_e0041_20260828/` preserves the E-0040 source/image
  baseline, final source and images, target payload probes, linked sections/pools/frames, review and
  validation evidence. Unrelated dirty work is preserved; no commit was made.


### E-0042 — Headless battery fixture and UI handoff

- 2026-08-28. Updated the proposed [shared UI plan](shared-ui-coordination.md#typed-observation-provider-handoff)
  to the implemented E-0039–E-0041 authority: exact committed surface tokens, independent checked
  observation epochs, retained-wake renewal and persistent trace delivery. The coordinator proposal
  remains Proposed. Existing provider fixtures stay at the authority boundary; coordinator overlay,
  rollback and refresh integration remain that plan's later work.
- Implemented the first device fixture slice: `OBSFIX BATTERY <id> <validity_ms>` enters the existing
  UI-owned battery authority and shared two-credit observe-now ingress. It installs no periodic
  subscription, changes no screen or five-minute trace demand, and creates no task or reply queue.
  One polled mailbox and one pending session are bounded separately. The serial path sets absolute
  expiry before enqueue; the existing five-minute validity cap is not a conversion-latency promise.
- Mailbox and authority reject zero/stale IDs and preserve checked owner generations. A new ID is
  consumed even on Busy rejection. Admission reports Full/Closed/Expired explicitly; no automatic
  retry occurs. The provider owns outstanding request credits and expiry wakes, including when UI
  polling stops. An already-started conversion may finish into cache after the request expires.
- `RESULT` carries provider/fields, owner generation, admission/expiry, state revision/health,
  attempt/sample timestamps, successful revision and baseline identity. Sampled requires a newer
  successful revision in the same provider generation and a completion timestamp within the request
  window; failure-only revisions cannot qualify. Closure cancels first, restart is explicit, and
  delayed UI polling may report an earlier in-window completion. Terminal reporting consumes the session.
- Added `hostctl test observation-fixture` and `observation-fixture.sw.yaml`: live PING, one request,
  exact-ID result, metadata validation, UART capture and a distinct `.report.json` file. Workflow YAML
  owns ordering and failure/report flow. Timeout preserves uncertainty and sends no retry/cancellation.
  The [run guide](../guides/troubleshooting.md#headless-battery-observation) documents validity, correlation
  and disconnect behavior. Review fixed Busy-ID reuse, weak host baseline validation and log/report
  filename collision before final validation.

Resources and software evidence:

| Image | `.data` / `.data.wifi` / `.bss` | Linked stack | Task-pool total |
| --- | --- | ---: | ---: |
| Inkplate BLE | 16816 / 1872 / 72244 | 40140 (6240 above unchanged 33900 floor) | 18432 |
| Inkplate default | 16976 / 540 / 63956 | 115132 | 15760 |
| Medinote default, BLE retained | 49600 / 284 / 204096 | 54368 | 23776 |

- Exact ESP32 payload probe: mailbox **40 internal bytes**, fixture core **72 external bytes**;
  authority **240→312**. The **112 persistent bytes** fit the old 128 aggregate model, but placement
  changes from 72 internal + 56 external to 40 + 72. Substitution into the three-provider Inkplate
  model gives **2524 bytes (1636 internal + 888 external)**; S3's proposed fixture remains unimplemented.
  The **112-byte result is transient**, not additional static storage. Book, Watches, ingress capacity,
  driver timing, linker, heap/draw-buffer capacities and BLE roles are unchanged.
- Linked stack falls **48 BLE / 24 default bytes**. Both serial pools grow **3000→3008**; battery632,
  display3064 and other pools stay unchanged. BLE display poll entry **784→768**, default416;
  fixture service and result formatting each add a **304-byte callee frame**, and the default UI
  battery-delivery helper grows **304→352**. Permanent boot frames remain128 BLE/112 default. Nested frame sizes are
  not runtime high-water or full-path bounds. The rebuilt S3 ELF is byte-identical to E-0041.
- **587 host tests pass:** observation77, touch-core226 (including44 presentation tests), hostctl284.
  Strict observation/touch-replay/hostctl lint, Inkplate default/minimal/BLE release and all-features
  Clippy, S3 release, BLE image/IRAM/floor, ownership/panel/network/BLE/stack guards, tracked-source
  reachability, include-usage, scoped formatting/analysis and changed-document links pass.
  Compiler warnings remain. This is not the complete software-baseline gate.
- Provenance: ignored `logs/typed-observation-subscriptions_e0042_20260828/` contains before/final source
  manifests and scoped patch, saved ELFs and hashes, type-probe source/commands, layout/frame metrics,
  logs and budget audit. The ledger dashboard now reflects current implementation rather than E-0035.
- No flash or hardware run. Next: run the identified battery image on device, complete metadata in
  BME688/Medinote projections, then extend provider/periodic-demand fixtures with independent expiry
  restoration, counters and panel/upload/sleep operations through their owners. E-0037/E-0038 placement,
  both-board fault recovery, runtime resource/latency limits and full qualification remain open.


### E-0043 — Inkplate battery device qualification and serial readiness

- 2026-08-28, Inkplate 4 TEMPERA / ESP32-D0WD-V3 v3.1, MAC `e8:6b:ea:fb:d5:54`,
  `/dev/cu.usbserial-2030`. The ESP32-S3 at `/dev/cu.usbmodem20101` was untouched. Initial
  `espflash board-info` timed out; bounded esptool identification succeeded. No hardware cause is
  inferred from the first timeout or empty passive read.
- Verified the saved E-0042 ELF and its scoped source manifest before flashing. Canonical
  `scripts/device/flash.sh ble-release` used the explicit ELF, full production layout and 60000ms
  boot capture. All four written regions verified; no fallback. ELF SHA-256
  `a6a05a5ba5dc200e19541ef0bc49f4b6ce09b5079ce62273ad1a7687e83e73ee`, application SHA-256
  `1a0e20b80a4877f27f3493f930d9fb8ea1ab77c3e0441f4b2b6c23e3d040b2ee`.
  Boot loaded `ota_0` at `0x80000`, reached `RUNTIME_READY`, then emitted
  `FIRMWARE_CONFIRM slot=ota_0 state=valid`. Flash workflow time synchronization finished `ok`.
  Explicit-image metadata leaves build settings unverified; the hash links this run to E-0042's
  saved build/layout evidence. This is not a clean-source BLE promotion claim.
- First fixture attach observed a fresh application boot between serial sessions: its initial PING
  arrived before runtime readiness, leaving the workflow waiting without sending an observation.
  Interrupted that attempt and preserved it. Exact electrical/reset-line causality was not measured.
- Fixed the host workflow to wait for either live PONG or `RUNTIME_READY`, then conditionally confirm
  with one more read-only PING. YAML owns this branch; no fixed startup sleep or observation retry.
  Added `--observe-ms` to retain the same serial connection after a successful result. The report
  separates sample verification, completed capture and overall run success; observation duration is
  an operator window, not a provider timing guarantee. Firmware remained byte-identical to E-0042.
- The corrected workflow, with `--observe-ms 360000`, **passed**. Request
  `1787894905191453000`, provider2/fields1/owner-generation1: baseline successful revision3 →
  revision4, generation0, `health=Ok`, percent100. Admission7215ms, completion7228ms, expiry307166ms:
  **13ms observed admission-to-sample latency**, inside the original validity window. A compatible
  startup conversion may satisfy this request; it is not evidence of an exclusive fixture conversion.
- The continuous capture then recorded ordinary `BATTERY_DELIVER` revision5, successful sample at
  **307231ms**, **300003ms after** the fixture sample, same generation and `health=Ok`. This proves
  a new periodic acquisition and trace delivery, rather than reuse of the fixture's cached value.
  Four battery and four BME688 suspensions were all `Quiesced`; no acquisition/cleanup failure,
  panic/watchdog signature or post-fixture application reset appeared in the captured window.
  Cold-boot `TIME_SYNC` offers expired during passive observation; these are recorded separately
  from sensor faults, and the flash workflow's completed clock synchronization remains `ok`.
- Host changes: **286 hostctl tests and strict lint pass**, including boot-readiness/missing-PONG
  cases and continued capture on the same descriptor. Scoped formatting, three-unit code analysis,
  whitespace and changed-document links pass. No firmware rebuild was needed; RAM/image evidence
  stays E-0042. No full software baseline, injected sensor fault, sleep, upload/active-BLE workload,
  physical panel inspection or runtime stack/heap high-water qualification is claimed.
- Evidence: `logs/typed-observation-subscriptions_e0043_20260828/` contains canonical `flash/`,
  the interrupted first attempt, `battery-fixture-qualified.log` and its `.report.json`,
  `device-evidence.json`, the read-only analyzer, preflight/source/image identities and host checks.
  Final normalized fixture capture is **663972 bytes**; workflow raw post-result observation is
  **658982 bytes**. They cover different portions/normalization and are not interchangeable counts.
- The plan and [hardware matrix](../reference/hardware-test-matrix.md#2j-typed-observation-subscriptions----real-provider-delivery-manual)
  now record this healthy battery path. Next: complete BME688/SHTC3/ADC metadata, extend fixtures,
  then qualify fault recovery, periodic-demand restoration and both-target workload/resource limits.
  E-0037/E-0038 placement and broader qualification gates remain open; no phase becomes Passed here.


### E-0044 — Provider metadata and BME688 fixture

- 2026-08-28. Completed provider identity, last-attempt timestamp and successful-sample identity in
  BME688/SHTC3/ADC projections. Production projection helpers preserve the last successful revision
  and timestamp across failed attempts; BME688 tracks successful revision separately in its task.
  Its Home entry bypass now recognizes a new conversion in the same millisecond. Provider identity
  is checked before Home delivery/admission uses state. Units, correction and driver timing are unchanged.
- Extracted the battery fixture into a shared typed core and protocol. `OBSFIX BATTERY` remains
  compatible; `OBSFIX BME688` enters the existing environment authority and requests both fields.
  One 40-byte command mailbox routes by provider and keeps a shared monotonic ID watermark. Each
  authority owns one bounded session using its existing two-credit ingress. BME688 service runs even
  without Home; no periodic subscription, screen change, task, reply queue or extra request credit.
- Shared completion checks preserve absolute expiry, successful baseline advancement, provider
  generation, cleanup epoch and terminal rejection. A failure-only revision cannot qualify; a prior
  in-window success retained through a later failure can. `hostctl test observation-fixture --provider
  bme688` checks provider1/fields3 and both raw values; battery remains the default. YAML still owns
  readiness, submission, result, capture and failure ordering; no observation retry is added.
- Extended existing host suites with BME688 no-Home/demand preservation, rejection/expiry/restart/
  cancellation, retained success, equal-timestamp entry, shared-mailbox routing, both Medinote
  projection helpers and wrong-provider rejection. The full Medinote run exposed seven older generic
  test cases that installed environment identity for battery values; corrected their setup to use the
  supplied state's provider, preserving the production identity check. The failing run is retained.

Resources, relative to E-0042/E-0043:

| Image | `.data` / `.data.wifi` / `.bss` | Linked stack | Task pools |
| --- | --- | ---: | ---: |
| Inkplate BLE | 16880 / 1872 / 72212 | 40108 (−32; 6208 above unchanged 33900 floor) | 18440 (+8) |
| Inkplate default | 17080 / 540 / 63916 | 115060 (−72) | 15768 (+8) |
| Medinote default, BLE retained | 49624 / 284 / 204096 | 54336 (−32) | 23776 (unchanged) |

- Exact ESP32 object probes: shared mailbox40; each typed fixture core72; BME688 authority272→336
  external (the changed entry baseline saves8); battery authority312 unchanged. Result112/120 bytes
  is transient. Both fixture sessions plus mailbox total184 persistent bytes (40 internal +144 external).
  The extended three-provider model is2596 (1636 internal +960 external), including existing objects;
  S3's proposed one-session model remains2528 internal. Full projections were already modeled, so
  their growth is not added twice. Drivers, futures/task headers and allocator overhead remain excluded.
- BME688 state/Watch40/72→64/96 and pool976→984. S3 probes: both state/Watch64/96 (from48/80), Home
  authority488 unchanged. Inkplate display/serial/battery pools stay3064/3008/632. Final BLE
  display/BME688 poll frames832/944; default416/1344. Formatting entry frames are256 plus callees;
  permanent boot/spawn return boundaries remain128 BLE/112 default. These are not runtime peaks.
  No linker, heap/draw-buffer capacity, DMA placement, BLE role/security or fixed stack floor changed.
- **700 host tests pass:** observation77, touch-core231, Medinote105 and hostctl287.
  Inkplate default/minimal/BLE releases, all-features Clippy and Medinote release build. Strict host
  lint, 23-unit scoped analysis/formatting, BLE image/floor, ownership/panel/network/BLE/stack guards,
  tracked-source reachability and include usage pass. This is not the full software-baseline gate.
- The initial candidate booted and confirmed valid. Final source analysis required helper extraction
  to keep the argument metric within its gate; the changed ELF was rebuilt, measured and flashed
  again before qualification. `flash-initial/` retains the first run; final `flash/` is the evidence
  below. Final ELF SHA-256 `38b094702cce4589f33d3f544c688a12d06d4215c8ef6b6603d8cf9f6491db05`,
  app SHA-256 `4664a9a9472ae13efadebeccca860c3d4f727785f0702580614599cda9c9ec81`.
- Identified Inkplate ESP32-D0WD-V3 v3.1, MAC `e8:6b:ea:fb:d5:54`, `/dev/cu.usbserial-2030`;
  the Medinote `/dev/cu.usbmodem20101` was untouched. Canonical full production flash verified four
  written regions, with no fallback; final boot reached `RUNTIME_READY`, confirmed `ota_0` valid and
  completed clock synchronization. Explicit-image metadata alone does not verify feature settings;
  the saved source/build/image manifests establish this dirty-checkout BLE candidate's provenance.
- First final-image BME688 request `1787896437583223000`: provider1/fields3, owner-generation1, generation0,
  baseline successful revision2→3, `health=Ok`, raw temperature3162centidegrees and humidity38737
  millipercent. Admission7233ms, sample7264ms, expiry307184ms: observed31ms admission-to-sample.
  Compatible in-flight conversion may satisfy the request; this is not exclusive-conversion evidence.
- The first six-minute workflow **passed its fresh-sample/capture checks**, but the subsequent
  five-minute delivery reused revision3. Its still-fresh cache consumed that delivery interval just
  before acquisition was due; the separate periodic-evidence check correctly failed. This was not
  counted as a new reading. No scheduling policy or guessed early-acquisition margin was introduced.
- A second capture covered two cadence intervals (`--observe-ms 660000`). Request
  `1787896846868756000` advanced baseline3→4, generation0/owner-generation1, `health=Ok`:
  admission7681ms, sample8985ms, expiry307681ms, observed latency1304ms; raw temperature3178
  centidegrees/humidity38727millipercent. The first cadence delivered that fixture sample; a later
  `BME688_DELIVER` carried successful revision5, sampled at309002ms, **300017ms after** the fixture
  sample, raw temperature3186centidegrees/humidity38732millipercent and `health=Ok`.
  This proves periodic acquisition and eventual delivery, not a fresh result at every cadence tick.
- The completed 11-minute capture **passes** the separate device analyzer: final ELF matches,
  fixture and subsequent sample identities advance, all eight BME688/eight battery suspensions are
  Quiesced, and no sensor/cleanup failure, panic/watchdog or post-fixture reset is recorded. Passive
  cold-boot clock offers timed out; final flash-workflow synchronization itself passed. Normal battery
  periodic delivery also continued. No injected faults, sleep/upload/active-BLE workload, physical
  panel inspection or runtime stack/heap high-water qualification is claimed.
- The default battery fixture also **passed on the final image**: request `1787897540852110000`,
  baseline3→4, generation0/owner-generation1, admission7235ms, sample7248ms, expiry307185ms,
  percent100 and `health=Ok` (observed13ms). This confirms the shared implementation preserves the
  existing command path; it does not add another battery fault-recovery qualification.
- Evidence: `logs/typed-observation-subscriptions_e0044_20260828/` preserves both canonical flashes,
  source/image/toolchain manifests, exact target probes and section/pool/frame measurements, all test
  attempts, `bme-fixture.log`/`bme-six-minute-evidence.json`, `bme-two-cadences.log`/`device-evidence.json`
  and `battery-regression.log`, with separate host reports. The final BME capture is1183490 bytes.
  Both-target builds retain BLE; the default Inkplate build is restored locally, while the final BLE
  candidate remains installed. No commit was made and unrelated dirty work was preserved.
- Plan, RAM reference, run guide, UI handoff and hardware matrix are current. Next: Medinote SHTC3/ADC
  fixtures, periodic-demand restoration, then declared latency/resource and fault/load qualification.
  E-0037/E-0038 placements, physical display speed, runtime peaks and the full baseline remain open;
  the cached cadence result remains evidence of delivery-phase lag. No phase becomes Passed here.


## E-0045 — Medinote SHTC3 and ADC fixtures

- Date: 2026-08-28
- Scope: shared one-shot session/evidence policy; Medinote product fixtures, bounded runtime USB
  commands, hostctl provider kinds, two-board build/resource comparison and S3 device qualification.
- Result: SHTC3 and ADC fresh fixtures and later periodic acquisition/delivery pass on identified
  production firmware. No phase is Passed; periodic-demand replacement/restoration is next.

### Implementation and authority

Moved the existing generic fixture session and result formatter into `platform/observation::fixture`.
Products choose typed fields, provider/owner IDs, values and validity; existing tests preserve Inkplate
semantics. Its mailbox, two-credit ingress and external authorities remain unchanged. Exhaustion
coverage moved from the Inkplate integration test to the shared core's unit test.

Medinote embeds two 72-byte sessions beside Home. Home uses owner 1, fixtures owner 0; committed entry,
withdrawal and rebuild do not replace fixture sessions or change their demand. Target code services
both fixtures on every UI tick, before returning for an absent Home key. Existing Watch state is read
once per provider; admission, expiry, close epoch, provider restart and successful revision fences
remain in the shared policy. No new task, queue depth or receiver is added.

USB input uses the same UI owner as clock provisioning, then bounded incremental polling (64 bytes,
one complete command per tick). The 80-byte decoder retains partial lines, discards oversized lines
through the delimiter, and consumes a global increasing u64 ID watermark across both providers.
`PING`, `OBSFIX SHTC3`, and `OBSFIX ADC` require no screen navigation. Runtime readiness follows the
initial clock provisioning opportunity. Validity remains 1–300000ms of eligibility, independent of
one-minute Home cadence; no scheduling lead time or arbitrary early-sampling margin was introduced.

Hostctl adds `--provider shtc3|adc`, keeps YAML orchestration and one serial owner, and checks product
units as well as IDs/fields. SHTC3 uses corrected millicelsius/signed millipercent; ADC millivolts/percent.
BME688's unsigned humidity parsing is preserved. Reports include `provider_kind`, since the two
products reuse numeric provider IDs. Home sample logs now distinguish successful revision/time from
attempt metadata. BLE roles/security/pairing, LVGL placement, sleep policy and sampling cadence stay intact.

### Exact resource comparison

All artifacts below are under `logs/typed-observation-subscriptions_e0045_20260828/`. The dirty
checkout is based on `27b091b9fef1b530c520afa2383dca387bed09e1`; before/final source manifests,
scoped/whole-tree patches, compiler identities, probe commands/dependency hashes and saved ELFs
record this slice separately from prior changes. No commit or staging was performed.

| Image | ELF SHA-256 | .data / .data.wifi / .bss | Stack | Task pools |
| --- | --- | ---: | ---: | ---: |
| Inkplate BLE | `1dd933b5f5259609dfb54b3caea0c946d5efef213f1a4b17ce0111aa254654c4` | 16880 / 1872 / 72212 | 40108 | 18440 |
| Inkplate default | `e9c1bc4c6ec6a3d2f0866a71835c866cf350dbb5d6cf1ae2d172028237b1e5f0` | 17160 / 540 / 63932 | 114964 | 15768 |
| Medinote default BLE | `25e54b8b7ad9adc8648fe808da5a106a71714a9f532dcf401ae497e8ad8bec74` | 49816 / 284 / 204304 | 53936 | 24000 |

Relative to E-0044: BLE reservation unchanged (6208 above the unchanged 33900 floor); default spends 96,
S3 spends 400 linked internal bytes. S3 Home authority 488→632 plus console 80 accounts for 224 persistent
bytes, UI pool 2712→2936; provider pools 888/480 and Watches 96 each remain unchanged. Both S3 results
are 120 transient bytes. No extra heap allocation/PSRAM use; the three-provider S3 planning model is
2624 internal bytes, not an incremental reservation. Inkplate object probes remain mailbox 40 internal,
cores 72 each external, authorities 312/336 external, results 112/120 transient. See the
[RAM budget](../reference/dram/dram-budget.md#medinote-fixture-storage-and-shared-core-e-0045).

Poll entry frames: BLE/default UI 896/416 (BLE +64), S3 UI 896 (−64), SHTC3 368 (unchanged), ADC 336 (+16).
S3 observation polling adds a 512-byte callee and result formatting 256; hourglass tick remains 24752.
These are static function-entry frames, not connected call-chain bounds or runtime high-water.
The initial S3 ELF 72c5e303… preceded the final build; only the full hash above was flashed and qualified.
Inkplate was not reflashed: E-0044's device evidence remains tied to its prior installed image.

### Verification and physical evidence

- Host suites: observation 78, touch-core 230, Medinote 113 and hostctl 288 pass (709 tests total).
  Final hostctl full run is `scripts/host-test.sh test hostctl -- -- --test-threads=1`; its 14 fixture tests
  also pass normally. A parallel full rerun failed the already-recorded
  `direct_mode_repeated_transport_reset_retries_below_fallback_streak_limit` test (287/288), while an
  earlier parallel run and final serial runs passed. Preserve the failure log; no upload code was changed.
- Strict observation/Medinote/touch-replay/hostctl lint; Inkplate BLE/default/minimal builds and
  all-features Clippy; locked S3 build; BLE floor/controller, IRAM, include/reachability, network owner,
  panel bus, stack-risk and UI ownership guards pass. Default root release artifact is restored.
- Scoped formatting and six-document link checks pass; source analysis passes except the existing
  S3 `runtime_ui.rs` parser gap.
  The tool misparses the whole file as a function with SLOC 5 both before/after; its reported
  cognitive/cyclomatic/arguments change 54/28/31→72/33/32 and are not valid function metrics. This
  coverage gap remains open, not waived or counted as a clean analysis/full baseline.
- Board-info identifies ESP32-S3 revision 0.2, MAC `a4:cb:8f:d0:6a:74`, USB `/dev/cu.usbmodem20101`,
  physical flash 16MiB. The unchanged target partition/flash workflow remains its configured layout.
  Canonical `targets/medinote-waveshare/flash.sh` succeeds, with verified clock sync and runtime-ready;
  `flash/firmware.elf` matches the qualified hash. Both captures retain BLE initialization through Ready.
- SHTC3 request 1787898527416732000: baseline 2→3, admitted 1943ms, sample 2018ms, **75ms**, health Ok,
  temperature 26021mC/humidity 47126mpermil. Later Home delivery reports revision 4 sampled 62091ms,
  **60073ms after fixture**. ADC request 1787898734637389000: baseline 2→3, admitted 1943ms,
  sample 1945ms, **2ms**, health Ok, **4116mV/99%**. Later delivery revision 4 sampled 61945ms,
  **60000ms after fixture**. Each hostctl capture retains the descriptor for 150000ms after success.
- Each first cadence delivery reuses its fixture/cached value. A later delivery advances successful
  identity; this qualifies eventual periodic acquisition/delivery, not fresh data at every tick.
  Each serial reopen records USB_UART_CHIP_RESET and an invalid-clock boot offer without host clock
  reply; each also logs one SHTC3 acquisition timeout before recovery and the fixture result. These
  are observed startup faults, not a diagnosed cause or controlled fault-recovery qualification.
  Post-result windows have no acquisition/cleanup fault, panic/watchdog or application reset.

Evidence: `flash/`, `shtc3-fixture.log`, `adc-fixture.log`, their JSON reports, `device-evidence.json`,
`analyze-device.py`, `*-metrics.json`, `probe-sizes.json`, `medinote-probe-sizes.json`, build/test/guard
logs and artifact manifests. A hostctl pass alone does not prove periodic delivery; the analyzer
separately checks successful revision, provider generation, >=60000ms sample separation and faults.
Physical non-Home operation, sleep/cleanup/ADC fault recovery, active BLE/load/upload, panel appearance,
allocation failure, latency bounds and runtime stack/heap peaks remain unqualified. No full software
baseline or whole-repository documentation-link pass is claimed. Next: bounded periodic-demand
fixtures with restoration on every exit, including expiry while the UI is stalled.


## E-0046 — Bounded periodic demand overrides

- Date: 2026-08-28
- Result: Implemented for all four providers; host restoration tests and two-board builds pass.
  Physical qualification is scoped below. No phase is marked Passed.

### Ownership and bounds

`platform/observation::periodic::DemandControl` retains live demand separately from an optional
expiring override. Products publish only live demand; the existing UI authority admits/cancels one
fixture per provider. The provider applies an override between acquisitions, includes expiry in
its idle/backoff wake deadline, and restores the latest live demand on expiry, correlated cancellation,
control or terminal stop. A conversion already selected may finish first. Every control entry clears
the lease before awaiting cleanup; periodic admission stays closed while parked and reopens only
when the running provider resolves demand. Terminal stop cannot reopen it.

`OBSPER <provider> <id> <interval_ms> <validity_ms>` and original-ID `CANCEL` share the strict grammar
and existing serial transports. New IDs remain monotonic across providers; cancellation cannot
rewind their watermark or remove a successor. One-shot and periodic fixtures are mutually exclusive,
while ordinary live requests retain existing credits. Intervals 60000–300000ms match the current
production range; maximum validity 900000ms bounds a diagnostic lease to three slow cycles.
These are explicit diagnostic limits, not predicted acquisition times or early-sampling margins.

Provider logs distinguish APPLIED, real successful in-window SAMPLE and terminal RESULT, including
restored live fields/ages. Hostctl's new YAML selects expiry or one cancellation after a bounded
sample count. It requires two advancing sample identities/cadence plus matching restoration, retains
batched lines, writes a separate report on failure, and never retries. No UI timer, new task, reply
queue, BLE role change, linker change, LVGL relocation or Wi-Fi/upload tuning was introduced.

### Exact resource evidence

Artifacts: `logs/typed-observation-subscriptions_e0046_20260828/`. Before/final source snapshots,
SHA-256 manifests and the scoped patch distinguish this slice from the dirty checkout based on
`27b091b9fef1b530c520afa2383dca387bed09e1`. No commit or staging was performed.

| Image | ELF SHA-256 | .data / .data.wifi / .bss | Stack | Pools |
| --- | --- | ---: | ---: | ---: |
| Inkplate BLE | `49fb5f293b5a855511e411f9c80d8c0619a9fbe1159156c91d97a14f18dc7d9c` | 17008 / 1872 / 72244 | 39948 | 18472 |
| Inkplate default | `32f204e5731f45ab785cd51ae98fde0696ea14226b834312308d22a5a398dbb3` | 17264 / 540 / 63956 | 114844 | 15800 |
| Medinote final BLE | `c445ce8caca30596e9d6fd59305f66335be01d8b9297b4061e07bfad8447ea64` | 49920 / 284 / 204352 | 53792 | 24032 |

Compared with E-0045, linked stack spends 160 BLE / 120 default / 144 S3 bytes. Inkplate BLE remains
6048 above its unchanged 33900 floor. Demand controllers measure 104 bytes for BQ and 112 for each
two-field provider, replacing 48/56-byte Watches. Optional active window 32 replaces receiver handle 16;
provider pools grow 16 each. Mailbox 40→48; authorities, state Watches and one-shot sessions stay unchanged.
The three-provider planning model becomes 2820 (1860 internal + 960 external) Inkplate / 2840 internal S3.
Model totals include existing payloads and are not linked deltas. Correct-version MCU probes, symbols,
call frames and RAM-kind distinctions are in the [budget](../reference/dram/dram-budget.md#periodic-demand-storage-e-0046).
Runtime stack/heap peaks and the existing S3 hourglass 24752 frame remain unqualified.

### Verification

- Observation 90, Medinote 114, touch-core 231 and hostctl 292 tests pass (727 total). Hostctl full run
  uses `scripts/host-test.sh test hostctl -- -- --test-threads=1`; no parallel full-baseline claim.
  New cases cover autonomous expiry, latest-live withdrawal/replacement, completion/backoff boundaries,
  control parking/terminal stop, stale cancellation, wake retention, command limits, fixture exclusion,
  global ID watermark, malformed evidence, batched UART and expiry/cancel YAML branches.
- Strict observation/Medinote/touch-replay/hostctl lint passes after fixing an obsolete import and
  extending authority tests to check the new pending state. Initial lint failures are retained.
  Inkplate BLE/default/minimal builds and all-features Clippy pass; locked S3 build and canonical
  final flash build pass. Default root release remains restored.
- BLE floor/controller, IRAM, include/reachability, network owner, panel bus, stack-risk and UI ownership
  guards pass. Scoped function metrics/formatting and documentation links are checked separately.
  The pre-existing S3 `runtime_ui.rs` parser gap and broader baseline/link issues remain open;
  this slice does not claim a whole-repository software baseline.

### Physical evidence and remaining qualification

Both boards were reidentified: Inkplate ESP32 rev3.1 MAC `e8:6b:ea:fb:d5:54`; Waveshare ESP32-S3 rev0.2
MAC `a4:cb:8f:d0:6a:74`. Canonical flash workflows succeeded, and S3 clock provisioning passed.
The first S3 image `1d4374743212c8bea22388e299c2756b90102a201cbb9473e7139ebbc53eb271` was used for SHTC3;
the final image above removes an unused import and updates a demand comment, with identical measured
sections/pools. Captures/reports retain these image distinctions.

SHTC3 expiry passed: request 1787900523831436000 applied 1946ms, successful revisions 3/4 at 61946/122019ms
(60073ms separation), and Restored at 151945ms, exactly its expiry. Live fields 3/ages 60000 were retained.
One startup `Acquisition(TimedOut)` preceded application and recovery; it is not a controlled fault test.

Inkplate battery and BME688 attempts both failed the uninterrupted periodic gate with explicit
`Closed` results during normal panel suspension. Battery request 1787900524821995000 applied 7238ms,
sampled 7260ms, closed 10200ms; BME688 request 1787900612836308000 applied 7242ms, sampled 7281ms,
closed 7371ms. These captures exercise control-exit restoration, not two-sample expiry/cancellation.
Both failed reports are retained; no retry, fixed startup delay or suspension bypass was added.
Uninterrupted Inkplate qualification needs lifecycle-owner orchestration that accounts for panel
refresh. Physical UI-stall expiry, navigation/sleep/fault recovery, active BLE/upload coexistence,
visuals, allocation failure and resource/latency bounds remain open.


ADC cancellation passed on the final S3 image: request 1787900792533369000 applied 1945ms with a 90000ms
interval over live 60000ms demand. Revisions 3/4 completed 91783/181783ms (exactly 90000ms apart), then
Cancelled at 181857ms, before expiry 241944ms. The retained 75000ms capture contains Home revision 5
sampled 241783ms, exactly 60000ms after the last fixture sample. Thus the post-cancel live acquisition
cadence is observed, not inferred from the terminal metadata. Each S3 capture reached BLE Ready;
that is initialization evidence only. Both had one startup SHTC3 timeout before application and no
post-application acquisition/cleanup fault, panic/watchdog or application reset.

Evidence: `flash-s3/`, `flash-s3-final/`, Inkplate `capture.log` and flash artifacts, four
`*-periodic.log` files and their JSON reports, `device-evidence.json`, `analyze-device.py`,
`*-metrics.json`, object probes/dependency hashes, `test-matrix.json`, `build-matrix.json`, guard,
scoped-analysis, formatting and documentation logs. All eight changed document link checks pass;
Markdown length remains advisory (the active observation plan is 220 lines). Next: lifecycle-owner
qualification of Inkplate periodic interruption/restoration, then physical sleep/fault and resource limits.


## E-0047 — Correlated Inkplate panel-cycle qualification

- Date: 2026-08-28
- Result: Both Inkplate panel-cycle/recovery gates pass on the final image. Device runs exposed
  and drove fixes for duplicate-cache delivery and host round-trip ordering. Startup I2C incidents
  recovered before preflight but remain an open investigation; the whole plan is not complete.

### Implementation and acceptance

`REPAINT <nonzero-u64-id>` carries a correlation through the existing eight-entry AppEvent channel
and UI/panel owner. Bare REPAINT/REFRESH retain their behavior. Immediate REPAINT OK still means
queued; diagnostic BEGIN/CONTROL/LIVE/END evidence describes execution. The trace reports exact
current BME688/BQ control IDs/acknowledgements, aggregate client success, latest live demand and
sample identity. A control snapshot reads request and acknowledgement under one lock. No control
publication, cleanup order, panel exclusion, channel capacity, periodic policy or BLE role was bypassed.
Correlated END requires both successful refresh and successful resume waits; upload refuses it.

Hostctl `--panel-cycle` selects `observation-panel-cycle.sw.yaml`, separately from the uninterrupted
periodic gate. After a correlated preflight repaint, the combined `OBSPER ... REPAINT` command uses
the existing mailbox and UI event queue. The UI admits the override, waits for exact-ID provider
application, and repaints before returning to ordinary rendering. A retained state getter plus one
notification signal prevents stale/coalesced wakes from acknowledging another request. The original
absolute fixture expiry bounds this UI wait, including queue time. Hostctl checks the matching
Closed/control/restoration sequence and waits for a new live delivery, without observe-now or retry.
A closure before the requested repaint cannot pass. All controls must be acknowledged, resume IDs must
advance, the override must be absent, and restored demand must match the closure's latest live demand.
The fresh sample must advance successful identity without a provider restart; cached delivery cannot pass.
Each wait is bounded by the operator's host deadline, with no guessed startup delay.

### Delivery race found by the device gate

On the initial image, battery request 1787904825742351000 passed: Closed at 14985ms within repaint
14765–17610ms, control IDs 9→10, restored age 300000ms and fresh revision 4→5 at 310029ms.
BME688 request 1787905143673243000 completed the same control/restoration checks, but failed the
420000ms fresh-delivery wait. At about 310050ms, the UI redelivered revision 3 sampled at 10050ms.
The cached reading could not pass the new gate; its failed report and capture are retained.

Both Inkplate UI adapters could redeliver an unchanged revision at the interval boundary, spending
another delivery interval immediately before the due conversion completed. Two production-adapter
host regressions reproduced this before the fix. The adapters now withhold an already-delivered
revision unless health changes or an admitted entry result is satisfied. This reuses existing
tracking, preserves normal limits for newer periodic samples, and leaves the generic core's cache
policy and both providers' acquisition/control logic unchanged. No timing margin or host retry was added.

The first post-fix BME688 run (request 1787905790947446000, BLE image `5a2f981a08266ac41cfc3943a7055464a1cfd3d0e1eb5545d7be9fad7fc678a6`) exposed a separate host round-trip race:
the fixture applied at 12830ms and closed during normal refresh at 12953ms, before the requested
repaint began at 13360ms. The strict gate rejected it. That failure motivated the combined UI-owner
operation above; no retry, startup delay or relaxed closure attribution was used. Host cases now
require the single combined command and exercise application-before-wait, delayed application,
stale notifications, wrong IDs, closure, expiry and mailbox ownership.

### Exact image and resource evidence

Artifacts: `logs/typed-observation-subscriptions_e0047_20260828/`. Before/final manifests and scoped
patches preserve this slice separately from the dirty checkout based on `27b091b9`. Nothing was staged
or committed. Initial qualification images, before the delivery-race fix:

| Image | ELF SHA-256 | .data / .data.wifi / .bss | Stack | Pools |
| --- | --- | ---: | ---: | ---: |
| Inkplate BLE | `cd93af0f7b9c0fd7dfab3c29877d9fb64521c5677e16b3a9fe09aa4619ca6876` | 17008 / 1872 / 72316 | 39876 | 18472 |
| Inkplate default | `f5066d3d41abfb80ee20380c3dd9ff7b20e0eefaca17dddf6f0c3086624b27e3` | 17168 / 540 / 64028 | 114868 | 15800 |

The MCU AppEvent is 16 bytes; the linked eight-entry channel grows 96→168 (+72 internal bytes).
Task pools, provider controllers, ingress/fixture storage and capacities are unchanged. No new task,
mailbox, reply queue or heap allocation was added. BLE spends 72 linked stack bytes, retaining 5976
above the unchanged 33900 floor. The initial default image recovers 24 linked bytes through `.data` layout changes.
BLE/default UI poll entry frames are 848/64; traced repaint callees 320/96 demonstrate why a small entry
frame is not a connected-call or runtime-high-water bound. BLE SD remains 2144. Medinote source and
resource layout are unchanged from E-0046; no new S3 qualification is claimed.

Intermediate images after the adapter fix, before the combined owner operation:

| Image | ELF SHA-256 | .data / .data.wifi / .bss | Stack | Pools |
| --- | --- | ---: | ---: | ---: |
| Inkplate BLE | `5a2f981a08266ac41cfc3943a7055464a1cfd3d0e1eb5545d7be9fad7fc678a6` | 17008 / 1872 / 72316 | 39876 | 18472 |
| Inkplate default | `73ffb00f74188788920abf423c9e8088e05b03eeb8c46e5667e5b2b56d4f95fd` | 17168 / 540 / 64036 | 114860 | 15800 |

BLE sections and both task-pool totals are unchanged by the fix. Default `.bss` grows eight bytes;
normalized named BSS symbol sizes are identical, identifying a layout delta rather than new source
storage. This intermediate default stack is 16 bytes above E-0046. Both UI poll entry frames remain 848/64.
`initial-*` artifacts and `flash/` preserve the first image; `flash-final/` identifies the replacement.

Final owner-operation images (626 host tests pass):

| Image | ELF SHA-256 | .data / .data.wifi / .bss | Stack | Pools |
| --- | --- | ---: | ---: | ---: |
| Inkplate BLE | `7271bf01abbf203c09381df364342faa39fd52a138dad5351a718bdf4d247db0` | 17016 / 1872 / 72332 | 39844 | 18472 |
| Inkplate default | `a33239589b6818eb10780046f1300fac08b1ed8c9275e16a67c82638212b20b3` | 17256 / 540 / 64036 | 114764 | 15800 |
| Medinote rebuild, not flashed | `e1de12c790936f07400b961d34e06028e0e32604763f27c3ad2b02b8e27d78ba` | 49920 / 284 / 204352 | 53792 | 24032 |

The application wake signal is 12 internal bytes; the mailbox remains 48 and AppEvent channel 168.
Task pools are unchanged. Final BLE stack retains 5944 above the unchanged 33900 floor. The final
BLE/default display poll entries are 64/64 and outlined bodies 832/400; BQ frames are 720/512 and
BME 1040/1392. These are static frames, not connected-path or runtime high-water bounds.
`delivery-fix-*` and `flash-final/` retain the intermediate images; `flash-owner/` holds the final flash.
The shared read-only method changes no Medinote state layout; its rebuilt sections and pools match
E-0046. Its device evidence still belongs to the prior flashed images.

### Verification and remaining device work

- Host suites: observation 90, touch-core 239 and hostctl 297 pass (626 tests); hostctl includes 23 fixture
  cases and its full run is serialized. Strict lint passes for these suites. New tests cover optional
  repaint IDs, snapshot coherence, both provider wire formats, batched UART, stale/failed acknowledgements,
  closure before the requested operation, pending/wrong live demand, cached/missing/failed/restarted
  recovery, unsupported option combinations and refused preflight sending no fixture.
- Inkplate BLE/default/minimal and Medinote release builds pass; Inkplate all-features Clippy passes.
  Default Inkplate release artifact is restored.
  BLE image/floor/controller, IRAM, include/reachability, network-owner, panel-bus, stack-risk and UI-owner
  guards pass. Scoped production function metrics/formatting and changed-document links are checked.
  Existing broader baseline/link and S3 parser-coverage issues remain open; no full baseline is claimed.
- Hardware: the prior `/dev/cu.usbserial-2030` disappeared. `/dev/cu.usbserial-2130` is visible but
  board-info timed out, and the first bounded passive attach produced no firmware text. A subsequent
  user reset produced three ROM `POWERON_RESET` / `DOWNLOAD_BOOT` / `waiting for download` sequences.
  Identification still failed with both default reset and `--before no-reset`; no competing serial owner
  was found afterward. USB reconnect did not fix default identification. Explicit `--chip esp32
  --no-stub` identification succeeded: ESP32 rev3.1, MAC `e8:6b:ea:fb:d5:54`, 4MB flash. The exact
  failing default-identification stage is not established. Connection logs are access evidence, not
  firmware lifecycle failures or successful qualification. All three canonical flashes succeeded,
  reached runtime-ready and completed time provisioning. Final qualification uses the owner-ordered
  workflow and the unchanged operator deadline.

Final BME688 request 1787906622824255000 passed on the owner-operation image: applied at 11659ms,
repaint 11668–14526ms, Closed at 11893ms, controls 9→10, live ages 300000ms, and fresh revision 3→4
at 310089ms (300019ms between successful samples). Final battery request 1787906943909027000
also passed: applied 10925ms, repaint 10934–13788ms, Closed 11157ms, controls 9→10, and fresh
revision 4→5 at 306976ms (300003ms between samples), retaining age 300000ms. Both final runs
had no post-admission acquisition/cleanup error, panic/watchdog or reset. Battery-run boot logged
BME688 and touch bootstrap `AcknowledgeCheckFailed(Unknown)` errors. Touch later reported acquisition-ready and BME688 delivered successful readings before preflight. These are
uncontrolled startup incidents, not injected-fault qualification; their cause remains unresolved.

Evidence: `*-panel-cycle*.log` and JSON reports, `device-evidence.json`, `flash-owner/`, saved
ELFs, final image/resource summaries, source manifests/scoped patch, regression failure and pass
logs, test/build matrices, guards and document checks. The two failed BME688 runs remain preserved.

Next: investigate the recovered startup I2C failures, then qualify sleep/fault recovery. Uninterrupted
periodic operation, UI-stall expiry, active BLE/upload contention, visual checks and runtime resource
bounds remain separate gates.

## E-0048 — Startup I2C transaction diagnostics and sleep scope

2026-08-28. Startup NACK cause remains **open**; this slice adds evidence at the missing transaction
boundary, not a speculative recovery fix. Dirty-checkout before/final snapshots, scoped patch, ELF
measurements, test logs and captures live in `logs/typed-observation-subscriptions_e0048_20260828/`.

### Investigation and implementation

- E-0047's battery panel-cycle capture reports BME688 and touch bootstrap
  `AcknowledgeCheckFailed(Unknown)`, then successful readings/touch before the fixture starts.
  Current `run_board_runtime` awaits core setup, BME688 initialization and touch bootstrap **before**
  starting core 1 or the IMU/display/provider tasks. Concurrent runtime providers therefore do not
  explain those bootstrap failures. The exact failed transaction was absent from the old logs.
- Inkplate uses registry `esp-hal 1.1.1`, not Medinote's S3 sleep patch. Its transaction paths reset
  the controller on error and clear interrupt/FIFO/command state before transmission. This source
  evidence does not establish an electrical cause or prove recovery on this incident. The ESP32
  `Unknown` NACK reason does not identify address versus data acknowledgement.
- `boards/inkplate-tempera/src/startup_i2c.rs` forwards the existing blocking HAL operations inside
  the same shared mutex. Its async adapter completes the blocking call without an extra suspension.
  On error, the product sink prints `I2C_STARTUP_ERROR` with timestamp, address, ordered operation
  descriptors, lengths, first write byte and unchanged HAL error. Descriptors describe the failed
  transaction, not the exact failing sub-operation. No returned sensor bytes are logged.
- `run_board_runtime` disables the sink under the same mutex before spawning runtime clients.
  Success is silent; provider failure diagnostics/backoff remain the runtime policy. No new probes,
  retries, startup waits, bus resets, provider controls or sensor configuration changes were added.
- Two unflashed async-wrapper drafts grew task pools by 288 and 184 bytes. Both were rejected and
  retained under `initial-diagnostics/` and `async-forwarding/`. Moving the synchronous interception
  below the async adapter removes that growth. This is diagnostic plumbing, not memory relocation.

### Image and software evidence

| Image | SHA256 | `.data / .data.wifi / .bss` | Linked stack | Task pools |
| --- | --- | --- | ---: | ---: |
| Inkplate BLE, flashed | `aaee034b2e8db20e400feecd2fffe0df52ee9aaf558a7f68ffb84d41b2c1e40a` | 17024 / 1872 / 72332 | 39844 | 18472 |
| Inkplate default | `883ec7505438b7df203a02d99fb223fd41a5d658e9843bdfd1669db3b2baf322` | 17216 / 540 / 64044 | 114804 | 15800 |

The bus static grows 72→80 bytes; linked alignment absorbs the BLE cost and default stack gains 40
bytes relative to E-0047. Every task pool is unchanged. BLE stays 5944 bytes above the unchanged
33900 floor; internal DRAM2 remains 113736 bytes, with no heap/LVGL/DMA/PSRAM placement change.
The startup diagnostic sink has a 208-byte entry frame on both images; BLE's returning initialization
frame is 5120 bytes (+16). These are static frame measurements, not runtime peaks. Medinote was
neither changed nor rebuilt/flashed by this slice; its last physical image remains E-0046.

- Host tests: board **51**, touch-core **239**, observation **90** passed (380 total).
  New cases cover exact combined-transaction forwarding, error preservation, one report/no retry,
  success silence, disabled runtime tracing and single-poll async completion.
- Board strict Clippy, Inkplate BLE/default/minimal builds and all-features Clippy passed.
  Default root release artifact is restored. Lockfile updates only add the board's direct
  `embedded-hal` dependency; no dependency version changes were requested.
- Scoped Rust format/RCA and include, source-reachability, network-owner, panel-bus, stack-risk and
  UI-owner guards passed. This is not a claim that the unresolved whole-repository baseline passed.

### Physical evidence and remaining work

- Rediscovered Inkplate at `/dev/cu.usbserial-8310`; explicit no-stub identification confirms ESP32
  revision 3.1, 4 MB flash, MAC `e8:6b:ea:fb:d5:54`. Earlier all-port/USB enumeration had shown only
  Medinote; a later reconnect exposed the USB serial bridge at the new location.
- Three pre-flash E-0047 reset captures all initialized BME688/touch and reached `RUNTIME_READY`.
  Canonical production flash of the image above and clock synchronization succeeded. Its flash
  capture and first three 15-second reset captures also initialized both devices and reached readiness,
  with no acquisition failure, panic or watchdog in those windows.
- The intended ten-cycle diagnostic soak did **not** pass: cycles 4–6 contain only espflash connection
  timeouts, and cycle 7 was interrupted before firmware evidence. The run was stopped instead of
  counting absent markers as sensor failures or continuing blind retries. Retain all partial/failed
  logs; three observed clean resets are bounded evidence, not a startup reliability estimate.
- No spontaneous startup NACK was reproduced in these captures. The error formatting/forwarding path
  is host-tested; its actual on-device error emission and the original fault cause remain unqualified.
- Sleep scope is target-specific: Medinote has UI-owned light/deep-sleep entry, a current physical
  BOOT trigger selecting deep sleep, and a 15-second timer wake. Inkplate has no sleep entry path.
  Do not add Inkplate sleep as an incidental observation test. Medinote still needs an owner-routed
  fixture adapter for automated sleep/rejection acceptance; a physical BOOT smoke is supplemental.
- A no-sync/no-stub normal reset and passive espflash attach restored readable Inkplate runtime
  telemetry afterward. The raw-cat attach produced baud-corrupted bytes and is retained but excluded
  from firmware evidence. No flash or sensor workaround was needed to regain that stream.
- Medinote manual sleep capture (`medinote-sleep.log`) used the unchanged E-0046 image
  `c445ce8caca30596e9d6fd59305f66335be01d8b9297b4061e07bfad8447ea64`, last flashed to the same
  USB-identified MAC `a4:cb:8f:d0:6a:74`. Both providers acknowledged current request **1** as
  `Quiesced`, `accepted=true`, before `DEEP_SLEEP_ENTER`. Wake logs report
  `RESET reason=Some(CoreDeepSleep) wake=Timer`, then Home and `RUNTIME_READY`.
  Battery produced successful revisions **1/2** at **1672/6711ms** after reset; environment produced
  revisions **1/2** at **1783/6784ms**. Pre-sleep both had reached revision 128. Deep sleep resets
  generation/counters; these are new-runtime samples, not same-generation continuation evidence.
- The operator reports pressing KEY around wake. Current deep-sleep source enables only the timer,
  and both reset/wake records identify Timer; retain the interaction rather than call this an
  untouched timer trial. **Normal provider quiescence and post-deep-sleep sampling passed for one
  manual cycle.** Hands-off duration, light-sleep retention, fixture closure/restoration, cleanup
  rejection, injected faults and automated owner-routed sleep acceptance remain open.

Next: locate a real startup failure with the added transaction context, then fix its established
cause. Add the Medinote UI-owner sleep fixture and controlled cleanup-rejection case before claiming
automated sleep acceptance; retain the separate Inkplate panel-fault and both-target resource gates.

## E-0049 — Automated Medinote sleep and cleanup rejection

2026-08-28. **Three automated device cases passed:** environment cleanup rejection, battery cleanup
rejection, then normal deep sleep with timer wake. This closes the missing UI-owner sleep adapter;
physical faults, active fixture closure/restoration, light sleep and loaded resource gates remain open.
Before/final source snapshots, scoped patch, saved ELF, measurements, failed/passing captures and
reports are in `logs/typed-observation-subscriptions_e0049_20260828/` (dirty checkout, unchanged HEAD
`27b091b9fef1b530c520afa2383dca387bed09e1`). No Inkplate firmware change or flash in this slice.

### Ownership and protocol

- `OBSSLEEP <id> <validity_ms> DEEP|REJECT_ENV|REJECT_ADC` uses the existing bounded Medinote
  console and shared monotonic ID watermark. One pending UI-local command carries its original
  expiry; eligibility is checked before dispatch and again before actual sleep. Invalid/stale commands
  cannot start a power action. No extra task, channel, heap allocation or RTC-retained identity.
- The single UI owner navigates to Home, withdraws observation demand, flushes the existing sleep
  card, and invokes the existing deep-sleep path. Normal BOOT and the fixture share the same helper.
  Both ingresses close before either waiter is polled; the shared two-second cleanup deadline remains.
  The same UI recovery helper rebuilds Home after deep/light-sleep rejection and renews observation
  ownership after publishing both resumes. No second power owner or Inkplate sleep path was added.
- Synthetic rejection is scoped to the exact provider-control request. The selected provider performs
  **real cleanup first**, then converts a successful acknowledgement into `CleanupFailed` and logs
  `OBSERVATION_CLEANUP_INJECTED`. It remains suspended until resume. The next request is normal;
  this is not a physical I2C/ADC fault or an injected driver-cleanup failure.
- `hostctl test observation-sleep` runs a Serverless Workflow YAML: readiness, single submission,
  transition validation, deep-sleep disconnect/reconnect, fresh-sample validation, report/failure.
  Reconnect uses only the explicitly selected port. No sleep command retry, fixed startup delay,
  fixture sample request or relaxed timeout was used to obtain a pass. Operator validity/wait limits
  bound eligibility and capture, not firmware latency. See the [run guide](../guides/troubleshooting.md#medinote-sleep-and-cleanup-rejection).
- The gate correlates both accepted current cleanup IDs, exact injected outcome, expiry and event
  order. Rejection requires newer resume IDs, no sleep/reset, and newer successful samples in both
  existing provider generations. Deep sleep requires actual entry, exactly one `CoreDeepSleep / Timer`
  reset, readiness and successful samples from both new runtime providers. Button presses, wrong
  identities, acquisition errors and cached-only recovery cannot pass.

### Image, storage and software evidence

Canonical S3 flash completed, including verified clock provisioning, on `/dev/cu.usbmodem21101`,
USB-identified MAC `a4:cb:8f:d0:6a:74`. Installed ELF SHA256:
`a664a3c13f9d0a3fccdb71b759804a7ef2e6ae53f79512d039a8b1b0a4c064ea`.

| Measurement | E-0047 S3 | E-0049 S3 |
| --- | ---: | ---: |
| `.data / .data.wifi / .bss` | 49920 / 284 / 204352 | 49960 / 284 / 204400 |
| Linked CPU stack reservation | 53792 | 53696 |
| All task pools / UI pool | 24032 / 2936 | 24080 / 2984 |
| SHTC3 / ADC / BLE pools | 904 / 496 / 19696 | 904 / 496 / 19696 |
| UI poll entry / Hourglass tick frames | 912 / 24752 | 1040 / 24752 |

The control statics remain 40 bytes each; the per-request rejection flag fits their existing layout.
The 96-byte linked stack cost is 40 data, 48 task storage and 8 alignment bytes. BLE central/security/
pairing, internal heap/LVGL/draw buffers and uninitialized PSRAM remain unchanged. These are linked
reservations/static frames, not measured runtime peaks. Inkplate retains E-0048's separate budget.

- Host suites passed: Medinote **115**, touch-core **240**, observation **90**, hostctl **301**
  (**746 total**, without double-counting focused runs). New regressions cover parser bounds/shared
  watermark, real-cleanup-before-injection, one-request failure lifetime, fresh/restarted/cached sample
  identities, incorrect wake/acknowledgement, and YAML reconnect/failure paths with one submission.
  Existing serial-console tests also pass. Medinote, touch-replay and hostctl strict Clippy and S3 release build pass.
- Initial full hostctl run retained the previously recorded transport-reset fallback test failure
  (**300 pass / 1 fail**). The final whole-suite run passed **301/301** without changing upload code;
  the intermittent transport-test issue is not declared fixed.
- Rust formatting and include/reachability, network-owner, panel-bus, stack-risk and UI-owner guards
  pass. All parsed named function metrics in this slice meet thresholds. Scoped RCA still reports
  the existing S3 UI parser gap: the whole file is misidentified as a function with 5 SLOC and negative
  blank lines. Before/final analyses are retained; its cognitive maximum falls 72→63 and cyclomatic
  remains 33 after extracting real sleep-transition/Home-recovery responsibilities. The aggregate
  argument count is not a valid per-function result. No whole-repository baseline pass is claimed.

### Device evidence and limits

The first run (`reject-environment/`) failed **before submission**: the existing serial attach explicitly
lowered DTR/RTS and the S3 reported `CoreUsbUart / Undefined`; the initial PING was lost. This boot also
logged one uncontrolled `SHTC3_ACQUIRE_FAILED error=Acquisition(TimedOut)`, followed by recovery.
That startup fault is retained separately and is not synthetic-rejection evidence. The new workflow
now uses a passive attach that preserves DTR and does not write RTS. Existing workflows keep their
prior attachment behavior. Both rejection captures attach without reset; deep-sleep reconnect preserves
the actual timer wake. OS/driver behavior on other hosts remains unqualified.

| Case / request ID | Control evidence | Fresh environment / battery evidence |
| --- | --- | --- |
| Reject environment / 1787909736373262000 | Cleanup 1: environment injected failure, battery quiescent; resume 2; END 108117ms; no reset | Both revision 3→4; sampled 108217 / 108144ms |
| Reject battery / 1787909759046252000 | Cleanup 3: battery injected failure, environment quiescent; resume 4; END 130784ms; no reset | Both revision 4→5; sampled 130884 / 130812ms |
| Deep / 1787909766857625000 | Both current cleanup 5 quiescent; ENTER 138503ms; actual timer-only sleep; one CoreDeepSleep/Timer reset | New-runtime generation 0, revision 1; sampled 1784 / 1674ms after reset |

All three final windows are free of button-press, acquisition/cleanup error, panic and unexpected-reset
markers. The deliberate injected acknowledgement is expected only in its selected rejection case.
The deep gate records readiness and new samples; it does not measure timer accuracy, electrical sleep
current, display retention/speed, wall-clock retention or continuous runtime stack/heap high-water.
No active periodic fixture or connected BLE workload was armed. Rejection followed by clean sleep on
the same image demonstrates that the one-request injection did not leak into the next control.

Next: exercise active periodic-fixture closure and latest-live-demand restoration through this owner
on rejection, then qualify physical faults, light sleep and loaded resource bounds. Locate the original
Inkplate startup failure with E-0048 transaction diagnostics before choosing a recovery change.

## E-0050 — Active periodic fixtures across rejected sleep

2026-08-28. Active periodic overrides are exercised through the E-0049 sleep owner without changing
provider scheduling or cleanup. Dirty-checkout snapshots, scoped patch, exact ELF, resource measurements,
software checks and device captures are in `logs/typed-observation-subscriptions_e0050_20260828/`.

### Implementation and contract

- Both existing provider tasks already call `DemandControl::end` at the settled control boundary,
  before parking in cleanup. This slice adds read-only `OBSSLEEP DEMAND` traces after the UI publishes
  both resumes and renews Home demand. There is no new authority, provider state, mailbox or allocator.
- `hostctl test observation-sleep --period-ms ...` prepares both providers using the original OBSPER
  protocol and the two IDs immediately preceding the sleep ID. YAML waits for each exact application,
  then sends one existing rejection request. Preparation failure sends no sleep; any armed bounded
  lease expires normally. No retries, host cancellation or host refresh command is sent after rejection.
- Both leases must close inside this sleep's control window, before their current acknowledgements,
  with live Home demand withdrawn. After rejection, both traces must show no pending override and
  the current two-field, one-minute Home demand. The existing exact cleanup/rejection/no-reset checks
  still apply. A different lease, cancellation, ordinary expiry or successor cannot substitute for closure.
- A fresh Home-entry result is only the first checkpoint. A later successful sample must keep provider
  generation, advance successful revision, and have an acquisition timestamp at least one Home interval
  later, sooner than the old override interval and before the old expiry. Cached UI delivery is ignored.
  Deep mode is refused with periodic restoration enabled; it resets the runtime instead of resuming it.
- The run uses a 180000ms override, 600000ms eligibility and 240000ms host qualification window.
  These are explicit operator limits within existing fixture bounds, not guessed firmware lead times.
  Host time includes preparation and restoration; sample timing is distinguished from log arrival.
  See the [run guide](../guides/troubleshooting.md#medinote-sleep-and-cleanup-rejection).

### Image and software evidence

Canonical flash and verified clock provisioning succeeded on Medinote `/dev/cu.usbmodem21101`,
USB-identified MAC `a4:cb:8f:d0:6a:74`. Installed default-release ELF SHA256:
`dcee03a165349cce10ac860809ce88435475e3deb7024233a98f4ad2bd2c02fb`.
No Inkplate source, image or device changes were made.

- S3 `.data/.data.wifi/.bss/.stack`: **49968/284/204400/53696**. Eight additional data bytes fit
  existing alignment; linked stack and all **24080** task-pool bytes are unchanged from E-0049.
  UI/SHTC3/ADC/BLE pools remain **2984/904/496/19696**. No heap, LVGL, draw-buffer, PSRAM or linker change.
- The terminal sleep-report entry frame grows **96→160** bytes; UI poll and Hourglass tick remain
  **1040/24752**. BLE central/security/pairing are retained. These are static measurements, not runtime
  headroom or peak-usage proof. Host and Espressif toolchain identities accompany the saved ELF.
- Host suites: touch-core **241**, observation **90**, hostctl **305**, **636 total**. The new provider/
  control regression closes an applied lease, waits for real cleanup before synthetic rejection, changes
  live field/cadence policy while parked, and verifies the resumed scheduler uses that latest policy.
  Validator regressions reject missing/wrong closure, leaked overrides, wrong demand, cached samples,
  expiry-based recovery, invalid setup and deep-mode mixing. YAML cases require both applications
  before sleep and prevent sleep/retry after preparation failure.
- Hostctl/touch-replay strict Clippy, S3 release build, scoped Rust format/RCA, and include/reachability,
  network-owner, panel-bus, stack-risk and UI-owner guards pass. The preexisting whole-repository
  linker/link/analysis issues remain open; the S3 runtime UI file was not modified in this slice.

### Device results

**Both device cases passed** on the same installed image, with both leases active in each run.
The first case rejects environment cleanup acknowledgement; the second rejects battery. Each selected
acknowledgement is synthetic after real cleanup; both providers remain in generation 0.

| Case / sleep ID | Closed environment / battery | Later acquisition environment / battery | Interval from entry sample environment / battery |
| --- | --- | --- | --- |
| reject-environment / 1787910688652177000 | 65037 / 65036ms | 125298 / 125156ms | 60074 / 60004ms |
| reject-battery / 1787910892737090000 | 268969 / 268968ms | 329232 / 329086ms | 60075 / 60002ms |

Both captures contain current accepted cleanup IDs, newer resumes, no reset/button press/acquisition
failure, no pending override and restored 60000ms live demand. The first uses cleanup/resume IDs 1→2,
the second 3→4. No OBSPER sample appears after either Closed result. Original eligibility extends
well past the recovery samples, excluding expiry as the cause of restoration.

Environment delivery first logged a cached entry value at the next one-minute UI tick. The gate
waited until a later delivery carried the new successful sample identity and timestamp; it did not
count the cached line as acquisition. These cases qualify lease closure and periodic recovery,
not UI delivery latency, in-flight conversion cancellation, electrical faults, connected BLE load,
visual screen correctness or continuous runtime memory peaks.


Next: declare the runtime resource workload/limits and measure with BLE connected, then qualify
physical faults and loaded recovery. Light sleep, in-flight/partial cleanup faults, prolonged/UI-stall
qualification and the original startup faults remain open. E-0049's deep-timer gate is earlier-image
proof, not a new deep-sleep run on this image.

## E-0051 — Subscription scope and proportional validation

2026-08-28. Documentation-only amendment requested by the user while firmware and workloads are
still evolving. This supersedes earlier requirements to finish general resource qualification before
closing subscriptions, including E-0050's proposed connected-BLE measurement step. No firmware,
hostctl implementation, image, device state or historical evidence changed; no phase is newly Passed.

- Keep relevant tests/builds per change, bounded subscription/request storage and existing safety
  gates, including the unchanged Inkplate BLE stack floor. Compare linked RAM, task pools and
  significant frames on affected targets when storage/task/call-path changes warrant it; distinguish
  internal RAM, stack, heap and external PSRAM. Investigate regressions rather than invent spare margins.
- Defer whole-firmware stack/heap high-water, connected-BLE capacity and sustained workload
  qualification until a representative milestone or a concrete fault/headroom regression requires it.
  These are not subscription completion gates. No separate resource plan is created now.
- Keep targeted checks for placement, allocation and ownership changes. E-0037/E-0038 SD/UI placement
  validation remains outstanding before landing those changes; deferral neither proves their safety
  nor waives that validation. Existing baseline/toolchain/analysis/transport issues remain recorded
  separately; this scope amendment does not turn failed or missing checks into passes.
- Refresh the phase table through E-0050: Medinote deep timer wake, both cleanup rejections and active
  periodic-lease restoration have device evidence, with the image/coverage limits recorded above.
  Inkplate sleep is not an observation deliverable. Fault, expiry, recovery and targeted coexistence
  remain subscription work; general memory qualification is not required after each step.

**Next implementation slice: Inkplate rejected-panel subscription restoration.** E-0047's host
validator in `tools/hostctl/src/workflows/observation_fixture/lifecycle.rs` requires `Completed` and
successful quiesce/resume acknowledgements. Its normal-cycle proof does not establish restoration
after a rejected panel entry with an active periodic override. Extend the existing fixture/YAML flow
without weakening that normal gate: one case each for BME688 and BQ27441, one-request synthetic
cleanup rejection after real cleanup, through the existing UI/panel owner. Require the exact current
failed acknowledgement, no waveform start by that operation, override closure, newer resumes, latest
live demand and fresh periodic acquisition without a host refresh request; then require a normal repaint to succeed.
Keep synthetic acknowledgement evidence distinct from driver/electrical fault coverage.

The original startup NACK remains unresolved: use E-0048 transaction diagnostics on recurrence and
choose a fix only from established evidence. Do not repeat blind reset/injection loops or treat an
unreadable serial connection as a sensor failure. No new hardware run occurred in this amendment.

Validation: both documents' 27 links pass the offline check; whitespace/local-path checks pass.
Historical E-0001–E-0050 content is unchanged. Plan length is 222 lines (advisory above 220).
