# Shared Wi-Fi Services and Waveshare Integration

- Status: Done — integration and recorded device gates complete; remaining qualification transferred to the active parent
- Last-reviewed: 2026-09-07
- Parent: [Connectivity and storage parity](../../plans/connectivity-storage-parity.md)
- Dependencies: [Shared storage and Waveshare SD](../../plans/shared-storage-and-waveshare-sd.md),
  [radio coordination](../../plans/ble/03-inkplate-radio.md)

## Outcome

Medinote/Waveshare provides the same Wi-Fi configuration, status, recovery, and HTTP-to-SD
asset operations as Meditamer/Inkplate. Both targets consume `platform/netstack` and one
shared HTTP/storage service implementation. Product presentation and enable policy may
differ; the network and file contracts remain compatible.

This is the active Medinote networking requirement that replaces the earlier product-roadmap
deferral in [Phase 4](../../plans/product-target-axis-phase-4-shared-runtime.md). The shared implementation
and device gates pass; wider physical qualification is tracked by the active parent.

## Current boundaries

- [The shared network runtime](../../../platform/netstack/Cargo.toml) already separates chip
  selection from Wi-Fi driver/scan/connect/recovery, network-owner epochs, configuration,
  persistence protocol, listener/IP truth, and telemetry.
- [The Medinote target](../../../targets/medinote-waveshare/Cargo.toml) composes the `wifi-storage`
  feature, shared netstack/http-upload service and SD owner. Its resource budget remains
  target-specific: the image maps 156,672 bytes of PSRAM for network buffers and uses a
  65,536-byte reclaimed internal radio heap.
- HTTP routing and upload handling live in `platform/http-upload` and are consumed by both
  Meditamer and Medinote through target host adapters. The
  [existing HTTP workflow](../../guides/wifi-asset-upload.md) remains the compatibility baseline.
- The [SD plan](../../plans/shared-storage-and-waveshare-sd.md) owns the S3 block driver and common
  operation owner. SD-backed persistence and end-to-end uploads use that shared storage path.

Current S3 product evidence covers a normal product boot, persisted SSID/password loaded
from SD in a build without environment credentials, direct 32,769-byte and staged 24,577-byte
byte-exact readback, and HTTP authentication/path and cleanup cases: missing or invalid token
401, invalid path 400, abort cleanup and short commit 400. Wider card classes, power-loss and
interruption coverage remain unqualified.

## Contract and decisions before enablement

| Concern | Required disposition |
| --- | --- |
| Network operational truth | Keep one owner in `platform/netstack`; no Medinote copy. |
| Product enable policy | Target installs the product policy through existing `NetHost` ports. |
| Wi-Fi credentials | Use `netstack::config` validation, codec, request correlation and persistence port. |
| HTTP protocol | Preserve current v1 routes, status/error mapping, path restrictions and hostctl compatibility. |
| FAT and upload sessions | Use the sole shared storage owner; no HTTP-owned card access. |
| Radio and sleep | Explicit target policy and bounded quiescence, qualified with BLE plan 03. |
| Resources | Target-specific internal/DMA/stack budgets; optional PSRAM only for eligible allocations. |

The initial parity path is the existing `/config/wifi.cfg` format through the shared SD
owner. Missing media and failed persistence must be visible; an in-memory test backend is
not persistence acceptance.
BLE bonds use the flash-backed contract in [BLE connections](../../plans/ble/01-connections.md),
independent of SD presence or Wi-Fi credential storage.

The S3 target's `AuthenticatedListener` policy requires a configured upload token before
the listener is exposed. The shared HTTP v1 service preserves exact token checks, `/assets`
mutation restrictions and existing host compatibility; it does not claim TLS. [BLE asset
upload](../../plans/ble/06-asset-upload.md) owns its separate security and common-protocol decision.

Capability parity alone does not enable simultaneous Wi-Fi/BLE. Preserve Inkplate's
accepted exclusive handoff and use the S3 target adapter's exclusive BLE/Wi-Fi policy through
[radio coordination](../../plans/ble/03-inkplate-radio.md). Do not infer runtime coexistence support
from chip capability or feature flags. Central-plus-peripheral BLE roles are a separate
decision in [BLE plan 05](../../plans/ble/05-simultaneous-roles.md).

## Delivery

### 1. S3 Wi-Fi bring-up

1. Establish S3 static, allocator, contiguous internal-memory, task-stack, DMA and optional
   PSRAM budgets using the full current target as baseline. Read the
   [DRAM guidance](../../reference/dram/dram-budget.md), but do not copy ESP32 linker values
   or reduce Inkplate's accepted memory floor to accommodate the second target.
2. Compose `platform/netstack` with the pinned S3 radio/HAL/RTOS dependencies. Keep a
   no-radio baseline and explicit capability feature profiles. Install configuration,
   telemetry, allocator and product-policy ports before spawning the owner.
3. Prove Wi-Fi alone first: provision credentials, scan/connect/DHCP, expose status,
   disconnect/recover/reassociate, and start/stop repeatedly without stale listener or
   owner state. Use a bounded diagnostic service/test backend before depending on SD.

Exit: the identified S3 artifact passes bounded Wi-Fi lifecycle checks and measured memory
and scheduling limits. This does not yet prove persistence or asset uploads.

### 2. Shared HTTP and storage integration

1. Census current HTTP routes, authorization, limits, transfer buffers, SD session bridge,
   abort paths, product callbacks and telemetry. Keep the reusable handler in
   `platform/http-upload` behind explicit resource/policy ports, preserving both consumers.
2. Keep `NetHost::serve` as the network-epoch service boundary and
   `NetHost::abort_service_work` as the drain/abort boundary. The SD plan supplies the
   shared storage session implementation; neither plan creates a competing upload owner.
3. Compose that service into Medinote with the proven SD transport. Wire persistent
   `NETCFG` and the shared `NET` control/status contract through thin target console
   adapters, and expose product enable/status actions through the UI owner.
4. Preserve HTTP v1 direct and chunked uploads, token checks, bounded request/body handling,
   path restrictions, error responses and hostctl fallback behavior. Add host conformance
   coverage at the shared boundary; target adapters retain only listener, resource and policy
   wiring.

Exit: both targets share the same HTTP/storage implementation, persist/reload credentials,
and complete byte-verified asset uploads using the existing host client.

### 3. Full product radio, storage and sleep lifecycle

- Under the selected radio policy, close admission before shutdown/handoff, drain or
  abort accepted HTTP/storage work, acknowledge quiescence, then stop the radio/card.
  A failed drain has an explicit bounded recovery outcome, not an indefinite sleep wait.
- After wake/restart, restore configuration and policy, establish fresh owner/session
  state, and make reconnect/readiness observable. Repeated cycles must not leak memory,
  retain stale acknowledgements, or publish partial uploads as complete assets.
- Verify BLE reconnect/control behavior across uploads, product UI responsiveness,
  interruption recovery and idle return-to-sleep/power behavior on each full image.
  Bounded coexistence feasibility tests may inform the radio-policy decision; concurrent
  operation is enabled in the product only after that policy and its qualification pass.

## Closeout (2026-09-07)

Closed for shared implementation and the recorded Inkplate and Medinote device
gates in [hardware matrix §2O](../../reference/hardware-test-matrix.md#2o-shared-network-and-http-service-2026-09-07).
The active [parent qualification checklist](../../plans/connectivity-storage-parity.md#shared-wi-fi-integration-closeout-2026-09-07)
now owns all unfinished acceptance: wider storage/card/power coverage, connected
BLE and upload-service policy coexistence. No pending hardware check is marked
passed by this administrative closeout. The original delivery and acceptance
criteria below are retained as history; full parity remains open in the parent.

## Validation and completion

Run host configuration, service and session tests plus ownership guards and both target
build/profile checks. Record ELF/static changes and measured runtime low-water/contiguous
internal-memory, stack, timing and power results separately from compile evidence.

Run the existing [Wi-Fi regression gate](../../guides/wifi-regression-gate.md) on Inkplate for
shared network/upload changes. Extend the canonical hostctl workflows for S3 console,
artifact, reset and target resource limits while preserving discovery, one-cycle,
three-cycle, panic/reboot, listener, reconnect and upload assertions. Do not substitute a
Wi-Fi-only probe for the full Medinote product gate or silently waive an assertion the
S3 workflow cannot yet collect. Orchestration stays in
`tools/hostctl/scenarios/*.sw.yaml`; Rust hostctl implements primitive actions.

Before any tuning/A-B rerun, perform the repository novelty gate against the archived
Wi-Fi/upload ledgers and exact knob/value. Ordinary new-target qualification does not
authorize repeating an already rejected tuning experiment.

Acceptance includes rebooted credential persistence, no-card behavior, full/removal/error
storage cases, direct and chunked upload readback, disconnect/abort/retry, repeated
start/stop and sleep/wake, and the selected BLE/Wi-Fi policy. Record identified board,
card, artifact, commands, logs and limits in the
[hardware matrix](../../reference/hardware-test-matrix.md). Update the existing workflow and
feature reference docs after implementation, keeping one owner per workflow.

Done means both complete products meet the parity checklist with target-specific device
evidence. Remaining qualification is owned by the active parent as recorded in the closeout
above; neither board waits for unrelated feature promotion on the other.
