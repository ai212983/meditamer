# ADR-0011: Use a bounded coordinated BLE service foundation

- Status: Proposed
- Author: Codex
- Date: 2026-08-11
- Amends: [ADR-0009](0009-ab-firmware-update-foundation.md), runtime update admission and
  transport-preparation ordering only
- References: [BLE plan](../plans/ble-foundation-plan.md),
  [BLE implementation ledger](../plans/ble-foundation-ledger.md),
  [DRAM budget](../reference/dram-budget.md),
  [Bluetooth Core 6.1 Link Layer](https://www.bluetooth.com/wp-content/uploads/Files/Specification/HTML/Core-61/out/en/low-energy-controller/link-layer-specification.html),
  [Apple CoreBluetooth peer identity](https://developer.apple.com/documentation/corebluetooth/cbpeer/identifier)

## Context

The product must retain permanent Wi-Fi, signed A/B update support, and BLE on the original
ESP32-WROVER-E. Permanent BLE means compiled into both A/B images, not continuously active. The
current rebuilt candidate fits at 1,831,536 bytes, leaving 69,008 bytes below the 1,900,544-byte ceiling, but
ESP32 Bluetooth reserves 65,536 internal bytes before linking and its controller creates additional
internal-heap state. The linked candidate has a 33,980-byte CPU0 stack remainder before runtime use.

The selected ESP32 adapter disables BT modem sleep. Unbounded advertising or connected idle would
therefore create an unaccepted power cost. The diagnostic service also needs a privacy and
rogue-connection policy before it can run on hardware.

Wi-Fi's controller and Embassy network runner are long-lived. Keeping them resident while suppressing
HTTP traffic is still a coexistence configuration; it does not reclaim Wi-Fi or coexistence memory.
The existing HTTP listener can be gated, but there is no common radio epoch, quiesce acknowledgement,
or active-operation arbitration yet.

ADR-0009 makes firmware update the base flash owner and requires quiet hardware around
cache-disabled flash work. Its current transport preparation predates BLE and has no BLE-close
handshake. The update transaction must never begin flash erase/write while a BLE controller or late
controller callback can still be active.

## Decision

Adopt one base `RadioCoordinator` policy task, one separately owned BLE runtime task, and a
coex-enabled dual-stack configuration with serialized product traffic. This ADR covers only a
non-mutating diagnostic BLE foundation. BLE asset upload remains blocked on its later security and
operation ADRs. This ADR adds an atomic update grant before ADR-0009 transport preparation; it does
not change that ADR's flash map, authenticity, staging, rollback, confirmation, or recovery rules.

### Ownership and lifecycle

The coordinator exclusively owns admission, deadlines, service epochs, `TrafficLease`, and
`UpdateGrant`. The Wi-Fi task retains its controller and network runner. The HTTP/upload owner retains
listener, request, upload-session, and SD-operation lifecycle. The BLE task alone owns controller and
host initialization, advertising, connection, GATT, callback ingress, and teardown. UI providers and
loadable apps receive commands and status only.

At boot the coordinator generates a random nonzero `boot_generation: u64`. Each accepted BLE request
allocates a nonzero `service_epoch: u64` before quiescence starts. Epochs never wrap within a boot;
exhaustion disables BLE. Every command and acknowledgement carries the boot generation, service epoch
or update-grant identity, and exact transition kind. Only the transition currently awaited in the
current state can acknowledge; late, duplicate, wrong-kind, and wrong-state messages are counted and
ignored. Close is idempotent within an epoch.

| State | Meaning |
| --- | --- |
| `OffConfirmed` | Callback ingress is fenced, prior BLE work cannot enqueue, no traffic lease is held, and the latest desired HTTP policy is restored. |
| `QuiescingTraffic` | New traffic admission is closed and the HTTP/upload owner is draining to a matching acknowledgement. No BLE start grant exists. |
| `StartingBle` | Traffic is quiesced and the BLE task may initialize only for this epoch. |
| `Advertising` / `Connected` | One bounded diagnostic window or connection is active. |
| `ClosingBle` | New GATT work is closed and callback-quiescent BLE teardown is awaited. |
| `RestoringTraffic` | BLE is confirmed off; the latest desired traffic policy is being restored. |
| `Backoff` | BLE is confirmed off and traffic restored, but a recoverable failure delays another open. |
| `UpdateReserved(grant, Option<TrafficLease>)` | BLE admission is closed, one updater exclusively holds the runtime update grant, and any traffic lease retained during preemption remains explicit state. |
| `FaultedOwnershipUnknown(Option<TrafficLease>)` | Controller/callback quiescence is unproved; any held traffic lease is retained, HTTP/upload admission remains closed, and BLE and runtime update remain disabled until reboot. |

The BLE task may acknowledge `BleOffConfirmed(boot_generation, epoch, Close)` only after advertising and the connection stop, callback
admission for the epoch is atomically revoked, later callbacks cannot enqueue work, the host and
controller are disabled/deinitialized, the bounded in-flight callback count reaches zero, and old
queues are drained or invalidated. That acknowledgement permits only `RestoringTraffic` or an atomic
transition to `UpdateReserved` while retaining the traffic lease; it is not global-off evidence. Only
the coordinator enters `OffConfirmed`, after matching `TrafficRestored` evidence or after proving that
no traffic lease was acquired. A timeout enters `FaultedOwnershipUnknown` while retaining any held
lease; best-effort teardown is not safe-off evidence and cannot be retried.

A recoverable failure may enter `Backoff` only after BLE-off and traffic restoration are confirmed.
One explicit request may retry after one second. Three consecutive recoverable failed windows disable
BLE until reboot; the count resets only after a complete successful window, confirmed teardown, and
traffic restoration. Lifecycle control uses reserved capacity that ordinary/data traffic cannot
consume; exact queue depths remain implementation-ledger budgets.

### Availability and diagnostic identity

BLE never opens at boot. An explicit local service action or authorized serial diagnostic request
opens one window. Advertising lasts at most 60 seconds, one connection lasts at most 60 seconds, idle
connection time is 30 seconds, and the absolute initialize/advertise/connect/close window is 120
seconds. Connection churn, valid or malformed GATT, CCC writes, notifications, and callbacks cannot
extend a deadline. Teardown must acknowledge within two seconds.

Each window starts a new confirmed controller-power epoch. Before controller enable, generate a
random-static address, force address bits 47:46 to `0b11`, reject an all-zero/all-one 46-bit random
part or the immediately previous address, and then hold it unchanged until confirmed full controller
disable/reset. Ambiguous teardown forbids address rotation and another window. Production telemetry
never logs the address.

Use legacy connectable/scannable undirected advertising (`ADV_IND`), LE 1M, channels 37–39, a fixed
250 ms interval, allow scan/connect from any central, and 0 dBm TX power. The advertisement contains
only Limited Discoverable plus BR/EDR-not-supported flags and the complete 128-bit diagnostic service
UUID. The scan response contains only the complete generic name `Meditamer`. Phase 4 pins and tests
the exact encoded bytes and 31-byte limits.

Diagnostic v1 exposes one primary service and exactly three unauthenticated characteristics:

| Value | Properties | Fixed contract |
| --- | --- | --- |
| Build Info | Read | 16 bytes: schema/protocol/capabilities, eight-byte pre-link build-manifest digest prefix, four reserved zero bytes. |
| Echo | Read, Write Request, Notify | 1–32 bytes; subscription required; at most one equal-length response, four writes/second and 16/connection; payload is never logged. |
| Lifecycle Status | Read, Notify | 8 bytes: schema, state, saturating seconds remaining, RX-drop count, and TX-timeout count; one coalesced pending notification. |

Build Info has no timestamp, dirty marker, path, serial, per-device value, or credential identity. The
build-manifest digest covers the durable source/config identity and is computed before link; it is not
a self-referential hash of the image containing it. Its prefix intentionally permits firmware-version
fingerprinting during an opened window.
Exclude peer address, Wi-Fi/network state, update state/key, SD paths, catalogue contents, user data,
and attacker-supplied echo bytes from status, telemetry, and advertising.

Diagnostic v1 is non-bondable: it initiates no pairing, stores no bond key or allowlist, writes no
peer state to flash, and rejects inbound SMP pairing as unsupported. This deliberately accepts bounded
nearby-central denial: an unauthenticated central may occupy the single connection for at most 60
seconds, but cannot extend the absolute window, mutate state, or starve reserved close/update control.
The product does not claim availability against a continuously present attacker. Multiple simultaneous
matching `Meditamer` advertisements are an explicit ambiguous-device error.

The macOS client waits for CoreBluetooth `.poweredOn`, scans by service UUID, selects only a current
unambiguous discovery, stops scanning, discovers the expected service/characteristics, and validates
schema and properties before subscribing or writing. It never treats name, a saved `CBPeer.identifier`,
`retrievePeripherals`, or state restoration as device authority. It discards all CoreBluetooth objects
after disconnect, reset/power-off, adapter loss, sleep/wake, or window close, then rescans and
rediscovers. At one metre line-of-sight, the proposed discovery floor is 99/100 windows within ten
seconds and every window within 30 seconds.

### Coexistence and traffic serialization

Compile BLE with coexistence enabled and leave the Wi-Fi controller, link, and Embassy runner
resident. Product traffic is serialized by a real `TrafficLease`; listener telemetry or polling is not
a quiescence acknowledgement. The HTTP/upload owner atomically closes new accept and route admission,
tracks every accepted socket/request/SD roundtrip, and preserves later user policy changes as the
latest desired state. An in-flight operation gets 12 seconds to finish, followed by a bounded abort and
two-second acknowledgement deadline for its socket, SD roundtrip, and persistent upload session.

Only matching `Quiesced(boot_generation, epoch, lease, policy_generation)` evidence proving zero
in-flight traffic permits BLE start. Failure rejects the BLE open without initializing BLE. While
leased, the Wi-Fi controller/link/DHCP may remain resident but no HTTP product payload, upload
mutation, scan, or reconnect begins. After BLE-off confirmation, restore the latest desired policy,
not a stale snapshot, and acknowledge restoration before `OffConfirmed`. DHCP/link failure after
policy restoration is Wi-Fi-degraded recovery, not ambiguous BLE ownership. Every evidence record
names controller, link, listener, traffic, BLE, advertising, and connection state independently.

Full Wi-Fi controller teardown and recreation is an optional experiment only. It may not claim memory
or power savings until it closes sockets, cancels the network runner, drops/recreates the controller,
reacquires DHCP, and passes rollback tests. Its absence does not block the foundation.

### Firmware-update precedence

Runtime update admission outranks BLE and is atomic. A status check or `OffConfirmed` observation is
not authority. The coordinator enters `UpdateReserved(grant, traffic_lease)` before returning a
non-cloneable grant; all BLE opens are rejected until it is consumed. The optional traffic lease is
present when update preemption retained traffic quiescence and absent when reservation began from a
globally confirmed off state.

| Current state | Update request outcome |
| --- | --- |
| `OffConfirmed` / `Backoff` | Atomically enter `UpdateReserved`. |
| `QuiescingTraffic` | Before reservation, either cancel quiescence and await matching `TrafficRestored`, then reserve from `OffConfirmed`, or finish acquisition, accept matching `Quiesced`, and enter `UpdateReserved(grant, Some(lease))`. Never abandon a pending lease request across the state change. |
| `StartingBle` / `Advertising` / `Connected` | Enter `ClosingBle(reason=Update)` and require the callback-quiescence fence. |
| `ClosingBle` | Join the existing close; never issue a second teardown. |
| `RestoringTraffic` | Reserve while retaining the lease only after the HTTP owner confirms restoration cancellation and lease retention; otherwise finish restoration and reserve from `OffConfirmed`. |
| `UpdateReserved` | Reject another principal; only the same grant identity may retry idempotently. |
| `FaultedOwnershipUnknown` | Reject with `BleOwnershipUnknown`; reboot is the only recovery. |

Every API that changes update transport, session/digest phase, flash, or OTA metadata requires the live
grant: transport preparation/end, begin, stream preparation, chunk write, finish, activate, abort, and
release. Read-only status needs no grant. Existing serial call sites must be reordered through this
authority; `FWPREPARE` is not itself a grant.

Only the callback-quiescent BLE-off acknowledgement may permit update admission from a state that
could own BLE. Timeout never prepares
transport, parks the other core, erases, writes, verifies, or alters OTA metadata. The grant remains
held while a session exists or activation is pending. Abort or terminal cleanup first makes update
state safe and consumes the grant. On a non-reboot exit, a retained traffic lease transitions to
`RestoringTraffic`; an absent lease transitions to `OffConfirmed`. Traffic admission never reopens in
`FaultedOwnershipUnknown`. Successful activation reboots. No path admits BLE between verification and
a pending activation/session decision. Boot selection and candidate confirmation run under
boot-exclusive authority before BLE admission.

### Resource and power acceptance

Every Phase 3–6 candidate keeps these hard limits:

| Resource | Hard limit |
| --- | ---: |
| Application image | <=1,900,544 bytes |
| CPU0 runtime stack minimum | >=8,192 bytes; >=12,288 target |
| Touch-core stack minimum | >=1,024 bytes |
| Internal free memory | >=16,384 bytes |
| Longest active scheduling gap | <=16 ms |
| Post-close heap/largest-block drift after warm-up | <=1,024 bytes and non-monotonic |

These are durable limits. Per-phase contingency allocations, symbol budgets, and borrowing decisions
remain in the implementation plan/ledger and may change only while every candidate retains the
absolute limits.

The following product choices require human acceptance before this ADR can become Accepted:

| State | Maximum incremental average | Maximum incremental peak |
| --- | ---: | ---: |
| BLE compiled but runtime off | 2 mA | 10 mA |
| Advertising at 250 ms / 0 dBm | 70 mA | 250 mA |
| Connected idle | 70 mA | 250 mA |
| Rate-limited echo exchange | 90 mA | 300 mA |

Incremental energy for the complete 120-second window is <=50 J. Runtime-off current returns within
2 mA of its pre-window value no later than two seconds after `OffConfirmed` and remains there for ten
seconds.

Measure at one recorded device-input voltage and ambient temperature with a calibrated fixed-range
analyzer sampling voltage/current simultaneously at >=1 ksample/s. Worst-case current uncertainty at
the selected range is <=0.5 mA. Record accuracy, resolution, range/shunt, bandwidth, calibration, raw
samples, artifact, display/frontlight/touch/SD state, and each Wi-Fi/listener/radio state.

Runtime-off delta compares paired source-identical builds made with the same target, profile,
optimization, linker settings, and features except BLE. Active-state deltas use the BLE artifact's own
immediately preceding runtime-off baseline. Run each state three times with 30 seconds before and after
the window; use the worst guarded result. Average is a stable ten-second interval; peak is the maximum
10 ms moving average; energy is sample-wise input-power delta integrated against linearly interpolated
pre/post runtime-off power.

Pass current only when observed increment plus analyzer error and half pre/post baseline drift is at
or below the ceiling. Apply corresponding voltage/current uncertainty to energy. Return passes when a
one-second moving average enters the pre-window value plus the uncertainty guard and 2 mA within two
seconds, then remains there ten seconds. Missing identity, states, samples, calibration/uncertainty, or
a recovered post-baseline invalidates the run; exceeding a guarded ceiling is failure.

### Artifact promotion

BLE remains non-default until foundation promotion. Promotion establishes one canonical production
release artifact with BLE compiled into both A/B images and reruns every device, resource, power,
Wi-Fi, update, and physical gate. Temporary profile names and optimization settings remain
implementation-ledger details; binary similarity is not evidence.

## Consequences

- BLE is available for explicit macOS service without continuous advertising or a persistent public
  device identity.
- Update can never overlap ambiguous BLE ownership, at the cost of rejecting rather than forcing an
  update when teardown fails.
- Wi-Fi reconnection pauses during a BLE window, but controller/link state can survive and avoids an
  unproved full teardown/recreation path.
- The fixed Bluetooth DRAM reservation remains paid even while runtime-off; teardown can recover only
  controller/host runtime allocations.
- The no-modem-sleep adapter may fail the proposed power budget and park promotion until the stack or
  lifecycle changes.
- The diagnostic service is intentionally too weak for uploads; adding mutation requires a new trust,
  authorization, framing, and storage decision.

## Alternatives considered

- **Continuous advertising:** rejected because the controller lacks modem sleep and no continuous
  power/tracking budget is accepted.
- **Run Wi-Fi product traffic concurrently:** rejected because it weakens deterministic lifecycle,
  resource, and upload ownership evidence.
- **Tear Wi-Fi down for every window:** deferred as a separate experiment because current ownership is
  long-lived and recreation/DHCP rollback is unproved.
- **Let update forcibly reset BLE and continue flashing:** rejected because late callbacks or
  cache-disabled controller work could violate ADR-0009's quiet-flash boundary.
- **Use a stable hardware address or serial in advertising:** rejected because macOS can rediscover a
  per-window identity and the diagnostic foundation needs no persistent tracking handle.
- **Approve power after measurement:** rejected because any observed value would otherwise pass; the
  product ceiling must be chosen before the device run.
