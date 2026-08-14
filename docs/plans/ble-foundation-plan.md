# BLE Foundation and Upload Transport Plan

- Status: Active
- Last-reviewed: 2026-08-14
- Started: 2026-08-11
- Evidence: [BLE implementation ledger](ble-foundation-ledger.md)
- Related: [Exclusive-handoff capacity evidence](ble-phase1s-capacity-recovery.md),
  [asset upload transport](asset-upload-transport.md), [BLE service
  ADR](../architecture/0011-bounded-ble-service-foundation.md), [DRAM
  budget](../reference/dram/dram-budget.md)

## Objective and authority

Add production BLE support on the ESP32-WROVER-E without weakening Wi-Fi, runtime safety, or the
existing resource owners. First promote a bounded, non-mutating GATT diagnostic service verified with
native macOS CoreBluetooth. BLE asset upload remains an optional extension with its own security and
storage gates.

Product configuration selects Wi-Fi or BLE as the default owner of the shared radio and runtime
resources. The other owner may receive a bounded exclusive lease, after which the coordinator restores
the configured default. BLE visibility remains explicit and time-bounded in either configuration.

Firmware delivery and BLE are independent. Each candidate must fit its selected release layout; the
current A/B layout is one measured target, not a permanent storage ceiling. USB flashing, an on-flash
runtime updater, and an SD-assisted updater may evolve separately while any runtime flash mutation
uses the coordinator's confirmed-off boundary.

This plan is the execution authority for the BLE foundation and supersedes the overlapping BLE
sequence in the Proposed [asset upload transport plan](asset-upload-transport.md). The
[implementation ledger](ble-foundation-ledger.md) owns detailed history, measurements, artifact
identities, and failed runs.

## Current verified baseline

- The repository contains a pinned, bounded BLE controller/host transport and a non-default
  `ble-foundation` probe. It currently allocates one peripheral-capable connection slot; advertising,
  connection establishment, and the product diagnostic service are not implemented.
- The current coordinator can stop Wi-Fi and its HTTP/upload epoch, establish `OffConfirmed`, run a
  bounded BLE controller/host probe, stop BLE, and restore Wi-Fi, DHCP, and listener service.
- One identified artifact passed 20 exclusive handoff/restore cycles, the complete Wi-Fi regression
  gate, runtime-resource floors, and UART-drop checks. The capacity plan and ledger retain the exact
  device evidence.
- The implemented policy still starts and restores Wi-Fi as the default. Configurable Wi-Fi/BLE
  default ownership is an ADR decision awaiting implementation and proof.
- The selected ESP32 adapter disables Bluetooth modem sleep. Advertising, connected idle, and
  BLE-default idle therefore require explicit power acceptance.
- HTTP is the current transport into the sole SD/FAT operation owner. A later BLE upload adapter must
  use the same operation authority.

## Invariants

1. Production candidates include Wi-Fi and BLE; fitting BLE never removes either capability.
2. One `RadioCoordinator` owns configuration, epochs, admission, exclusive leases, and restoration.
   One Wi-Fi owner and one BLE task own their respective stacks; exactly one owns shared radio/runtime
   resources at a time.
3. The configured default is resolved before radio startup. Configuration changes use the same
   bounded handoff and restore the latest accepted policy.
4. Runtime firmware mutation, when present, outranks radio leases and begins only after both stacks
   and callback ingress are confirmed off. USB full flashing remains outside application ownership.
5. Callbacks perform fixed work and bounded enqueue only. Lifecycle control retains reserved capacity
   under connection, GATT, upload, or callback saturation.
6. Pools, queues, packet sizes, connection counts, paths, sessions, and configurable deadlines have
   explicit bounds and validation rules.
7. GATT framing is versioned and separate from upload operations. ATT receipt, queue admission, SD
   acceptance, sync, and durable publication are distinct states.
8. Every promoted configuration fits its selected firmware layout and passes the runtime-resource,
   power, owner-restoration, radio, client, and physical regression gates.

## Scope

This plan delivers configurable Wi-Fi/BLE default ownership, a time-bounded single-connection BLE
peripheral, a non-mutating diagnostic GATT service, native macOS acceptance, and optional bounded
asset upload through the existing SD owner.

BLE central operation, multiple simultaneous connections, pairing/bonding, and HID-over-GATT
accessories belong to a dedicated follow-on plan. The foundation must leave those roles possible
within the same BLE owner, but diagnostic v1 does not implement or reserve resources for them.
Classic/SPP, continuous public advertising, app-owned radio resources, deep-sleep integration, and
executable-package trust also remain separate work.

## Completed feasibility work

The following gates are complete; their criteria and evidence remain in the ledger rather than being
repeated here:

- dependency/source audit, bounded RX/backpressure, callback-woken finite TX waits, reproducible
  source identity, locked builds, and fixed-cost inventory;
- current-layout image and linked-section measurement;
- resident Wi-Fi plus BLE evaluation, which did not preserve the internal-memory floor;
- exclusive Wi-Fi/BLE teardown, allocator settlement, callback fencing, controller/host lifecycle,
  Wi-Fi restoration, and update-admission ordering;
- 20 exact-artifact handoff cycles and the complete Wi-Fi regression gate.

A dependency, feature graph, ownership rule, or relevant runtime-layout change reopens the affected
gate. Compilation or host tests cannot replace device evidence.

## Phase 2: Accept the BLE architecture

Review and accept [ADR-0011](../architecture/0011-bounded-ble-service-foundation.md) before product BLE
implementation.

- P2-A1: settle configurable default ownership, boot/transition identity, `OffConfirmed`, callback
  fencing, restoration, cancellation, close deadlines, retry/backoff, and ambiguous-ownership recovery.
- P2-A2: settle runtime-update exclusion without selecting a firmware-delivery or partition design.
- P2-A3: accept numeric power ceilings for linked-off, BLE-default idle, advertising, connected idle,
  exchange energy, complete-window energy, and return to the configured baseline.
- P2-A4: settle configurable visibility deadlines, advertising interval and transmit power, legacy
  `ADV_IND` parameters, controller-epoch identity, discovery SLA, fixed diagnostic schema, exposure,
  and the non-pairing/non-bonding policy.
- P2-A5: record which decisions require a successor ADR for upload security, persistent identity,
  pairing/bonding, central/accessory roles, or firmware delivery.

## Phase 3: Generalize the radio coordinator

Evolve the proven Wi-Fi-to-BLE handoff into the ADR state model and validated default-owner
configuration. Keep the current exclusive resource ownership; do not introduce a second controller
instance or a second storage owner.

- P3-A1: boot tests start the configured default, reject invalid configuration before radio startup,
  and expose the effective owner and policy generation.
- P3-A2: transition tests cover both directions, active-lease configuration changes, slow or aborted
  Wi-Fi work, BLE start/close failure, stale acknowledgements, latest-policy restoration, retry/backoff,
  and `FaultedOwnershipUnknown`.
- P3-A3: BLE-default idle owns the initialized BLE runtime without advertising. A Wi-Fi lease restores
  BLE-default idle after Wi-Fi controller, DHCP, listener, and accepted work close cleanly.
- P3-A4: runtime-update tests, when that capability is included, require a live non-cloneable grant for
  every flash/boot-metadata mutation and reject ambiguous ownership before flash preparation.
- P3-A5: lifecycle control remains available under saturated data queues. Wi-Fi regression and the
  exclusive-handoff hardware gate pass for every supported default-owner configuration.
- P3-A6: the exact candidate fits its selected release layout; linked sections, controller/task/private
  allocations, runtime floors, and storage headroom are recorded separately.

## Phase 4: Implement diagnostic peripheral v1

Replace the controller/host probe with the production BLE task, legacy advertising, and the ADR's
Build Info, bounded Echo, and Lifecycle Status characteristics. Diagnostic v1 remains one peripheral
connection with no pairing, bonding, peer persistence, SD mutation, paths, or product secrets.

- P4-A1: validate all configured deadlines and radio parameters. Defaults are 60 seconds advertising,
  60 seconds connected, 30 seconds idle, a 120-second complete window, two-second teardown, 250 ms
  advertising interval, and 0 dBm. Invalid relationships and unsupported values fail closed.
- P4-A2: byte-level tests cover the random-static controller-epoch address, legacy `ADV_IND` on LE 1M
  primary channels 37-39, payload/scan-response layout, UUIDs, permissions, schemas, quotas, and fixed
  exposure.
- P4-A3: host tests cover offsets, lengths, MTU limits, CCC churn, write quotas, notification
  coalescing/backpressure, disconnects, stale epochs, queue exhaustion, floods, and close/update
  precedence with bounded work.
- P4-A4: ATT payloads and all host/controller queues are compile-time bounded; ordinary traffic cannot
  starve close, watchdog, touch, owner restoration, or update exclusion.
- P4-A5: the exact candidate repeats Phase 3's layout, linked/private allocation, and runtime-floor
  accounting before device work.

## Phase 5: Prove the device and macOS lifecycle

Run one identified release artifact with native macOS CoreBluetooth and the complete display, touch,
Wi-Fi, HTTP, and SD workload.

- P5-A1: 100 windows pass service-filtered discovery, fresh schema discovery,
  read/write/subscribe/notify/close/restart, and stale-object rejection. At one metre line-of-sight,
  at least 99 windows discover within ten seconds and every window within 30 seconds.
- P5-A2: CPU0 stack remains at least 8,192 bytes (12,288 target), touch-core stack at least 1,024,
  internal free memory at admission at least 16,384, and the longest active scheduling gap at most
  16 ms. Heap/largest-block drift after warm-up remains at most 1,024 bytes and non-monotonic.
- P5-A3: every run records the configured default, active owner, Wi-Fi controller/link/listener/traffic,
  BLE controller/advertising/connection, and storage owner independently. Handoff restores the latest
  default policy without duplicate ownership.
- P5-A4: three guarded runs per state pass the ADR's average, peak, energy, uncertainty, and
  return-to-baseline ceilings for each supported default-owner configuration.
- P5-A5: controller and over-air evidence proves address and advertising behavior. Native macOS proves
  cache recovery; a raw central proves ATT/GAP/SMP, rogue connection hold, no-bond behavior, flooding,
  and the configured deadlines.
- P5-A6: Wi-Fi regression, touch/panel behavior, reset recovery, and any included runtime-update path
  pass on the same artifact. Host, device/serial, client, and physical evidence remain distinct.

## Phase 6: Promote the minimal foundation

Compile the proven diagnostic capability into every production candidate while keeping visibility
explicit and bounded.

- P6-A1: each supported configuration fits its selected firmware layout and reports storage headroom.
  A layout change reopens build, flash, boot, resource, and recovery evidence.
- P6-A2: the exact promoted artifact reruns Phase 5 without reduced repetitions, workloads, resource
  floors, power ceilings, or physical gates.
- P6-A3: configuration, user flow, protocol compatibility, troubleshooting, telemetry, and regression
  maintenance are documented. Completion promotes the BLE foundation independently of upload.

## Optional BLE asset upload

Asset upload starts only after the minimal foundation is promoted:

1. Accept a security and operation ADR covering the attacker model, trust anchor, peer authorization,
   integrity/confidentiality, replay protection, privacy identity, reset/recovery, allowed mutations,
   and absolute expiry. No SD allocation occurs before authorization.
2. Implement transport-neutral `begin`, sequential `chunk`, `commit`, `abort`, `query`, and `stat`
   through the sole SD task. HTTP and in-memory BLE adapters pass the same conformance suite.
3. Prove canonical paths, root confinement, quotas, free-space floors, SHA-256, one global mutation
   lease, atomic publication, and power-cut recovery. Readers observe either the old or new complete
   asset, never partial content.
4. Add versioned GATT Control, Data, and Status. Device credits and authoritative status bound macOS
   write-without-response pacing, reconnect, resume, interruption, and commit-response loss.
5. Declare fixture sizes, digests, repetitions, throughput/failure ceilings, and power/resource limits
   before device runs. Promote upload only after BLE, HTTP, SD, Wi-Fi, reset, touch, panel, resource,
   power, and any included runtime-update gates pass on the exact artifact.

iOS diagnostic and upload clients may reuse the released contracts after separate native discovery,
privacy, interruption, integrity, and physical-device evidence. They do not block macOS promotion.

## Advancement and next step

Every pass maps each criterion to append-only ledger evidence. Changed dependencies, feature graphs,
criteria, configurations, or release artifacts reopen affected gates. Missing limits cannot be waived
by measuring first; failed runs remain evidence.

The exclusive-handoff feasibility and Wi-Fi regression prerequisites are complete. The next step is
to review ADR-0011 for acceptance, then implement and host-test configurable default ownership before
adding product advertising or GATT behavior.
