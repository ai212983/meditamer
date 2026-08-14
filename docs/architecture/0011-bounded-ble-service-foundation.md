# ADR-0011: Use a bounded coordinated BLE service foundation

- Status: Proposed
- Author: Codex
- Date: 2026-08-11
- References: [BLE plan](../plans/ble-foundation-plan.md),
  [BLE implementation ledger](../plans/ble-foundation-ledger.md),
  [DRAM budget](../reference/dram/dram-budget.md),
  [Bluetooth Core 6.1 Link Layer](https://www.bluetooth.com/wp-content/uploads/Files/Specification/HTML/Core-61/out/en/low-energy-controller/link-layer-specification.html),
  [Apple CoreBluetooth peer identity](https://developer.apple.com/documentation/corebluetooth/cbpeer/identifier)

## Context

Meditamer needs Wi-Fi and an explicitly opened BLE service on the original ESP32-WROVER-E. A product
configuration selects Wi-Fi or BLE as the default owner of the shared radio and its runtime
resources. Production firmware includes both stacks, while BLE advertising remains explicit and
bounded in either configuration.

The ESP32 shares internal memory and radio resources between Wi-Fi and Bluetooth. Device testing
validated exclusive handoff by fully stopping Wi-Fi before BLE uses the radio, then restoring it.
This ADR applies the same handoff to either configured default and requires evidence for each. The
selected Bluetooth adapter disables BT modem sleep, so bounded availability is part of the power model.

Firmware delivery remains a separate product choice: USB-only flashing, on-flash A/B updates, and an
SD-assisted updater are all candidates. The current A/B layout supplies capacity evidence rather than
a permanent BLE ceiling. A future runtime updater receives flash authority after radio and callback
quiescence is confirmed.

## Decision

Use one base `RadioCoordinator`, one Wi-Fi owner, and one BLE runtime task. A validated configuration
selects Wi-Fi or BLE as the default owner. The coordinator starts that owner, grants the other owner a
bounded exclusive lease when requested, and then restores the configured default. Exactly one radio
stack owns the hardware and shared runtime resources at a time.

BLE ownership and BLE visibility are separate. A BLE-default configuration may keep the initialized
BLE stack as the non-advertising idle owner. Advertising and connections still require an explicit,
time-bounded diagnostic window.

This ADR approves read-only build/status and bounded echo diagnostics. Separate decisions govern BLE
asset upload, pairing, bonding, device authorization, and firmware delivery.

### Ownership and lifecycle

- The coordinator owns configuration, admission, deadlines, epochs, exclusive leases, and update grants.
- The Wi-Fi owner owns its controller, runner, listener, requests, uploads, and SD-operation lifecycle.
- The BLE task owns its controller, host, advertising, connection, GATT, callbacks, and teardown.
- UI providers and loadable apps receive commands and status; they own none of these resources.

The coordinator uses a random nonzero boot generation and service epoch for each accepted request.
Commands and acknowledgements identify the boot, epoch, and transition. Late, duplicate, wrong-kind,
or wrong-state acknowledgements are counted and ignored.

| State | Meaning |
| --- | --- |
| `Serving(owner)` | The configured default or a temporary borrower owns the radio. BLE additionally tracks idle, advertising, or connected service state. |
| `Quiescing(owner)` | New work is closed and the active owner's services, controller, callbacks, queues, and task resources are being drained or stopped. |
| `OffConfirmed(next_owner, lease)` | Prior ownership is released, callback ingress is fenced, queues are reclaimed, and the exclusive lease names the next owner. |
| `Starting(owner)` | The next owner may initialize for the matching lease. |
| `RestoringDefault(owner)` | The configured default owner and its latest desired service policy are being restored. |
| `Backoff` | Ownership is known and the configured default is safe, while a recoverable failure delays another request. |
| `UpdateReserved` | If runtime updating exists, one updater holds the exclusive mutation grant and radio admission is closed. |
| `FaultedOwnershipUnknown` | Radio or callback quiescence is unproved; BLE and runtime flash mutation remain disabled until reboot. |

The default is resolved before radio startup. A later configuration change uses the same lease
transition, takes effect after the active lease, and restores the latest requested owner.

Wi-Fi quiescence closes new listener and route admission, waits up to 12 seconds for accepted work,
then performs a bounded abort with a two-second acknowledgement deadline. It completes after the
network runner and controller stop, callback admission closes, in-flight callbacks reach zero, queues
are reclaimed, and the resource snapshot stabilizes. Matching transition evidence establishes
`OffConfirmed`; status polling and best-effort teardown provide diagnostic evidence only.

BLE handoff or runtime-update preparation follows the same fence: advertising and connections stop,
callback admission closes, in-flight callbacks reach zero, queues are drained or invalidated, and the
host and controller stop. Closing a diagnostic window under BLE-default returns to non-advertising
BLE idle; transferring ownership performs the full fence. A quiescence timeout enters
`FaultedOwnershipUnknown`, whose recovery is reboot.

A recoverable failure enters `Backoff` after ownership is known and the configured default is safe.
One explicit request may retry after one second. Three consecutive failed windows disable BLE until
reboot; a complete successful window and default-owner restoration reset the count. Reserved control
capacity protects close and recovery from ordinary-traffic saturation.

### Availability, identity, and diagnostic service

A validated configuration sets these deadlines; current defaults are:

- advertising: 60 seconds;
- connection: 60 seconds, with a 30-second idle timeout;
- complete visibility window: 120 seconds;
- teardown acknowledgement: two seconds.

All values are positive; idle is at most connection, and advertising and connection are at most the
complete window. Connection churn, GATT traffic, CCC writes, notifications, and callbacks preserve
the configured deadlines.

Each confirmed controller-power epoch gets a new random-static address. Its two most significant bits
are `11`; valid generation excludes all-zero, all-one, and immediately repeated random parts. The
address stays fixed within the epoch and telemetry omits it. Ambiguous teardown blocks address
rotation and another window.

Use legacy `ADV_IND` because the target ESP32-WROVER-E implements Bluetooth 4.2, while extended
advertising starts with Bluetooth 5. `ADV_IND` lets an unpaired central scan and connect to GATT.
Legacy advertising uses LE 1M and primary channels 37–39; using all three improves discovery
resilience. A validated configuration selects interval and transmit power; defaults are 250 ms and
0 dBm. The 31-byte advertising payload carries flags and the diagnostic service UUID, while the scan
response carries the generic name `Meditamer`.

Diagnostic v1 has one primary service and three unauthenticated characteristics:

| Value | Properties | Contract |
| --- | --- | --- |
| Build Info | Read | Fixed 16-byte schema, protocol, capabilities, build-manifest digest prefix, and reserved fields. |
| Echo | Read, Write Request, Notify | 1–32 bytes; subscription required; one equal-length response; at most four writes/second and 16/connection. |
| Lifecycle Status | Read, Notify | Fixed 8-byte schema, state, remaining time, RX drops, and TX timeouts; one coalesced pending notification. |

Build Info identifies a firmware build through the manifest digest prefix. Its fixed exposure omits
timestamps, dirty markers, local paths, serials, per-device values, and credential identity.
Advertising, status, and telemetry omit peer addresses, network state, update state or keys, SD paths,
catalogue contents, user data, and echo payloads.

Diagnostic v1 is non-pairing and non-bonding, stores no peer state, and exposes read/echo/status
operations only. An unauthenticated central may occupy the single connection until its deadline;
this bounded nearby denial is accepted. Availability against a continuously present attacker is out
of scope. Multiple matching devices are an explicit ambiguous-device error.

The macOS client establishes device authority through one current, unambiguous, service-filtered
discovery followed by schema and characteristic validation. Names, saved `CBPeer.identifier` values,
retrieved peripherals, and restored CoreBluetooth state are hints only. After disconnect, adapter
reset, sleep/wake, or window close, the client discards cached objects and rediscovers. The proposed
discovery gate is 99 of 100 windows within ten seconds and every window within 30 seconds at one metre
line-of-sight.

### Firmware-update independence

Firmware delivery has its own decision space: mechanism, partition map, storage reserve, signature
format, rollback policy, and recovery design. This ADR contributes the shared runtime-flash exclusion
rule.

USB full flashing runs outside application-firmware ownership. A runtime updater—whether its image
arrives over serial, Wi-Fi, BLE, or SD—uses one live, non-cloneable `UpdateGrant` for every API that
can erase or write firmware flash or boot metadata. Update admission outranks radio leases, joins an
in-progress close, and proceeds after both stacks and their callbacks are confirmed off. Ambiguous
ownership returns an update error before flash preparation.

The update design supplies authenticity, target/layout checks, read-back, power-loss behavior,
activation, rollback, and recovery. An SD-resident image follows the same safety contract.

### Resource and power acceptance

Every production candidate must satisfy these runtime limits:

| Resource | Hard limit |
| --- | ---: |
| CPU0 runtime stack minimum | >=8,192 bytes; >=12,288 target |
| Touch-core stack minimum | >=1,024 bytes |
| Internal free memory at admission | >=16,384 bytes |
| Longest active scheduling gap | <=16 ms |
| Post-close heap/largest-block drift after warm-up | <=1,024 bytes and non-monotonic |

The production image fits the selected release layout. Storage headroom is advisory and reported;
image-size and reserve limits come from that release layout.

These incremental power ceilings require human acceptance before this ADR becomes Accepted:

| State | Maximum average | Maximum peak |
| --- | ---: | ---: |
| BLE linked, controller off | 2 mA | 10 mA |
| BLE-default non-advertising idle | 70 mA | 250 mA |
| Advertising (default 250 ms / 0 dBm) | 70 mA | 250 mA |
| Connected idle | 70 mA | 250 mA |
| Rate-limited echo exchange | 90 mA | 300 mA |

The default 120-second window adds at most 50 J. Other visibility deadlines require an accepted energy
ceiling and power validation. Current returns to within 2 mA of the configuration's pre-window
default-owner baseline within two seconds after restoration and remains there for ten seconds.

Measure three runs per state with a calibrated fixed-range analyzer sampling voltage and current at
at least 1 ksample/s. Linked-off delta uses paired source-identical builds; other deltas use the same
artifact and configuration's immediately preceding default-owner baseline. Retain raw samples,
identity, instrument uncertainty, and radio/display/touch/SD state. The worst guarded run must pass;
missing evidence invalidates the run.

### Promotion

BLE remains feature-gated until production evidence passes. Promotion names every supported
default-owner configuration; each configuration must fit its chosen firmware layout and pass the
device, runtime-resource, power, owner-restoration, diagnostic-protocol, macOS, and physical
regression gates. A firmware-layout change triggers remeasurement while preserving this architecture.

## Consequences

- BLE offers explicit macOS diagnostics with bounded advertising and a per-window identity.
- Product configuration can favor Wi-Fi service or BLE readiness through the same ownership model.
- Borrowing the radio interrupts the configured default owner and pays teardown and restoration
  latency. Wi-Fi restoration additionally pays controller restart, DHCP, and listener recovery.
- Ambiguous teardown favors safety over availability: BLE and runtime flash mutation stay disabled
  until reboot.
- Bluetooth's fixed linked-memory cost remains even while BLE is off; teardown recovers only runtime
  allocations.
- The adapter may still fail the proposed power budget and block promotion.
- Diagnostic v1 leaves uploads and firmware delivery to a separate trust and operation design.
- Firmware storage remains a release-architecture choice rather than a BLE hard limit.
- Supporting both defaults expands the power, recovery, and device acceptance matrix.

## Alternatives considered

- **Resident Wi-Fi/BLE coexistence:** rejected for the foundation because feasibility work could not
  preserve the internal-memory floor. It may be reconsidered only with new evidence.
- **Fix Wi-Fi or BLE as the permanent default:** rejected because default ownership is a product
  configuration while the safety and handoff model is shared.
- **Continuous advertising:** rejected because the controller lacks modem sleep and the accepted
  power and tracking budgets cover bounded operation.
- **Concurrent product traffic:** rejected because one radio owner and one storage owner give bounded,
  testable lifecycle behavior.
- **Force flash work after best-effort radio reset:** rejected because late callbacks or cache-disabled
  controller work could corrupt the update boundary.
- **Stable device identity in advertising:** rejected because the diagnostic service needs no
  persistent tracking handle and clients can rediscover each window.
