# BLE Foundation and Upload Transport Implementation Ledger

- Status: Active
- Last-reviewed: 2026-08-14
- Started: 2026-08-11
- Plan: [BLE foundation and upload transport](ble-foundation-plan.md)
- Related decisions: [A/B firmware-update foundation](../architecture/0009-ab-firmware-update-foundation.md),
  [bounded BLE service foundation](../architecture/0011-bounded-ble-service-foundation.md)

## Ledger contract

This ledger has two content classes:

- Mutable summaries: Phase Status and Risk Register show current state. Every state change cites an
  append-only evidence or transition entry.
- Append-only records: baseline observations, decision history, evidence, deviations/failures, and
  transition history are never deleted or silently corrected. A correction gets a new ID and a
  `Supersedes` reference.

ADRs remain durable architecture authority. A phase can pass only when every required criterion has
exact evidence and each blocking risk's phase-local milestone is evidenced. A shared risk may remain
`Mitigating` for descendants after the current milestone passes; no current-phase blocker may depend
on a descendant criterion. Promotion closure is track-local: Phase 6 closes foundation milestones;
optional Stage C adds upload-delta milestones without reopening Phase 6 unless shared foundation code
or its release artifact changes; Phase 10 closes upload milestones. Each phase/risk pair in the status
table must have an explicit same-phase milestone in the risk row. Track closure requires resolution
or explicit acceptance by named authority.

## Phase status

| Phase | State | Prerequisites | Required criteria | Gate evidence | Blocking risks | Next action |
| --- | --- | --- | --- | --- | --- | --- |
| 0. Planning baseline | Passed | — | P0-A1–P0-A4 | E-0001–E-0004 | — | Complete. |
| 1. Source audit/fixed cost | Passed | P0 | P1-A1–P1-A6 | E-0005–E-0006, E-0008–E-0010 | R-001–R-003, R-012, R-019 | Complete for commit `77569d3`; reopen on source/dependency change. |
| 1R. Capacity reclamation | Conditional | Recoverable P1 shortfall | P1R-A1–P1R-A4 plus P1-A1–P1-A6 | — | R-002, R-003 | Only for a proved relocation path. |
| 1D. Runtime/shutdown feasibility | Blocked | P1, directly or after P1R | P1D-A1–P1D-A5 | E-0009 | R-003–R-004, R-012, R-018–R-019 | Await revalidated exact artifact; baseline runner exists, forced-race/largest-block lanes remain. |
| 1S. Exclusive radio handoff | In progress | P1; resident coexistence infeasible | P1S-A1–P1S-A5 | E-0010, F-0008, [CAP-0009](ble-phase1s-capacity-recovery-ledger.md#cap-0009--formal-20-cycle-gate-passed-clean-no-capacity-candidate-needed), [CAP-0013](ble-phase1s-capacity-recovery-ledger.md#cap-0013--wi-fi-regression-gate-rerun-cap-0005s-sd-blocker-is-gone-a-different-unrelated-internal-memory-floor-failure-now-blocks-a-clean-pass), [CAP-0014](ble-phase1s-capacity-recovery-ledger.md#cap-0014--cap-0013-root-caused-a-monotonic-since-boot-low-water-register-sampled-over-a-longer-teardown-free-session-not-a-leak) | R-003–R-004, R-012, R-018–R-019 | Capacity (P1S-A4) passed clean on exact commit `9606e152...`. The Wi-Fi regression gate's SD-poisoning blocker (`HCTLUPLD.TMP`, see F-0008 update) is now fixed and verified; `discovery_debug`/`acceptance_1_cycle` pass clean on the fixed artifact. `acceptance_3_cycle` still fails reproducibly (3/3) on an internal-memory-floor violation; CAP-0014 root-caused it as expected order-statistics behavior of a monotonic since-boot diagnostic register under a longer, continuous-session workload (not a leak, and the coordinator's real BLE-admission gate — re-probed live, not read from that register — already clears with ~3x margin per CAP-0009). No safe first-party fix was found; whether the test's floor check itself is measuring the right thing for this workload is referred to the user (ADR-0011 requires human acceptance for any floor change). P1S-A4 remains open pending that decision. |
| 2. BLE architecture ADR | Blocked | P1D or P1S | P2-A1–P2-A5 | E-0007–E-0008 | R-005, R-013–R-014, R-017–R-018 | Await exact-artifact runtime feasibility. |
| 3. Coordinator/update lease | Blocked | P2 | P3-A1–P3-A5 | — | R-002–R-003, R-007A, R-017 | Await accepted ADR. |
| 4. Base diagnostic BLE | Blocked | P3 | P4-A1–P4-A5 | — | R-002–R-004, R-012, R-014 | Await coordinator proof. |
| 5. Device/macOS proof | Blocked | P4 | P5-A1–P5-A5 | — | R-002, R-003, R-004, R-005, R-006, R-007A, R-014, R-017 | First physical BLE phase. |
| 6. Foundation promotion | Blocked | P5 | P6-A1–P6-A3 | — | R-002, R-003, R-004, R-005, R-006, R-007A, R-011, R-014, R-017 | Permanent BLE terminal gate. |
| 7. Upload/security ADR | Blocked | P6 | P7-A1–P7-A3 | — | R-008 | Optional upload extension. |
| 8. Upload operation boundary | Blocked | P7 | P8-A1–P8-A6 | — | R-002–R-003, R-008, R-010, R-016 | No device BLE writes yet. |
| 9. GATT upload/macOS client | Blocked | P8 | P9-A1–P9-A5 | — | R-002, R-003, R-005, R-006, R-008–R-010, R-015 | Await bounded SD service. |
| 10. BLE upload promotion | Blocked | P9 | P10-A1–P10-A3 | — | R-002, R-003, R-005, R-006, R-008–R-011, R-015, R-016 | Independent upload gate. |
| 11. iOS foundation | Deferred | P6 | P11-A1–P11-A3 | — | iOS-only | Does not block P6 or P10. |
| 12. iOS upload | Deferred | P10, P11 | P12-A1–P12-A2 | — | iOS-only | Does not block macOS promotion. |

Allowed states are `Blocked`, `Conditional`, `Ready`, `In progress`, `Passed`, `Failed`,
`Needs revalidation`, and `Deferred`. A source, dependency, feature, criterion, or artifact change
moves affected passed phases to `Needs revalidation` and creates a transition entry.

## Baseline observations — append only

| ID | Observation | Evidence | Implication |
| --- | --- | --- | --- |
| B-001 | Target is original ESP32-WROVER-E on Inkplate 4 TEMPERA; hardware change is out of scope. | Hardware matrix; user direction | Use the existing dual-mode radio. |
| B-002 | Wi-Fi works and remains permanent. | Default features; user direction | Removing it is not a BLE-fit mitigation. |
| B-003 | `esp-radio` 1.0.0-beta.0 currently enables ESP32/Wi-Fi/logging/unstable, not BLE/coex. | `Cargo.toml` | BLE starts behind a non-default probe. |
| B-004 | `esp-radio` supplies a controller adapter, not a GATT host. | Locked source | Select and prove a host explicitly. |
| B-005 | Controller uses `bt-hci` 0.8; inspected `trouble-host` 0.6 uses 0.8 while 0.7 uses 0.9. | Local crate metadata | Interface match is only a candidate, not acceptance. |
| B-006 | Each slot is 2,031,616 bytes with a mandatory 131,072-byte reserve. | ADR-0009 | Image ceiling is 1,900,544 bytes. |
| B-007 | Accepted Phase 6 image is 1,855,360 bytes. | UI/app ledger E-0016 | Only 45,184 bytes remain beyond reserve. |
| B-008 | Phase 6 `.data/.bss/.stack/.dram2_uninit` is 15,804/69,420/110,836/104,392. | DRAM budget | Every BLE delta is measured. |
| B-009 | `.stack` is the remainder of always-full `dram_seg`; internal DRAM binds. | DRAM budget | Default pools/queues are not acceptable evidence. |
| B-010 | HTTP serializes bounded upload commands through the sole SD task. | Upload source | BLE must be an adapter to the same authority. |
| B-011 | The Proposed asset-upload plan did not ratify or implement BLE. | Related plan | This plan controls staged BLE execution. |
| B-012 | Prior feasibility did not establish low-power BLE lifecycle. | Prior assessment | Use a bounded window and measure each state. |
| B-013 | ESP32 BLE enables `RESERVE_DRAM=0x10000`, projecting Phase 6 pre-static `.stack` near 45,300. | `esp-hal` 1.1.1 build source; linker overlay | Hidden reservation is a Phase 1 gate. |
| B-014 | BTDM task stack consumes at least 4,112 release/10,256 debug internal bytes, plus task/controller allocations. | `esp-radio`/`esp-rtos` locked source | It is not visible as Embassy pool or linked static. |
| B-015 | Current VHCI RX allocates per packet into unbounded `VecDeque`; TX has unbounded busy waits. | `esp-radio` 1.0.0-beta.0 source | Current beta is compile/size probe only. |
| B-016 | ESP32 adapter forces `BTDM_MODEM_SLEEP_MODE_NONE`. | `esp-radio` source | Approved numeric power ceilings block device acceptance. |
| B-017 | Wi-Fi controller and net runner are long-lived. | `src/firmware/net/runtime.rs` | Serialized traffic is still coex-enabled dual stack. |

## Decision history — append only

| ID | Date | Status | Decision | Authority / supersedes |
| --- | --- | --- | --- | --- |
| D-001 | 2026-08-11 | Constraint | Compile BLE into both production images; control availability at runtime. | User direction |
| D-002 | 2026-08-11 | Constraint | Retain permanent Wi-Fi. | User direction |
| D-003 | 2026-08-11 | Constraint | Use BLE GATT, not Classic/SPP. | User direction; transport plan |
| D-004 | 2026-08-11 | Proposed | Initial availability is a time-bounded service window. | Pending Phase 2 ADR |
| D-005 | 2026-08-11 | Proposed | Separate controller, host, lifecycle, framing, operations, and FAT layers. | Pending Phase 2 ADR |
| D-006 | 2026-08-11 | Proposed | One base BLE task and base radio coordinator own lifecycle. | Pending Phase 2 ADR |
| D-007 | 2026-08-11 | Constraint | Preserve A/B map and `0x20000` reserve. | ADR-0009 |
| D-008 | 2026-08-11 | Gate | No storage mutation before an accepted security decision. | Plan |
| D-009 | 2026-08-11 | Superseded | Prefer serialized Wi-Fi/BLE ownership over concurrent coexistence. | Superseded by D-012 |
| D-010 | 2026-08-11 | Constraint | Prove BLE fixed cost before allocating remaining base capacity. | User direction |
| D-011 | 2026-08-11 | Constraint | Native macOS CoreBluetooth is the first and sole initial client gate; iOS is later. | User direction |
| D-012 | 2026-08-11 | Proposed | First policy is coex-enabled dual stack with serialized product traffic; full Wi-Fi teardown is separate. | Supersedes D-009; Phase 2 ADR |
| D-013 | 2026-08-11 | Gate | Security/privacy/session ADR precedes mutating GATT. | Agentic review; plan |
| D-014 | 2026-08-11 | Gate | macOS transport loss requires rediscovery, reauthorization, and device query; no stale CoreBluetooth objects. | Agentic review; plan |
| D-015 | 2026-08-11 | Gate | WWR obeys CoreBluetooth readiness plus absolute device credit; readable Status is authoritative. | Agentic review; plan |
| D-016 | 2026-08-11 | Gate | Normal v1 upload excludes `mkdir`/`remove`; destructive administration requires separate authority. | Agentic review; plan |
| D-017 | 2026-08-11 | Constraint | Promote minimal permanent BLE independently of optional upload; iOS blocks neither. | User goal; agentic sequencing review |
| D-018 | 2026-08-11 | Accepted | Own the narrow bounded `esp-radio` patch in this repository, pinned to the audited crates.io base. | User authorization; E-0006 |
| D-019 | 2026-08-11 | Accepted for probe | Use a dedicated size-optimized `ble-release` profile for the non-default candidate; leave ordinary production `release` unchanged until promotion. | Phase 1 containment; E-0006 |
| D-020 | 2026-08-11 | Proposed | Adopt ADR-0011's coordinator, bounded service window, serialized coex traffic, update precedence, privacy identity, resource floors, and promotion rule. | ADR-0011; E-0007; human acceptance pending |
| D-021 | 2026-08-11 | Proposed | Revise ADR-0011 around `OffConfirmed`, atomic update/traffic grants, controller-power identity, exact advertising/macOS contracts, and guarded power measurement. | Supersedes proposal D-020; E-0008; acceptance pending |
| D-022 | 2026-08-11 | Accepted for probe | Keep Phase 1D lifecycle execution behind explicit `BLEPROBE` serial admission and report baseline completion separately from the full Phase 1D gate. | E-0009; prevents non-vacuous race/largest-block gates from being implied by ordinary cycles |

## Risk register — mutable summary

| ID | Severity | State | Blocks | Risk | Resolution criterion | Disposition/evidence |
| --- | --- | --- | --- | --- | --- | --- |
| R-001 | P1 | Resolved | P1 | Exact controller/host incompatibility | P1: P1-A3–P1-A4 | Exact locked controller/host pair builds and passes strict Clippy; E-0006. |
| R-002 | P1 | Mitigating | P1, P1R, P3–P6, P8–P10 | A/B image ceiling | P1: P1-A6; P1R: P1R-A4; P3: P3-A5; P4: P4-A5; P5: P5-A2; P6: P6-A1; P8: P8-A6; P9: P9-A5; P10: P10-A1 | Corrected candidate leaves 69,488 bytes; durable-source repetition and descendant gates remain; E-0008. |
| R-003 | P1 | Mitigating | P1, P1R, P1D, P3–P6, P8–P10 | Hidden/static/runtime memory cost | P1: P1-A5; P1R: P1R-A4; P1D: P1D-A2/P1D-A4; P3: P3-A5; P4: P4-A5; P5: P5-A2; P6: P6-A2; P8: P8-A6; P9: P9-A5; P10: P10-A1 | Static inventory exists; adverse runtime forecast requires Phase 1D; E-0006/E-0008. |
| R-004 | P1 | Open | P1D, P4–P6 | Restartable teardown/resource release | P1D: P1D-A3/P1D-A4; P4: P4-A1; P5: P5-A1; P6: P6-A2 | — |
| R-005 | P1 | Open | P2, P5–P6, P9–P10 | No modem sleep/power cost | P2: P2-A3; P5: P5-A4; P6: P6-A2; P9: P9-A3/P9-A5; P10: P10-A1 | Guarded paired-baseline method drafted; human acceptance and device proof pending; E-0008. |
| R-006 | P1 | Open | P5–P6, P9–P10 | Native CoreBluetooth behavior | P5: P5-A1/P5-A5; P6: P6-A2; P9: P9-A1/P9-A4; P10: P10-A1 | — |
| R-007A | P1 | Open | P3, P5–P6 | Serialized handoff/rollback | P3: P3-A1/P3-A3; P5: P5-A3; P6: P6-A2 | — |
| R-007B | P2 | Open | Optional experiment only | Full Wi-Fi teardown/recreation viability | P2-A4 plus successor criteria | Non-blocking |
| R-008 | P1 | Open | P7–P10 | Trust, auth, confidentiality, privacy, credential lifecycle | P7: P7-A1/P7-A2; P8: P8-A1; P9: P9-A1/P9-A2; P10: P10-A1 | — |
| R-009 | P1 | Open | P9–P10 | Bounded WWR/credit/control progress | P9: P9-A1/P9-A2; P10: P10-A1 | — |
| R-010 | P1 | Open | P8–P10 | Session interruption/resume authority | P8: P8-A3/P8-A5; P9: P9-A2/P9-A4; P10: P10-A1 | — |
| R-011 | P1 | Open | P6, P10 | Whole-system regression | P6-A2, P10-A1 | — |
| R-012 | P1 | Mitigating | P1, P1D, P4 | Maintained bounded controller transport | P1: P1-A1–P1-A3; P1D: P1D-A3; P4: P4-A1/P4-A3 | Async TX and source-disabled callback-quiescence guards/builds pass with 3/3 cancellation tests; durable-source rebuild and device forced-race proof remain; E-0008–E-0009. |
| R-013 | P1 | Open | P2 | Numeric power budget not approved | P2-A3 | Explicit guarded ceilings/method await human acceptance; E-0008. |
| R-014 | P1 | Open | P2, P4–P6 | Diagnostic connection hold/flood and tracking exposure | P2: P2-A1/P2-A5; P4: P4-A3/P4-A4; P5: P5-A5; P6: P6-A2 | Revised bounded-denial/identity policy awaits acceptance; E-0008. |
| R-015 | P1 | Open | P9–P10 | Lost Status/readiness stall/stale macOS objects | P9: P9-A1/P9-A4; P10: P10-A1 | — |
| R-016 | P1 | Open | P8, P10 | FAT replacement crash consistency | P8: P8-A2/P8-A4; P10: P10-A1 | — |
| R-017 | P1 | Open | P2–P6 | BLE/update race during cache-disabled flash | P2: P2-A2; P3: P3-A2; P4: P4-A1; P5: P5-A5; P6: P6-A2 | Predecessor grant and two-second close rule drafted; acceptance/implementation remain; E-0007. |
| R-018 | P1 | Mitigating | P1D, P2 | Opaque allocation or nondeterministic callback shutdown makes the architecture infeasible | P1D: P1D-A2–P1D-A5 | Checked source contract and baseline analyzer exist; opaque allocation, largest block, and device determinism remain open; E-0008–E-0009. |
| R-019 | P1 | Open | P1, P1D | Phase-1 artifacts lack a durable reconstructible source identity | P1: P1-A3–P1-A6; P1D: P1D-A1 | Current inputs include dirty/untracked first-party and vendor sources; E-0008. |

Risk states are `Open`, `Mitigating`, `Resolved`, `Accepted`, or `Superseded`. Once a phase-local
milestone passes, a shared risk becomes `Mitigating` for its descendants. Accepting a blocker requires
named authority, rationale, ADR/evidence, and a review condition; severity alone does not decide
whether it blocks.

## Evidence entries — append only

### E-0001: Initial Phase 0 planning baseline

- Date/scope: 2026-08-11; read-only architecture/dependency inspection and initial documents.
- Source baseline: shared checkout; unrelated changes preserved; no firmware, dependency, lockfile,
  partition, build artifact, or device state changed.
- Result: initial layering, candidate versions, image/section baseline, and macOS-later sequencing were
  recorded. Metadata/interface inspection was not target compilation or runtime proof.
- Capacity: image 1,855,360; 45,184 beyond reserve; `.data/.bss/.stack/.dram2_uninit`
  15,804/69,420/110,836/104,392.
- Evidence boundary: no BLE code built, flashed, advertised, connected, power-tested, or client-tested.
- Historical validation: scoped links 5/5 and path/whitespace audits passed; then-current plan was 289
  lines and ledger 127. These counts are a historical snapshot, not the amended documents.
- Gate disposition: initial Phase 0 Pass; later reopened by E-0003.

### E-0002: macOS-first client sequencing

- Date/scope: 2026-08-11; plan/ledger sequencing only.
- Result: macOS became the initial harness/uploader and sole promotion client gate; iOS moved later.
- Evidence boundary: no firmware, client, artifact, or device state changed.
- Gate disposition: documentation result only.

### E-0003: Agentic review findings

- Date/scope: 2026-08-11; independent embedded feasibility, architecture sequencing, security/GATT,
  execution-ledger, and adversarial reviews of the initial plan/ledger.
- Criteria covered: P0-A1–P0-A3 review only.
- Result: confirmed blockers included hidden BTDM reservation/task allocation, unbounded VHCI RX/TX,
  missing numeric power gates, coordinator/update lease ordered too late, security after product GATT,
  hostile ATT/control starvation gaps, crash-consistency ambiguity, and promotion coupled to upload.
- Evidence boundary: source/document review only; no firmware or hardware execution.
- Gate disposition: reopened Phase 0 for amendment; no implementation phase passed.

### E-0004: Agentic fix pass and Phase 0 revalidation

- Date/scope: 2026-08-11; plan/ledger amendments only at source HEAD
  `803077a0816b191342f80c7ee1d2edaba6eafb1c`; scoped status contains these two untracked documents.
- Phase and criteria: Phase 0, P0-A1–P0-A4.
- Agentic result: embedded-feasibility, architecture-sequencing, security/GATT, execution-ledger, and
  adversarial findings were integrated. Focused re-reviews found the embedded gates clean, no
  impossible risk cycle, and no remaining P0/P1 adversarial loophole.
- Architecture result: foundation promotion ends at Phase 6 independently of optional upload;
  coordinator/update arbitration precedes hardware BLE; security precedes mutating GATT; macOS remains
  first and iOS remains non-blocking.
- Resource result: fixed `0x10000` BTDM reservation, hidden controller task/heap, unbounded current
  VHCI path, numeric runtime floors, power approval, per-firmware-phase capacity gates, and Phase 1R
  product regressions are explicit stop conditions.
- Source identity: plan SHA-256
  `4db703ae4151d20a65cd8cd1d4f72d6a5b3e9e097dfa1c041af65dd2f264b62e`; no firmware artifact exists
  because this was documentation-only.
- Validation: scoped Markdown links passed 6/6; whitespace/diff and absolute-local-path checks passed.
  Plan is 295 lines: above the 220-line advisory but below the 300-line high-attention threshold. The
  append-only ledger is exempt from the LOC advisory.
- Evidence boundary: no dependency, source, lockfile, firmware, macOS client, build, flash, BLE radio,
  power, SD mutation, power-cut, or physical-device behavior changed or passed.
- Gate disposition: Phase 0 Pass. Phase 1 is Ready and is the only next implementation phase.

### E-0005: Phase 1 maintained controller-transport audit

- Date/time and scope: 2026-08-11; Phase 1 source/dependency audit only.
- Criteria covered: P1-A1, P1-A2, and the source-identity portion of P1-A3.
- Evidence kind: source and registry inspection; no dependency or firmware changes.
- Project source identity: HEAD `803077a0816b191342f80c7ee1d2edaba6eafb1c`; scoped Phase 1
  documents remain untracked in the shared dirty checkout.
- Released controller identity: `esp-radio=1.0.0-beta.0`, crates.io checksum
  `0f25cc4e3ce27476b42c4a68943f10a92f9dec3c24bb001269958f0318fef02c`, `bt-hci=0.8`;
  crates.io still reports this as the latest release. Candidate host is `trouble-host=0.6.0` on
  `bt-hci=0.8`.
- Maintained-source identity: esp-rs/esp-hal `main` at
  `5c2672becc9f6161da65c329ef3593ed770af629` still labels `esp-radio` 1.0.0-beta.0 but moves to
  `bt-hci=0.9`; embassy-rs/trouble `main` at `088e09c451177d5db50cf3e58c68d05512265ba0`
  labels `trouble-host` 0.7.0 on `bt-hci=0.9`. All inspected packages are MIT/Apache-2.0 licensed.
- Toolchain/environment: `rustc 1.97.0-nightly (1.97.0.0)`, Cargo 1.97.0-nightly, target
  `xtensa-esp32-none-elf`. Commands included `cargo search/info`, `git ls-remote`, locked-source
  inspection, and current upstream raw-source inspection.
- P1-A1 result: **Fail** for both the released crate and maintained `main`. The ESP32 VHCI receive
  callback copies every packet into `Box<[u8]>` and appends it to an unbounded `VecDeque` in
  `esp-radio/src/ble/btdm.rs` and `esp-radio/src/ble/mod.rs`; no fixed overflow/backpressure policy
  exists.
- P1-A2 result: **Fail** for both source identities. `send_hci` polls send availability in an
  unbounded loop and then busy-waits on `PACKET_SENT` without yielding, awaiting, deadline, or fault
  transition; the async `bt-hci` transport calls this synchronous function directly.
- P1-A3 result: **Partial**. Exact release checksum and maintained repository revisions, feature/HCI
  relationship, licenses, and active maintenance were identified. No accepted bounded source exists,
  so advisory review, clean locked build, and promotable dependency pin were not attempted.
- P1-A4–P1-A6 result: **Not run**. The plan forbids the build/size probe until P1-A1/P1-A2 pass or an
  explicitly owned patch is authorized.
- Recovery preparation/result: no Cargo or firmware edit was made, so the Wi-Fi baseline remains
  untouched. Recovery requires either a maintained upstream fix or explicit authorization to carry a
  narrow repository-owned patch with fixed RX capacity/backpressure, yield/await TX progress, finite
  deadlines, source tests, and rebase responsibility.
- Evidence boundary: no target build, Clippy, image/section measurement, controller initialization,
  BLE radio, macOS client, power, or hardware result exists.
- Gate disposition: **Fail/Blocked** at `BLE-BOUND-01/02`; R-012 remains open. Await the patch-ownership
  decision before modifying dependencies.

### E-0006: Phase 1 bounded transport and fixed-cost probe

- Date/time and scope: 2026-08-11; authorized repository-owned controller patch, exact dependency
  integration, non-default diagnostic service, locked Xtensa builds, static resource inventory, and
  host-side validation.
- Phase and criteria covered: Phase 1, P1-A1–P1-A6. Supersedes E-0005's blocked disposition without
  deleting its failed-source evidence.
- Evidence kind: source, host, and build. No device or physical evidence.
- Project source identity: HEAD `803077a0816b191342f80c7ee1d2edaba6eafb1c`; shared checkout remains
  dirty with unrelated work preserved. Toolchain is `rustc 1.97.0-nightly
  (8ea53bcd7257011cbf96f6398551bdc650b04334)`, Cargo 1.97.0-nightly, target
  `xtensa-esp32-none-elf`.
- Controller identity: repository path `vendor/esp-radio-1.0.0-beta.0-bounded`, immutable crates.io
  base checksum `0f25cc4e3ce27476b42c4a68943f10a92f9dec3c24bb001269958f0318fef02c`, upstream
  revision `b4c8d9bc634373bc140df1c3c83ba42706a55944`, and patched crate-tree SHA-256
  `624f3cbca0d36e1b261515f1ac04a7b4166f69e5d0b788d32ca52b56a61893c0`.
- Host identity: exact `trouble-host=0.6.0` checksum
  `1df7817cead4b83dfbeeaa59736ecbc97c30b21dc3bfc0cc2ce8a6687f70b37a`; exactly one
  `bt-hci=0.8.1` checksum `211713d2e9fb4793ce4360a712c0764264aff6be48932ccf02ca2a331c0436a9`.
  Trouble default features and its default packet pool are disabled; selected features cap connection
  events and L2CAP RX/TX queues at 2, with one connection and a first-party four-by-64-byte packet pool.
- License/advisory result: all three selected packages declare MIT OR Apache-2.0. `cargo-audit 0.22.2`
  found no vulnerabilities in 262 locked packages. Its two allowed unmaintained warnings are existing
  `paste` via `esp-hal` and `proc-macro-error2` via `statig`, not new BLE paths.
- Bounded transport result: RX uses four fixed 259-byte packets, drops newest on full/oversize, and
  exposes overflow/oversize/high-water counters. TX rejects over-capacity assembly, yields while
  waiting, uses 100 ms availability/completion deadlines, propagates errors, and fault-latches timeout
  until controller reinitialization so a late callback cannot acknowledge a later packet.
- Guards: `scripts/ci/check_ble_controller_patch.sh` and `scripts/ci/check_ble_image_budget.sh`
  passed and are wired into the software baseline. A recursive diff against the
  immutable registry base showed only `src/ble/mod.rs`, `src/ble/btdm.rs`,
  `src/ble/controller/mod.rs`, and the patch manifest changed.
- Build commands: `scripts/build/build.sh release default`; `CARGO_FEATURES=ble-foundation
  scripts/build/build.sh ble-release default`; strict Clippy with `CARGO_FEATURES=ble-foundation
  scripts/build/build.sh clippy default`; all used `--locked`. The ordinary release profile remains at
  optimization level 3; only the named BLE candidate profile uses size optimization.
- Artifact identity: default ELF SHA-256
  `739bbcd6459aef4d6b58cda80bf94a24c177091ad26692ce896b692eeccace0e`, application SHA-256
  `e8d08044f4c8baf26d79ecc3c69cb05d5cebb29f37a19e25dbe39c6d513074cd`; BLE ELF SHA-256
  `01c7650280836e176ad013f7423f1d238b02211b48ffc5dd19a1e3f65a53fc93`, application SHA-256
  `8246a66c56fcfcfddf5cc798a8d11ca6cad76509adb103b296f90f15c471a6d6`.
- Linked/private inventory: named BLE statics total 5,341 bytes: task pool 1,424; `BT_STATE` 1,336;
  GATT server 1,116; host resources 600; host stack 328; HCI collector 268; packet slots 264; pool
  counter 4; fault latch 1. ESP32 Bluetooth separately reserves 65,536 bytes before linking. The BTDM
  task requires at least 4,112 release/10,256 debug internal-heap bytes plus its control block and
  opaque controller allocations. The selected Trouble production path uses fixed-capacity storage and
  the first-party static packet pool; controller heap high-water is deferred to the Phase 5 device
  floor, where it can actually be observed.
- Phase 2–6 capacity forecast: the 78,096-byte headroom allocates at most 16,384 bytes to coordinator
  integration, 24,576 to production GATT/lifecycle hardening, and 12,288 to promotion telemetry and
  integration, leaving 24,848 bytes unallocated. Borrowing between allocations requires an explicit
  ledger decision and every descendant phase still enforces the absolute 1,900,544-byte ceiling.
- Recovery preparation/result: BLE remains non-default and was not flashed. Removing the feature from
  the build excludes the probe; ordinary production `release` retains its prior optimization policy.
  Changing controller base/tree digest, dependency versions, feature union, capacities, timeouts, or
  profile reopens Phase 1.

| Criterion | Result | Observation/metric | Required threshold/source | Evidence |
| --- | --- | --- | --- | --- |
| P1-A1 / `BLE-BOUND-01` | Pass | Fixed RX/host pools; observable drop/exhaustion | No callback allocation/unbounded enqueue | Patch, guard, source audit |
| P1-A2 / `BLE-BOUND-02` | Pass | Yielding 100 ms TX waits; propagated fault latch | Finite deadline/fault transition | Patch, guard, source audit |
| P1-A3 / `BLE-PIN-01` | Pass | Exact source/checksums/features; one `bt-hci`; locked build | Exact reproducible identity | Lock/tree/build/audit results above |
| P1-A4 | Pass | Default and BLE candidate build; strict BLE Clippy | Clean locked Xtensa builds | Commands above |
| P1-A5 / `BLE-MEM-01` | Pass | Linked, reserved, task/private, host/controller allocation classes inventoried | Separate inventory | Resource rows and symbol inventory below |
| P1-A6 | Pass | BLE image 1,822,448; explicit 78,096-byte forecast | Image <=1,900,544 | Application image and forecast above |

| Resource/state | Default release | BLE candidate | Delta | Floor/ceiling | Result |
| --- | ---: | ---: | ---: | ---: | --- |
| Application image | 1,853,872 | 1,822,448 | -31,424 across distinct profiles | <=1,900,544 | Pass |
| `.data` | 15,772 | 17,900 | +2,128 | Inventory | Pass |
| `.data.wifi` | 540 | 1,872 | +1,332 | Inventory | Pass |
| `.bss` | 69,428 | 76,084 | +6,656 | Inventory | Pass |
| `.stack` linker remainder | 110,860 | 35,212 | -75,648 including 65,536 reserve | Phase 1 inventory | Pass |
| `.dram2_uninit` | 104,392 | 104,392 | 0 | <=113,840 region | Pass |
| Named BLE statics | 0 | 5,341 | +5,341 | Fixed/counted | Pass |
| BTDM private/task | 0 | 65,536 reserved + >=4,112 release heap | New | Enumerated; runtime floor at P5 | Pass for P1 |

- Evidence boundary: no controller was initialized on hardware; no BLE advertisement, connection,
  macOS interaction, runtime heap/stack, power, coexistence, update race, touch, panel, SD, or physical
  behavior was tested. Host/build evidence cannot satisfy Phases 4–6.
- Gate disposition: **Pass**. Phase 1R condition is false because the candidate fits without relocating
  fonts/assets. R-001 is resolved; R-002, R-003, and R-012 are mitigating for descendant gates. The
  next and only ready phase is the Phase 2 architecture/resource ADR.

### E-0007: Proposed Phase 2 architecture/resource ADR

- Date/time and scope: 2026-08-11; current ownership/source inspection and documentation-only Phase 2
  ADR draft.
- Phase and criteria covered: Phase 2, P2-A1–P2-A5, draft completeness only.
- Evidence kind: documentation/source inspection. ADR-0011 remains `Proposed` because its numeric
  product power limits require human acceptance.
- Source identity: HEAD `803077a0816b191342f80c7ee1d2edaba6eafb1c`; shared dirty checkout and
  all unrelated changes preserved. No dependency, feature, firmware, lockfile, partition, or build
  artifact changed for Phase 2.
- ADR identity: `docs/architecture/0011-bounded-ble-service-foundation.md`, SHA-256
  `33653307ca2ef34625f1c644a31090f69b0afcd3d1603e4847373b75ae56a794` before any acceptance edit.
- Ownership/lifecycle recommendation: one coordinator owns epochs/admission/deadlines; Wi-Fi and BLE
  tasks retain their respective controller ownership; reserved close/update control cannot be starved;
  service windows are explicit, 60 seconds advertising, 120 seconds absolute, 30 seconds connected
  idle, and two seconds hard close.
- Update recommendation: update admission outranks BLE and may call transport preparation only after
  an `Off(epoch)` teardown acknowledgement. Close timeout returns `BleCloseTimeout`; it never proceeds
  into core park or cache-disabled flash with ambiguous BLE ownership.
- Coexistence recommendation: retain the Wi-Fi controller/link/runner and coexistence code; block new
  product traffic, allow 12 seconds for an accepted operation plus two seconds for abort, defer Wi-Fi
  scan/reconnect, then restore the prior gates. Full Wi-Fi teardown remains an optional experiment.
- Identity recommendation: fresh random-static address per window, generic advertisement/service UUID,
  no stable identifier or sensitive network/storage/update fields, no pairing/bonding for non-mutating
  diagnostic v1, and mandatory macOS rediscovery between windows.
- Resource recommendation: preserve the existing image, stack, heap, scheduling, drift, and Phase 1
  capacity allocations. Phases 3–5 use `ble-release`; Phase 6 creates and revalidates one canonical
  ordinary production profile.
- Power recommendation: runtime-off <=2 mA average/10 mA peak incremental; advertising and connected
  idle <=70/250 mA; echo exchange <=90/300 mA; complete 120-second window <=50 J; return within 2 mA
  of baseline in two seconds. Average/peak measurement definitions and instrument fidelity are fixed in
  ADR-0011.
- Validation: scoped Markdown links passed 27/27; the repository LOC advisory completed with no new
  high-attention file; scoped whitespace, absolute-local-path, and `git diff --check` checks passed.
  No artifact exists because this phase changed documentation only.

| Criterion | Result | Observation | Remaining gate |
| --- | --- | --- | --- |
| P2-A1 | Partial | Ownership, state, epochs, deadlines, retries, and safe-off are explicit. | Accept ADR-0011. |
| P2-A2 | Partial | Both request orders and cache-disabled exclusion are explicit. | Accept ADR-0011. |
| P2-A3 | Blocked | Numeric ceilings and measurement method are explicit recommendations. | Human accept or revise values. |
| P2-A4 | Partial | Full teardown is separate and non-blocking. | Accept ADR-0011. |
| P2-A5 | Partial | Payload, random address, exposure, and tracking policy are explicit. | Accept ADR-0011. |

- Evidence boundary: no coordinator or update arbitration was implemented or host-tested; no BLE
  controller, advertisement, connection, macOS client, power instrument, Wi-Fi handoff, flash update,
  or physical device was exercised.
- Gate disposition: **Partial/In progress**. The next action is a human accept-or-revise decision on
  ADR-0011, especially its power ceilings. Phase 3 remains blocked.

### E-0008: Agentic review fix pass and Phase 1 reopening

- Date/time and scope: 2026-08-11; five independent architecture, adversarial, embedded, macOS/privacy,
  and ledger reviews followed by a source/documentation fix pass.
- Evidence kind: source, host/build, and documentation. No device or physical evidence.
- Source identity: HEAD `803077a0816b191342f80c7ee1d2edaba6eafb1c`; shared dirty checkout.
  The first-party BLE integration, manifests/lockfile, guards, and vendor patch are not all identified
  by that commit or a committed reconstructible bundle. This is a failed P1-A3 gate, not an exception.
- Confirmed transport failure: E-0006's HCI availability/completion paths could synchronously occupy
  the Embassy executor for up to 100 ms despite yielding to RTOS tasks, contradicting P1-A2 and the
  16 ms product scheduling limit.
- Source correction: repository patch tree digest
  `fae60f6341a0653475525e9f474252363a8d4e506bd735b1747e2840efa3d794` uses one fixed async-mutex TX
  collector, callback-woken availability/completion futures, independent 100 ms Embassy deadlines,
  cancellation fault-latching, and a post-lock fault recheck for queued senders. The guard rejects
  synchronous polling/yield loops and requires the awaited controller call.
- Validation: locked ordinary release, locked BLE `ble-release`, strict BLE Clippy, Rustfmt check,
  controller-patch guard, 3/3 deterministic cancellation-guard host tests, and image guard passed. BLE
  ELF SHA-256 is `32256551ff48034f1f66c79a34ec759779eeccf0d51107378e41eadbc8ac18fb`;
  application SHA-256 is `91d36a15909f84b4ad01249d659e8c22345782efd8400ca97e8fc889094e3508`.
- Linked result: image 1,831,056/1,900,544 bytes, leaving 69,488; `.data` 18,276,
  `.data.wifi` 1,872, `.bss` 77,028, `.stack` 33,884, `.dram2_uninit` 104,392; named BLE statics
  total 6,606 bytes.
- Adverse runtime forecast: the 58 KiB internal heap, prior 48,520-byte workload high-water, and at
  least 4,112-byte BTDM task forecast at most 6,760 bytes free before opaque controller allocations.
  This is not a measurement, but it is sufficiently adverse to require Phase 1D before ADR acceptance.
- ADR correction: ADR-0011 now defines transition-matched boot/service identity, `OffConfirmed`,
  `FaultedOwnershipUnknown`, callback fencing, real traffic leases, atomic update grants on every
  mutating API, controller-power address epochs, exact advertising/macOS/schema policy, bounded denial,
  and guarded power measurement. It remains `Proposed`; revised SHA-256 is
  `adf91c617e0f4c16f45ebd36e374559ee3321ac82a6c89586268bb01afff9e35`.
- Plan correction: Phase 1 is reopened, Phase 1D is inserted before Phase 2, cancellation is mandatory
  at every TX await boundary, Wi-Fi/coex owners must stay resident during the probe, active/full-queue
  callback closure is non-vacuous, Phase 5 results are evidence-lane specific, and Phase 6 reruns every
  Phase 5 gate.
- Documentation validation: the four-file scoped link check passed 16 links and excluded two
  external specification links; whitespace, local-path, shell syntax, and tracked-diff checks passed.
  ADR-0011 is a 273-line LOC warning and the 338-line plan is a high-attention advisory; the 484-line
  ledger is append-only and exempt. These advisories do not waive any content or implementation gate.

| Criterion | Result | Observation | Remaining gate |
| --- | --- | --- | --- |
| P1-A1 | Pass | Fixed RX capacity/backpressure unchanged. | Durable complete source identity. |
| P1-A2 | Partial | Async source/guard/build and 3/3 cancellation-guard host tests pass. | Include in durable-source rebuild evidence. |
| P1-A3 | Fail | Current artifacts are not reconstructible from committed source identity. | Isolated commit or committed immutable source bundle. |
| P1-A4–P1-A6 | Partial | Rebuilt artifacts and static/image gates pass. | Repeat from durable identity. |
| P1D-A1–P1D-A5 | Not run | Hardware gate newly required. | Complete source gate, then exact-artifact device run. |
| P2-A1–P2-A5 | Partial | Proposed ADR now contains the reviewed contracts. | Phase 1D plus human product acceptance. |

- Evidence boundary: no controller was initialized, no shutdown/callback fence was observed, and no
  runtime heap, power, advertisement, connection, macOS, Wi-Fi handoff, update race, touch, panel, SD,
  or physical behavior was tested.
- Gate disposition: **Phase 1 Needs revalidation; Phase 1D and Phase 2 Blocked.** E-0008 supersedes
  only E-0006's gate disposition and E-0007's next-action claim; historical observations remain.

### E-0009: Callback-quiescent lifecycle baseline implementation

- Date/time and scope: 2026-08-11; Phase 1 transport shutdown and Phase 1D baseline implementation,
  host evidence workflow, build/size refresh, and documentation. No device run.
- Evidence kind: source, host, build, and documentation. No device or physical evidence.
- Source identity: HEAD `803077a0816b191342f80c7ee1d2edaba6eafb1c`; shared dirty checkout.
  The complete first-party/vendor inputs still are not identified by a durable commit or immutable
  source bundle, so P1-A3 remains failed. Patched vendor-tree digest is
  `4019a3738d1b312acd55030b26ac41691ebecb592bd1b3c0f91285b63d403a93`.
- Source result: every ESP32 VHCI callback now increments an in-flight count before admission, late
  ingress is rejected, shutdown atomically disables the BTDM callback source once, and a bounded
  async wait observes zero in-flight callbacks before first-party GATT/host/controller storage is
  dropped. Timeout enters `ownership_unknown` and prohibits token reconstruction until reboot.
- Baseline result: `BLEPROBE START` explicitly runs 20 controller/host init, 750 ms active, checked
  close, and reinit cycles. Wi-Fi controller-task and Embassy-runner residency are independently
  sampled with link, DHCP, listener, CPU0/touch minima, internal free/minimum, callback counters,
  transport counters, and packet-pool state. Firmware reports `completed`, never `passed`.
- Host evidence path: `hostctl test ble-phase1d` requires a labeled `ble-release` flash-capture
  artifact set with `ble-foundation` and a clean exact git HEAD, verifies its recorded
  ELF/application hashes, matches the running build identity through `BLEPROBE STATUS`, checks HTTP
  health before and after, refuses an ambiguous probe timeout retry, and
  emits a JSON baseline verdict. The workflow file owns its sequencing and failure branch.
- Build validation: locked BLE strict Clippy and `ble-release` builds, ordinary release build,
  controller-patch guard, image guard, 3/3 cancellation tests, and 6/6 baseline artifact/analyzer/workflow
  tests pass. Full hostctl tests and strict host Clippy pass.
- Artifact result: BLE ELF SHA-256
  `fe6997e81af3d2e9a6ccb26c05f155dadf1fd7ad0a35f2bea8f594ec8836644a`;
  application SHA-256 `a62ec84037b36f77ef9c1f99d8142b30730c04f90396da6b05a527612709de13`.
  Image 1,837,168/1,900,544 bytes leaves 63,376 bytes. Sections are `.data` 18,260,
  `.data.wifi` 1,872, `.bss` 77,116, `.stack` 33,812, and `.dram2_uninit` 104,392; named
  BLE/lifecycle statics total 6,729 bytes.

| Criterion | Result | Observation | Remaining gate |
| --- | --- | --- | --- |
| P1-A1/P1-A2 | Partial | Bounded TX plus source-disabled, counted callback quiescence pass source, guard, host, and build checks. | Repeat from durable source identity. |
| P1-A3 | Fail | Checkout still contains dirty/untracked Phase 1 inputs. | Isolated commit or committed immutable source bundle. |
| P1-A4–P1-A6 | Partial | Both build profiles pass; current image retains 63,376 bytes. | Repeat from durable source identity. |
| P1D-A1–P1D-A5 | Not run | Firmware and host baseline lane exist, but no hardware was flashed or measured. | Durable P1 artifact, device baseline, largest-block telemetry, and forced TX/RX race lanes. |

- Known non-claims: `esp-alloc` exposes total/capability free bytes but not largest free block; the
  baseline does not force cancellation at both HCI TX waits or during active/full-queue RX callback
  ingress. The JSON report hard-codes these as remaining gates and `phase1d_gate_passed=false`.
- Evidence boundary: no BT controller was initialized; no callback, reinit, runtime heap, Wi-Fi HTTP,
  reset, watchdog, touch, panel, power, macOS, GATT, or physical behavior was observed.
- Gate disposition: **Phase 1 Needs revalidation; Phase 1D and Phase 2 remain Blocked.** E-0009
  supersedes E-0008's next implementation action, not its durable-source failure or hardware boundary.

### E-0010: Selective Phase 1S integration on the active Wi-Fi branch

- Date/time and scope: 2026-08-14; selective import of the last code-bearing Phase 1S source
  `69dd8499bc3823db04420596bf78749e22552ec6` into `fix/wifi_connectivity`. Experimental and
  evidence-snapshot commits were not merged. The active branch's later non-cancellable SD DMA rule
  was preserved while adding the required internal bounce sector for PSRAM-backed FAT state.
- Evidence kind: committed source, host, build, and static analysis. No device or physical run.
- Durable source identity: commit `77569d33d575a5ebb70a2edc45047ab351d7ce5c`. The locked patch set is
  restartable `embassy-net` 0.9.1, allocator-provenance `esp-alloc` 0.10.0, bounded `esp-radio`
  1.0.0-beta.0, and retained `esp-radio-rtos-driver` 0.3.0. The retained pre-integration safety
  stash was not dropped.
- Source result: the network supervisor owns restartable Wi-Fi epochs and exact request/boot/epoch
  handoff acknowledgements. Acquire closes HTTP admission, drains or aborts SD/upload ownership,
  fences callback/queue sources, settles allocator evidence, and exposes a bounded BLE controller/
  host-only window. Release closes BLE, rejects ambiguous ownership, and restores Wi-Fi, DHCP,
  listener policy, and upload service. Update admission outranks handoff.
- Host/evidence result: the Phase 1S Serverless Workflow and `hostctl test ble-phase1s` retain
  schema-versioned success/failure reports, exact artifact/feature identity, allocation-free serving
  snapshots, allocator RX correlation, stack/UART/resource floors, and known/unknown ownership
  branches. Hostctl passed 257 tests; BLE transport/handoff passed 32 tests; SD passed 19 tests.
- Build/static result: the complete software baseline passed locked metadata, source/vendor guards,
  all host tests and strict lints, default and BLE releases, minimal/slim/telemetry/all-feature builds,
  strict firmware Clippy, image/stack ratchets, reachability, script-surface, rust-analyzer summary,
  and code-analysis ratchets. BLE image is 1,739,296/1,900,544 bytes with 161,248 headroom; ELF
  SHA-256 is `d9d60721605c46c14715ebbb013f4e4ec0686700f9537f3b887f134e38f0aded`.
- Linked BLE sections: `.data` 16,272, `.data.wifi` 1,872, `.bss` 76,364, `.stack` 36,564, and
  `.dram2_uninit` 113,736. The latter contains the 45,000-byte active panel framebuffer and
  68,736-byte sole internal heap, leaving 104 bytes in `dram2_seg`.
- Capacity boundary: the imported source branch's binding device evidence reached 15,156 internal
  bytes against the unchanged 16,384-byte floor, a 1,228-byte deficit. This integration did not rerun
  or supersede that device result and does not enable BLE by default. Both Wi-Fi and BLE remain product
  requirements; the next candidate must recover applicable live internal memory or explicitly justify
  a different floor from a reviewed workload/failure model before changing the threshold.

| Criterion | Result | Observation | Remaining gate |
| --- | --- | --- | --- |
| P1-A1–P1-A6 | Pass | Durable exact source, locked patches, full builds, image budget, and guards pass. | Reopen on source, dependency, feature, or artifact change. |
| P1S-A1–P1S-A3/P1S-A5 | Source/host pass | Handoff lifecycle, rollback, update priority, evidence workflow, and focused host tests are present and guarded. | Exact device exercise. |
| P1S-A4 | Fail retained | Historical binding minimum is 15,156 versus 16,384; no new device run was made. | New memory or threshold premise plus complete Wi-Fi/20-cycle rerun. |

- Gate disposition: **Phase 1 passes at durable source/build scope; Phase 1S remains In progress at
  the runtime capacity gate; Phase 2 remains Blocked.**

## Transition history — append only

| ID | Date | Target | From | To | Reason | Authority | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| H-0001 | 2026-08-11 | Phase 0 | Not started | Passed | Initial plan/ledger established. | Plan | E-0001 |
| H-0002 | 2026-08-11 | Phase 0 | Passed | Needs revalidation | Agentic review found structural blockers. | Review | E-0003 |
| H-0003 | 2026-08-11 | Phase 0 | Needs revalidation | Passed | Fix pass integrated and validated. | Plan | E-0004 |
| H-0004 | 2026-08-11 | Phase 1 | Blocked | Ready | Phase 0 prerequisite passed. | Plan | E-0004 |
| H-0005 | 2026-08-11 | Phase 1 | Ready | In progress | Began maintained controller source audit. | User direction | E-0005 |
| H-0006 | 2026-08-11 | Phase 1 | In progress | Blocked | Released and maintained sources fail bounded RX/TX gates. | Plan | E-0005 |
| H-0007 | 2026-08-11 | Phase 1 | Blocked | In progress | User authorized repository ownership of the bounded patch. | User direction | E-0006 |
| H-0008 | 2026-08-11 | Phase 1 | In progress | Passed | Exact bounded source, locked builds, and fixed-cost gates pass. | Plan | E-0006 |
| H-0009 | 2026-08-11 | Phase 2 | Blocked | Ready | Phase 1 prerequisite passed without Phase 1R. | Plan | E-0006 |
| H-0010 | 2026-08-11 | Phase 2 | Ready | In progress | ADR-0011 drafts the architecture and numeric product budgets; human acceptance remains required. | User direction | E-0007 |
| H-0011 | 2026-08-11 | Phase 1 | Passed | Needs revalidation | Async-progress and durable-source gates were falsified. | Agentic review | E-0008 |
| H-0012 | 2026-08-11 | Phase 2 | In progress | Blocked | Reopened Phase 1 and new runtime feasibility gate are prerequisites. | Plan | E-0008 |
| H-0013 | 2026-08-11 | Phase 1D | Not started | Blocked | Await durable Phase-1 source/build evidence. | Plan | E-0008 |
| H-0014 | 2026-08-14 | Phase 1 | Needs revalidation | Passed | Selective Phase 1S import creates one durable exact source/build identity and passes the complete software baseline. | User direction and source/build evidence | E-0010 |
| H-0015 | 2026-08-14 | Phase 1S | Not started | In progress | Both Wi-Fi and BLE are required; exclusive handoff is imported while its 1,228-byte runtime floor deficit remains binding. | User direction and retained device evidence | E-0010, F-0006 |

## Deviations and failures — append only

| ID | Date | Phase/criteria | Failure | Containment | Recovery evidence | State |
| --- | --- | --- | --- | --- | --- | --- |
| F-0001 | 2026-08-11 | P1-A1/P1-A2 | Latest released and maintained `esp-radio` transport is unbounded. | No dependency or firmware change; build probe stopped. | E-0005; upstream fix or authorized owned patch required. | Open |
| F-0002 | 2026-08-11 | P1-A1/P1-A2 | Closure record for F-0001: upstream remains unbounded, but the authorized repository patch now satisfies the phase-local gates. | Exact base/tree digest and feature identity reopen P1 on change. | E-0006; supersedes F-0001 state only. | Closed |
| F-0003 | 2026-08-11 | P1-A2 | The first bounded TX patch yielded to RTOS tasks but synchronously occupied the Embassy executor for up to 100 ms. | Replace with callback-woken async waits; reopen Phase 1. | Source correction and guards pass in E-0008; durable rebuild still required. | Mitigating |
| F-0004 | 2026-08-11 | P1-A3 | Phase-1 artifacts are not reconstructible from the recorded HEAD/vendor digest because first-party inputs are dirty or untracked. | Preserve prior evidence; require an isolated commit or committed immutable source bundle. | Replacement P1 evidence required. | Open |
| F-0005 | 2026-08-11 | P1D-A2/P1D-A4 | Static heap forecast is below the 16 KiB runtime floor before opaque controller allocations. | Insert Phase 1D before ADR acceptance. | Exact-artifact runtime measurement required. | Open |
| F-0006 | 2026-08-14 | P1S-A4 | The imported source branch's binding Wi-Fi run reached 15,156 internal bytes against the unchanged 16,384-byte floor. The selective import adds no remaining `dram2_seg` capacity and does not itself rerun the device gate. | Keep BLE non-default and Phase 2 blocked; preserve both Wi-Fi and BLE requirements and the exact historical failure. | Recover applicable live internal ownership or approve a reviewed workload/failure-model change to the floor, then build/full-flash one exact artifact and rerun complete Wi-Fi plus 20 Phase 1S cycles. | **Resolved** — the formal 20-cycle gate on exact commit `9606e152...` (post F-0008 fix) passed clean with a 19,896-byte serving low-water, clearing the floor by 3,512 bytes; the historical 15,156-byte failure does not reproduce on this source. See [Phase 1S capacity ledger CAP-0009](ble-phase1s-capacity-recovery-ledger.md#cap-0009--formal-20-cycle-gate-passed-clean-no-capacity-candidate-needed). |
| F-0007 | 2026-08-14 | P1-A3 | Closure record for F-0004: the complete selective integration, exact vendor identities, guards, host workflow, and build configuration are committed together at `77569d33d575a5ebb70a2edc45047ab351d7ce5c`. | Reopen P1 on any source, patch digest, feature graph, or artifact-identity change. | E-0010 supersedes F-0004's open state only; its historical dirty-source observation remains. | Closed |
| F-0008 | 2026-08-14 | P1S-A4/P1S-A5 | **Fixed and device-verified; not a hang (corrected by CAP-0006).** On `33061989...` (source-identical to `77569d3...`), whenever `RADIOHANDOFF ACQUIRE` landed while a genuine SD upload write was in flight and outlasted `ACTIVE_OPERATION_GRACE` (12s), the forced-abort path hit `Fat(ClusterChainTooLong)` removing the temp file (correctly propagated, not swallowed), and `run_network_epoch`'s retry path recreated the Wi-Fi connection task from a **stale, epoch-entry-only snapshot of `wifi::current_runtime_config().credentials`** instead of live config, losing the credentials the first connection had actually been using. The reseeded task legitimately had no SSID to try and sat idle until `await_restoration`'s own bounded `restoration_timeout` ([15_000, 180_000] ms) expired (~130s observed), after which the outer loop re-read config fresh and Wi-Fi fully recovered on its own — never a hardware-reset-requiring hang. Root cause and exact log trace: [CAP-0006](ble-phase1s-capacity-recovery-ledger.md#cap-0006--correction-not-a-hang-bounded-130s-self-recovery-exact-root-cause-identified). Prior device evidence (CAP-0004/CAP-0005) remains factually accurate raw data; its "hang" characterization is superseded. Separately, and still unfixed: aborted uploads still permanently poison that directory's shared `HCTLUPLD.TMP` (CAP-0005), a distinct bug. | Fixed in `src/firmware/net/runtime.rs`: `run_network_epoch`'s retry loop now re-reads `wifi::current_runtime_config()` on every iteration instead of once at epoch entry. Verified device-side 2/2: after the fix, reconnection begins within 14ms of the abort (vs. ~102s idle before), confirmed by the credentials-loss log line never reappearing post-abort and the retry correctly re-selecting the configured SSID. Both verification runs also hit an unrelated, pre-existing ~42.7s AP-session `connect_timeout`/driver-restart step (plausibly a missing deauth on the abrupt pre-fix teardown, not confirmed) — noted in [CAP-0007](ble-phase1s-capacity-recovery-ledger.md#cap-0007--fix-implemented-and-verified-retry-reads-live-config-not-a-stale-snapshot) as a separate, un-investigated observation. | Closed for the credentials-snapshot defect (CAP-0007). `HCTLUPLD.TMP` poisoning is now also fixed — root-caused and hardware-verified as [CAP-0011](ble-phase1s-capacity-recovery-ledger.md#cap-0011--cap-0005-root-caused-and-fixed-chain-free-step-budget-derived-from-a-stale-directory-entry-size-not-the-actual-chain-length)/[CAP-0012](ble-phase1s-capacity-recovery-ledger.md#cap-0012--cap-0011-fix-hardware-verification-pre-fix-repro-post-fix-prevention-and-poisoned-file-recovery): the chain-removal step budget was derived from the upload's never-persisted-until-commit directory-entry `size` (always 0 in-progress) instead of the volume's cluster count, so a valid-but-longer-than-32-cluster chain was misdiagnosed as `ClusterChainTooLong`; fixed by bounding the walk to `volume.total_clusters` at both call sites. The AP-session-timeout observation (CAP-0007) remains open, separate, lower-priority. Re-running the Wi-Fi regression gate on the fixed artifact surfaced a new, unrelated internal-memory-floor failure at `acceptance_3_cycle` ([CAP-0013](ble-phase1s-capacity-recovery-ledger.md#cap-0013--wi-fi-regression-gate-rerun-cap-0005s-sd-blocker-is-gone-a-different-unrelated-internal-memory-floor-failure-now-blocks-a-clean-pass)) — a capacity-model question, not part of this finding, tracked separately and now root-caused (not a leak) by [CAP-0014](ble-phase1s-capacity-recovery-ledger.md#cap-0014--cap-0013-root-caused-a-monotonic-since-boot-low-water-register-sampled-over-a-longer-teardown-free-session-not-a-leak), with the residual floor-methodology question referred to the user. | Closed |

## Evidence entry template

### E-NNNN: Short title

- Date/time and scope:
- Phase and criteria covered:
- Evidence kind: documentation | host | build | device | physical.
- Source identity: branch, HEAD, dirty-scope digest; toolchain/target; exact dependency source,
  constraint, checksum, feature graph, license/advisory and maintenance evidence.
- Artifact identity: ELF/application hashes, build identity and slot, or `N/A — reason` when no artifact
  can exist.
- Hardware/client identity: board, Mac/macOS/Bluetooth adapter/client version, instruments and
  calibration.
- Commands/environment and raw repo-relative evidence paths plus hashes:
- Recovery preparation, action, expected state, and observed result:
- Failure/deviation references and evidence boundary:

| Criterion | Result | Observation/metric | Required threshold/source | Evidence |
| --- | --- | --- | --- | --- |
| Pn-Am | Pass/Fail/Not run/N/A | — | — | — |

| Resource/state | Baseline | Observed | Delta | Floor/ceiling | Result |
| --- | ---: | ---: | ---: | ---: | --- |
| Image/sections/`RESERVE_DRAM` | — | — | — | — | — |
| Embassy/host pools and BTDM private/task release+debug allocation | — | — | — | — | — |
| CPU/touch stack, internal free/largest block, queues/pools | — | — | — | — | — |
| Power state/energy and return deadline | — | — | — | — | — |

- Gate disposition: Pass | Fail | Partial | Informational.
- Blocking risks resolved/created; next action; supersedes:

`N/A` is valid only for a criterion declared conditional in the plan, with evidence that its condition
was false. A retry always receives a new evidence ID.

## Next implementation step

Commit `77569d33d575a5ebb70a2edc45047ab351d7ce5c` closes the durable source/build-identity gap.
Next, identify a safe source-level recovery of at least the observed 1,228-byte internal-floor deficit,
or write and review a workload/failure-model basis for a different floor before changing it. Build and
full-flash one clean labeled candidate, then run the complete Wi-Fi regression and exactly 20 Phase 1S
handoff/BLE/restore cycles. Do not accept ADR-0011, implement Phase 3, begin macOS product BLE, mutate
SD over BLE, or enable BLE by default until that exact-artifact runtime gate passes.
