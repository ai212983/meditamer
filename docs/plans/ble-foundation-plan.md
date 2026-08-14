# BLE Foundation and Upload Transport Plan

- Status: Active
- Last-reviewed: 2026-08-14
- Started: 2026-08-11
- Evidence: [BLE implementation ledger](ble-foundation-ledger.md)
- Related: [Phase 1S capacity recovery](ble-phase1s-capacity-recovery.md),
  [Asset upload transport plan](asset-upload-transport.md), [A/B update
  foundation](../architecture/0009-ab-firmware-update-foundation.md),
  [BLE service ADR](../architecture/0011-bounded-ble-service-foundation.md)

## Objective and authority

Add permanent, base-firmware BLE support to both A/B images on the existing ESP32-WROVER-E. Keep
Wi-Fi, the A/B reserve, and current ownership boundaries intact. Prove a bounded, time-limited,
non-mutating GATT service with native macOS CoreBluetooth first; promote asset upload over BLE only as
a later, independently removable extension. iOS remains a later, non-blocking client.

"Permanent" means compiled into every production image, not continuous advertising. This plan is the
current execution authority for BLE and supersedes the overlapping sequence in the Proposed
[asset-upload transport plan](asset-upload-transport.md). It does not ratify BLE as the primary upload
transport until the macOS upload gate passes.

This work begins after UI/app Phase 6. Phase 1 determines BLE's fixed cost before remaining A/B
capacity is assigned. If that cost crosses the current limit but is recoverable by moving enumerated
fonts/assets to SD, Phase 1R may do so without changing the partition map or reserve. SD-loaded WASM
architecture remains separate.

Both Wi-Fi and BLE are product requirements. Commit `77569d3` imports the exclusive radio-handoff
implementation and its exact dependency/host evidence surface into the active branch: Wi-Fi remains
the normal serving owner, a bounded lease can quiesce and release it for BLE, and restoration must
recover DHCP/listener service before the lease closes. The import does not claim the runtime memory
gate: the source branch's binding Wi-Fi run reached 15,156 internal bytes against the current 16,384
floor. The next work is to recover applicable internal memory or re-derive that floor from an explicit
failure model; it is not permission to drop either radio or silently lower the threshold.

## Confirmed baseline

- Each A/B slot is 2,031,616 bytes and must retain 131,072 bytes; maximum image size is 1,900,544.
  The accepted Phase 6 image is 1,855,360 bytes, leaving 45,184 bytes beyond that reserve.
- Phase 6 sections are `.data` 15,804, `.bss` 69,420, `.stack` 110,836, and `.dram2_uninit`
  104,392 bytes. Internal DRAM is binding and `.stack` is the `dram_seg` remainder.
- The imported `ble-release` candidate is 1,739,296/1,900,544 bytes with 161,248 bytes image
  headroom. Its sections are `.data` 16,272, `.data.wifi` 1,872, `.bss` 76,364, `.stack` 36,564,
  and `.dram2_uninit` 113,736. This is build evidence, not a device memory-floor pass.
- Enabling `esp-radio/ble` enables `esp-hal/__bluetooth`. On ESP32, `esp-hal` 1.1.1 then sets
  `RESERVE_DRAM=0x10000`; against Phase 6 this reduces the pre-BLE-static stack ceiling to about
  45,300 bytes.
- BTDM creates a non-Embassy task from internal memory. Its 4,096-byte configured stack occupies at
  least 4,112 bytes in release and 10,256 bytes in debug after current `esp-rtos` overhead/alignment,
  excluding its control block and controller allocations. It is not an Embassy `::POOL` or linked
  `.data/.bss` cost.
- Unpatched `esp-radio` 1.0.0-beta.0 allocates each VHCI receive packet into an unbounded `VecDeque`;
  transmit availability/completion waits busy-spin without a deadline. It cannot pass Phase 1
  unchanged. The first owned patch recorded in E-0006 fixed capacity/deadlines but still synchronously
  occupied the Embassy executor; the current callback-woken async correction awaits replacement
  durable-source evidence.
- The same ESP32 adapter selects `BTDM_MODEM_SLEEP_MODE_NONE`. Power measurements without approved
  limits are characterization, not acceptance.
- Wi-Fi is permanent. Its controller and Embassy network runner are long-lived, so pausing product
  traffic does not remove the Wi-Fi stack or coexistence cost.
- HTTP already serializes bounded requests through the sole SD/FAT owner. BLE must use the same
  operation authority, not create another filesystem owner.

## Invariants

1. BLE and Wi-Fi remain compiled into both A/B images; no phase removes Wi-Fi to make BLE fit.
2. The signed A/B map, bootloader, update transaction, health confirmation, and `0x20000` slot reserve
   remain unchanged unless a successor ADR explicitly changes them.
3. One base coordinator owns radio epochs and leases. One BLE task owns host, advertising,
   connection, and GATT lifecycle. Loadable apps own neither.
4. The firmware-update lease outranks BLE: BLE open is rejected while an update is active; update
   start either refuses active BLE or obtains an acknowledged bounded BLE close before cache-disabled
   flash work, as selected by the Phase 2 ADR.
5. Callbacks perform fixed-work validation and bounded enqueue only. They never access SD/FAT, LVGL,
   panel, app-state flash, firmware-update flash, or boot metadata.
6. Pools, queues, paths, sessions, MTU, and connection count have compile-time limits. Lifecycle
   control has reserved capacity so data saturation cannot starve close, watchdog, touch, Wi-Fi
   restoration, or update exclusion.
7. GATT framing is versioned and independent of upload operations. ATT receipt, queue admission, SD
   acceptance, and durable publication are distinct states.
8. A disconnect, timeout, service-window close, reset, or mode change has one explicit recovery rule;
   no partial asset becomes visible.
9. Native macOS evidence is required and is not iOS evidence. Host/build, device/serial, and physical
   panel/touch evidence remain separate.
10. Every permanent phase rebuilds the production candidate and reapplies the image, resource, and
    regression gates.

## Scope boundary

In scope: a single-connection peripheral; time-bounded service window; coordinator/update lease;
non-mutating Device Information/control/status GATT; native macOS harness; optional bounded BLE asset
upload through the existing SD owner; resource, power, lifecycle, and coexistence evidence.

Out of scope: Classic/SPP; default continuous advertising; central or multi-connection roles; BLE
firmware OTA; app-owned BLE; partition/reserve changes; deep-sleep integration; and executable package
trust. Upload fixtures for future apps are size/integrity tests only, not installation or execution.

## Stage A: Falsify feasibility first

### Phase 0: Planning baseline — complete

- P0-A1: plan and ledger separate BLE, Wi-Fi, A/B, UI, upload, and FAT ownership.
- P0-A2: confirmed baseline numbers cite current accepted evidence or inspected pinned source.
- P0-A3: Phase 1 is the only ready implementation phase.
- P0-A4: scoped documentation checks pass; no firmware/device result is implied.

### Phase 1: Controller/host source audit and fixed-cost probe — needs revalidation

Audit before integrating. Resolve exact direct dependencies with `=` versions or immutable revisions,
commit their lockfile checksums, and record target/toolchain, feature union, provenance, license,
advisory, and maintenance evidence. The current beta may be used only to reproduce its build/size and
the known transport failures. A maintained upstream set or an explicitly authorized, owned, audited
patch must supply bounded RX/backpressure and bounded yielding TX waits.

- P1-A1 (`BLE-BOUND-01`): no controller/host callback allocates, grows a collection, or enqueues
  without fixed capacity and an observable overflow rule.
- P1-A2 (`BLE-BOUND-02`): every HCI availability/completion wait is callback-woken and asynchronously
  awaited with a finite deadline/fault transition. No synchronous loop, including an RTOS-yield loop,
  may occupy the Embassy executor while waiting. Cancellation at each await boundary must latch the
  transport before releasing collector ownership; a queued sender must recheck the fault after locking,
  and source/behavior guards prove that no partial or in-flight packet can be appended or resent.
- P1-A3 (`BLE-PIN-01`): one intended `bt-hci` interface and a durable source identity reconstruct every
  first-party, manifest, lockfile, profile, guard, vendor, and Phase 1D probe/shutdown input. Clean
  locked builds and exact checksums/features are recorded; any input change reopens Phase 1.
- P1-A4: Wi-Fi-only and smallest supported Wi-Fi+BLE+coex release builds and strict Clippy pass on the
  Xtensa target. The spike exposes only build identity, harmless echo, and lifecycle status. The
  size-optimized probe uses the named `ble-release` profile; ordinary production `release` remains
  unchanged until Phase 6 promotion.
- P1-A5 (`BLE-MEM-01`): image and all linked sections, `RESERVE_DRAM`, Embassy pools, host packet
  pools, BTDM private reservation/task release+debug costs, and controller/host heap allocations are
  separately inventoried.
- P1-A6: image is at most 1,900,544 bytes and the forecast for Phases 2–6 retains explicit
  contingency; no upload, SD, UI, or update behavior is added.

Stop on unresolved unbounded transport behavior, incompatible HCI traits, unowned patch burden,
irreducible image/reserve failure, or a partition change. Compilation alone cannot pass Phase 1.

Ledger E-0006's gate disposition is reopened: its TX wait can occupy the Embassy executor for up to
100 ms and its dirty source identity is not durably reconstructible. Preserve its historical build and
size evidence, but do not advance until replacement evidence passes P1-A1–P1-A6.

### Phase 1R: Conditional capacity reclamation

Run only if Phase 1's irreducible BLE cost exceeds the image ceiling but an enumerated font/asset move
can recover it. Preserve both slots and the `0x20000` reserve.

- P1R-A1: an explicit relocation whitelist records every asset, exact reclaimed bytes, consumers,
  provisioning/recovery path, and an embedded minimal UI/font fallback.
- P1R-A2: the exact artifact boots and preserves UI/touch/panel behavior with SD present, absent,
  removed, corrupt, and containing missing/invalid relocated assets.
- P1R-A3: Wi-Fi, A/B update, cold boot, catalogue/font consumers, and recovery regressions pass.
- P1R-A4: all P1 criteria rerun against the reclaimed exact artifact and still pass.

Park BLE if either the product regressions or minimal supported resource profile fails.

### Phase 1D: Exact-artifact runtime allocation and shutdown feasibility

Run only after Phase 1 passes directly or after Phase 1R. This is a bounded hardware feasibility probe,
not product advertising, macOS interoperability, power acceptance, SD mutation, or default inclusion.

- P1D-A1: flash the exact rebuilt artifact with recorded ELF/application hashes, board identity,
  pre-flash capture, and recovery path.
- P1D-A2: before init, initialized, and after shutdown, record CPU0/touch minima, internal free/largest
  block, opaque controller/task allocation, pools/queues, callback counts, active `coex` feature, and
  independent Wi-Fi controller/link/DHCP/runner states. Those Wi-Fi owners remain resident throughout;
  CPU0 remains >=8,192, touch >=1,024, and internal free >=16,384 bytes.
- P1D-A3: with the Wi-Fi controller, link, DHCP, and Embassy runner resident under coex, 20
  controller+host init/shutdown/reinit cycles show no panic, reset, watchdog delay, monotonic allocation
  growth, stale acknowledgement, or callback attributed to a closed epoch. At least one forced close
  occurs at every HCI TX await boundary and during RX enqueue/callback activity, including with the
  fixed RX queue full; each cancellation fault-latches, rejects queued TX, and requires full
  teardown/reinitialization before reuse. Ingress is visibly rejected after revocation, and the close
  acknowledgement waits for the measured in-flight callback count to reach zero.
- P1D-A4: checked callback-quiescent shutdown acknowledges within two seconds; after warm-up,
  free-memory/largest-block drift is <=1,024 bytes and non-monotonic. The evidence separately proves
  that the Wi-Fi controller, link, DHCP, and runner stayed resident throughout every BLE active/close
  interval and that Wi-Fi product traffic works before and after.
- P1D-A5: record the opaque allocation plateau and whether deterministic shutdown is feasible. Stop on
  any floor breach, growth, late callback, unacknowledged shutdown, reset, or artifact mismatch.

### Phase 1S: Exclusive Wi-Fi/BLE radio-handoff feasibility

Use this branch when resident Wi-Fi plus BLE cannot preserve the internal-memory floor. Both
capabilities remain in each candidate image, but only one controller owns the shared radio at a time.

- P1S-A1: the network owner is restartable from the original Wi-Fi token and static stack resources;
  no runner, socket, callback, or queue owner escapes its epoch.
- P1S-A2: acquire closes HTTP admission, drains or aborts upload/SD work, drops the network runner,
  fences callbacks, and acknowledges `OffConfirmed` only after resource and allocator settlement.
  Ambiguous ownership fault-latches until reboot.
- P1S-A3: a bounded BLE controller/host-only window starts only from that exact off lease, closes with
  balanced operations and zero live/unattributed owners, then restores Wi-Fi, DHCP, listener policy,
  and upload service for the same boot/epoch.
- P1S-A4: the exact clean `ble-release` artifact passes 20 handoff/restore/upload cycles, the complete
  Wi-Fi regression gate, CPU0/touch floors, zero UART-drop delta, and internal free >=16,384. The
  source-branch 15,156 result remains the binding failure until a new architectural or threshold
  premise is implemented and remeasured.
- P1S-A5: update admission outranks handoff, never flashes through ambiguous ownership, and closes BLE
  before restoration or flash preparation. Evidence is schema-versioned and written on success and
  failure.

Phase 2 may proceed after either resident Phase 1D or exclusive-handoff Phase 1S passes.

## Stage B: Minimal permanent BLE foundation

### Phase 2: BLE architecture and resource ADR

Accept and link an ADR only after Phase 1D or Phase 1S passes. It ratifies coordinator/task ownership,
time-bounded availability, coex-enabled serialized traffic, diagnostic GATT, resource/power budgets,
macOS-first acceptance, diagnostic advertising/privacy identity, and a successor/amendment contract
for ADR-0009's update lease.

- P2-A1: the accepted ADR decides boot/transition identity, `OffConfirmed`, ambiguous ownership,
  callback fence, traffic lease, restoration, cancellation, close deadline, and retry semantics.
- P2-A2: atomic update reservation covers every coordinator state and every mutating update API;
  cache-disabled flash requires a live grant after callback-quiescent BLE-off proof.
- P2-A3: approved numeric power ceilings cover off-state delta, advertising average/peak, connected
  idle, exchange energy, total service-window energy/duration, and return-to-baseline deadline.
- P2-A4: full Wi-Fi teardown/coex avoidance is explicitly optional and requires a separate experiment
  that cancels runners, closes sockets, drops/recreates Wi-Fi, and reacquires DHCP.
- P2-A5: the ADR decides exact advertising/radio parameters, controller-epoch address construction,
  discovery SLA, bounded nearby-central denial, stable/build exposure, no-bond policy, fixed diagnostic
  schema, and the executable macOS cache contract.

### Phase 3: Radio coordinator and update lease

Implement and host-test the coordinator using the real HTTP/SD/update owners with only the BLE
controller backend stubbed. The first policy keeps Wi-Fi/coex resident but serializes product traffic.

- P3-A1: transition-identity/lease tests cover accept/route/SD races, slow sockets, concurrent opens,
  quiesce timeout, BLE start failure, close, stale/wrong-kind acknowledgements, latest-policy restore,
  Wi-Fi degradation, and retry/backoff.
- P3-A2: every coordinator-state/update ordering and every mutating update API requires the current
  non-cloneable grant; missing, released, and stale grants fail before transport/flash mutation.
- P3-A3: saturation cannot starve lifecycle control; every failure restores the previous state or
  named safe-off state within its deadline.
- P3-A4: Wi-Fi regression and A/B update regression pass with the stub BLE backend.
- P3-A5: the production candidate remains <=1,900,544 bytes; section/`RESERVE_DRAM`, task/private
  allocation, and cumulative Phase 6 contingency deltas are recorded and within budget.

### Phase 4: Base BLE task and diagnostic service

Implement the production-shaped BLE task and non-mutating Device Information, harmless control/echo,
and readable/notifiable lifecycle status. Expose only protocol/schema and Phase-2-approved
non-sensitive capabilities/identity. Do not add Data, paths, upload commands, or product secrets.

- P4-A1: pure tests cover every transition, timeout, stale event, queue-full path, disconnect, restart,
  and controller fault; all radio actions require a coordinator grant.
- P4-A2: ATT payload is compile-time capped and never determines allocation size.
- P4-A3: pure host tests cover fixed permissions/lengths, invalid offsets, unsupported long/signed
  writes, MTU extremes, CCC churn, echo quotas, notification coalescing/backpressure, stale epochs,
  churn, and floods with bounded work and no payload log, false acknowledgement, or lifecycle starvation.
- P4-A4: byte-level tests match ADR-0011's address construction, controller epoch, advertising PDU,
  UUIDs, permissions, fixed schema, no-bond policy, deadlines, quotas, and telemetry.
- P4-A5: the production candidate repeats P3-A5's image, linked/private allocation, and cumulative
  contingency gate; host-measurable bounds pass before device work.

### Phase 5: Device lifecycle, resource, power, and macOS proof

Enter BLE only through the coordinator. Run one identified release artifact with native macOS
CoreBluetooth, Wi-Fi present, and the complete touch/display workload.

- P5-A1: 100 native macOS windows pass service-filtered discovery, fresh schema discovery,
  read/write/subscribe/notify/close/restart, the discovery SLA, and stale-object rejection without
  panic, reset, growth, watchdog delay, or touch/panel regression.
- P5-A2 (`BLE-MEM-02`): image <=1,900,544; CPU0 stack >=8,192 bytes (12,288 target), touch-core stack
  >=1,024, internal free memory >=16,384, active gap <=16 ms; largest block and high-waters plateau.
- P5-A3 (`BLE-COEX-01/02`): every run records coex feature, Wi-Fi controller/link/listener/traffic,
  BLE controller/advertising/connection independently; serialized handoff restores Wi-Fi and never
  duplicates upload ownership. Only a separate accepted experiment may claim coex avoidance.
- P5-A4 (`BLE-POWER-01`): three guarded runs per state follow ADR-0011's paired-build/runtime baseline,
  fixed-range instrument, raw-sample, uncertainty, peak, energy, and recovery method; every ceiling passes.
- P5-A5: controller/over-air evidence proves 100 valid unique random-static controller epochs and exact
  advertising parameters; native macOS proves cache recovery; a named raw central proves ATT/GAP/SMP,
  rogue hold, no-bond, and flood behavior. Every case preserves close/update deadlines, resource floors,
  fixed exposure, and next-window discovery. CoreBluetooth evidence cannot substitute for raw protocol.

### Phase 6: Minimal BLE foundation promotion

Make the proven diagnostic capability a default compiled feature in both A/B images, runtime-off
outside the bounded service window. Completion satisfies permanent BLE independently of upload.

- P6-A1: the exact ordinary release and both signed A/B images remain <=1,900,544 bytes and preserve
  the mandatory slot reserve.
- P6-A2: that exact default artifact reruns and passes P5-A1–P5-A5 without reduced repetitions,
  workloads, resource floors, power ceilings, or physical gates; binary similarity is not evidence.
- P6-A3: user flow, protocol compatibility, troubleshooting, telemetry, and regression maintenance
  are documented; runtime disable does not remove the compiled capability.

## Stage C: Optional macOS BLE asset upload

### Phase 7: Upload security and operation ADR

Before mutating GATT, accept and link an ADR for attacker model, trust anchor, peer authentication, message
integrity/confidentiality, replay protection, privacy identity, nonce source, credential provisioning,
storage/reset/host replacement, service-window abuse, allowed mutations, and BLE-versus-HTTP role.
A challenge proves freshness only unless cryptographically bound to identity or deliberate physical
confirmation. No SD allocation occurs before authorization.

- P7-A1: the accepted ADR binds authorization to device, principal, protocol, boot/window epoch,
  operation, sequence, payload, nonce, and absolute expiry; nearby-attacker races, replay, hijack,
  brute force, and reset are tested.
- P7-A2: exact ATT permissions and separate upload/delete/admin capabilities are decided. Normal v1
  excludes `mkdir` and `remove` unless separately authorized.
- P7-A3: this plan and the Proposed asset-upload plan are reconciled; rejection parks upload without
  rolling back Phase 6.

### Phase 8: Transport-neutral upload operation boundary

Formalize `begin`, sequential `chunk`, `commit`, `abort`, `query`, and `stat` through the sole SD task.
The shared service owns canonicalization, root confinement, namespace/reserved-name rules, case/alias
policy, quota/free-space floors, SHA-256, and one global HTTP/BLE mutation lease.

- P8-A1: HTTP and in-memory BLE adapters pass the same conformance suite; adapters cannot bypass path
  or authorization policy.
- P8-A2: received, queued, SD-accepted, synced, and published states are distinct; readers see only a
  complete size+digest-verified asset.
- P8-A3: timeout, disconnect, window exit, SD removal/full/error, corrupt/duplicate chunk, and conflict
  leave a bounded recoverable state.
- P8-A4: power cuts at begin, mid-data, before/after sync, rename/publication, and cleanup yield exactly
  old or new complete content after boot, never partial or damage outside the root.
- P8-A5: v1 resume is same boot/window/principal via authoritative query; cross-reset resume requires a
  later authenticated atomic journal. Existing HTTP/SD hardware gates pass.
- P8-A6: the production candidate repeats the image, linked/private allocation, runtime-floor forecast,
  and cumulative contingency gate before on-device BLE upload is enabled.

### Phase 9: Versioned GATT upload and native macOS uploader

Add upload Control with response, Data write-without-response, and readable/notifiable Status only
after Phases 7–8. Status contains epoch, operation, monotonic sequence, SD-accepted offset, absolute
credit limit, and terminal result. Critical results remain queryable or use bounded indications.

- P9-A1: Data is admitted only after authorization and Status subscription. macOS fragments to the
  minimum of protocol cap and CoreBluetooth maximum, obeys absolute device credits and
  `canSendWriteWithoutResponse`, and resumes only on readiness callback before a stall deadline.
- P9-A2: malformed, stale, duplicate, reordered, replayed, sequence-wrap, credit-violation, congestion,
  and disconnect-at-transition cases have bounded deterministic outcomes; lifecycle control remains
  available and pacing violators are disconnected.
- P9-A3: fixed small/medium/large fixtures, sizes, digests, repetitions, throughput/failure ceilings,
  and current limits are declared before the run and pass end-to-end on macOS.
- P9-A4: sleep/wake, Bluetooth off/on, adapter loss, process kill/restart, cancellation, commit-response
  loss, and stale CoreBluetooth objects recover by rediscovery, reauthorization, and device query; no
  state restoration assumption or blind commit replay is used.
- P9-A5: the exact candidate repeats all image, linked/private allocation, runtime floor, power, and
  cumulative contingency gates before Phase 10.

### Phase 10: BLE upload promotion

- P10-A1: BLE upload, HTTP, SD, Wi-Fi, update, reset/cold boot, touch, panel, resource, and power gates
  pass on the exact release artifact.
- P10-A2: committed assets alone reach catalogue/font consumers; upload can be disabled without
  removing the Phase 6 foundation.
- P10-A3: protocol maintenance and backward rejection are documented; no promotion blocker remains.

## Stage D: Later iOS extension

Phase 11 proves iOS interoperability with the Phase 6 diagnostic protocol. P11-A1 covers native
discovery/schema/permissions; P11-A2 covers foreground/background, Bluetooth state, interruption, and
privacy; P11-A3 preserves separate exact-client/device evidence. Phase 12 adds an iOS upload adapter
only after Phases 10 and 11: P12-A1 reuses the released auth/framing/recovery contract without an
implicit firmware break, and P12-A2 passes end-to-end integrity and interruption gates. Neither phase
blocks macOS foundation or upload promotion.

## Advancement rules and next step

Every `Pass` maps each required criterion to append-only evidence. A changed dependency, feature graph,
criterion, or release artifact reopens affected gates. Missing numeric limits cannot be waived by
recording a measurement. Host evidence cannot satisfy device/physical criteria. Failed runs remain.

Commit `77569d3` is the durable source/build identity for the restartable network owner, exclusive
handoff, allocator provenance, bounded BLE transport, and Phase 1S workflow. Next, identify a safe
source-level recovery of at least the observed 1,228-byte floor deficit, or write and review a
workload/failure-model justification for a different floor before changing it. Build and full-flash
one new labeled artifact, then run the complete Wi-Fi regression and 20-cycle Phase 1S workflow.
Do not accept ADR-0011, implement Phase 3, begin macOS product service, mutate SD over BLE, or enable
BLE by default until the exact-artifact runtime gate passes.
