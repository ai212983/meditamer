# Product/Target Axis Phase 4: Shared Runtime Design

- Status: Done — extraction verified in E-0007; Inkplate Wi-Fi gate passed and recorded in E-0010
- Last-reviewed: 2026-09-07
- Parent plan: [Product and target axis completion](../../plans/product-target-axis-completion.md)
- Execution evidence: [Product and target axis implementation ledger](../../plans/product-target-axis-completion-ledger.md)
- Decision: [ADR-0023 — Platform, board, product, and target axes](../../architecture/0023-platform-board-product-target-axes.md)

## Closeout (2026-09-07)

Ledger E-0010 closes the remaining Inkplate hardware gate using the identified
September 7 shared-storage/network artifact and its passing discovery, one-cycle
and three-cycle regression report. No panic or unexpected reboot was detected;
soak was skipped. E-0007 retains the extraction's source, host, build and ELF
validation. This closeout does not claim a fresh full-suite run on the current
working tree or qualify later upload-service policy changes.

Medinote adoption and wider physical qualification retain their separate owners
under the [parity plan](../../plans/connectivity-storage-parity.md). The design and migration
sequence below are retained as historical context.

## Outcome

Phase 4 creates one reusable network runtime and configuration capability. The runtime owns Wi-Fi,
Embassy networking, network policy and configuration state, listener/link/IP truth, and its
telemetry. Products that need networking supply service work and product policy; their targets select
the chip and install the concrete ports.

The extraction produces two platform crates:

```text
platform/runtime/                 # shared log-domain filtering
platform/netstack/                # network runtime, configuration, and telemetry
  src/
    config/                       # types, validation, codec, channels, persistence port
    host.rs                       # product service and resource ports
    owner.rs                      # restartable network owner
    telemetry/                    # network counters and snapshot
    wifi/                         # driver, scan, connect, recovery, status
```

`platform/netstack` is the sole owner of network operational truth. Meditamer consumes it now.
Medinote adoption is required by [connectivity and storage parity](../../plans/connectivity-storage-parity.md)
and delivered through [shared Wi-Fi services](../wifi/shared-wifi-services-and-waveshare-completed-2026-09-07.md), using these
contracts rather than adding a product-local implementation.

## Ownership boundary

| Concern | Owner after Phase 4 | Composition rule |
| --- | --- | --- |
| Wi-Fi driver, scanning, connection, DHCP, retry, and recovery | `platform/netstack` | Target selects the Espressif chip. |
| Network-owner epochs and radio handoff | `platform/netstack` + `platform/arbitration` | Target supplies the product service future. |
| Credentials, runtime policy, commands, validation, and status types | `platform/netstack::config` | Products consume one public contract. |
| Credential file codec and request/response protocol | `platform/netstack::config` | Persistence backends call the shared codec. |
| Credential bytes on SD, flash, or another medium | Target-selected storage backend | Backend performs physical I/O through the shared persistence port. |
| HTTP upload service and SD upload sessions | Current product service owner; shared extraction required by parity plans | Supplied through `NetHost`; the Wi-Fi and storage plans move the reusable service and sole storage owner intact. |
| Network enabled policy | Product | Published through one synchronous callback. |
| Link, IPv4, listener, admission, and network counters | `platform/netstack` | Product diagnostics read the platform snapshot. |
| Log filter mask and domain registry | `platform/runtime` | Network and product diagnostics share one registry. |
| Chip features, startup, task spawning, and port installation | `targets/*` | Each target composes one product, one board, and the platform crates. |

This keeps functionality together by domain. The persistence seam exists because the network owns
credential semantics while a target's single storage owner must serialize the physical I/O.

## Crate and feature model

`platform/netstack` is a shared workspace crate with no chip feature of its own. It declares
`esp-hal`, `esp-radio`, and `esp-rtos` without a chip selection; the final target selects `esp32` or
`esp32s3`, following `platform/console`'s existing feature-unification pattern. Literal target
dependency tables provide the matching `esp-wifi-sys-esp32` or `esp-wifi-sys-esp32s3` alias used by
the low-level diagnostics.

The crate retains behavior features such as `ble-foundation` and `telemetry-defmt`. Target features
forward to `netstack` directly. Network dependencies leave `products/meditamer`; each adopting target
resolves the shared crate with its own chip selection and patch set.

The public surface is bounded to:

- `config`: configuration types, commands, validation, codec, channels, and persistence port;
- `host`: product service and synchronous runtime inputs;
- `telemetry`: network snapshot and recorders needed by service owners;
- `wifi`: configuration/status snapshots and the RX-buffer diagnostic;
- runtime entry points and radio-handoff commands already consumed by targets and products.

Internal driver, scan, retry, and epoch modules remain private.

## Product and resource ports

Keep the established split between generic async work and installed synchronous callbacks:

- `NetHost::serve` supplies one product service future for each network epoch.
- `NetHost::abort_service_work` drains in-flight service work before radio handoff. Meditamer maps
  this to its existing upload abort.
- Existing callbacks report active service connections, storage roundtrips, service sessions, and
  update transport reservation.
- One policy callback reports whether the product currently enables networking. Meditamer maps it
  to its upload-enabled app state; Medinote maps it to its own service policy during adoption.
- Two diagnostic callbacks return a netstack-owned plain memory snapshot and contiguous-block probe.
  The target adapter converts from its allocator's types, so netstack does not depend on a product
  allocator.

Listener enable state, listener sequence, and radio-handoff admission move into netstack rather than
becoming product callbacks. They are network policy and operational state, and the HTTP service reads
them inward through the platform API.

All callbacks are installed once before the network task starts. Pre-install reads keep the current
safe defaults. The port uses function pointers and plain values; hot telemetry writes remain direct
atomics and async work remains allocation-free.

## Shared configuration and persistence

Move the complete `types/wifi.rs` domain into `netstack::config`, including `WifiCredentials`,
`WifiRuntimePolicy`, `NetConfigSet`, `NetControlCommand`, `WifiConfigRequest`,
`WifiConfigResponse`, result codes, and all Wi-Fi bounds/defaults. Move `WIFI_CONFIG_FILE_MAX` with
the codec.

Move all six network-configuration channels together:

- credentials updates;
- runtime-policy updates;
- complete config-set updates;
- network control commands;
- persistence requests;
- persistence responses.

The persistence pair is the `CredentialStorePort`. It keeps the existing bounded request IDs,
single outstanding request, response timeout, and stale-response rejection. The shared codec owns
SSID/password validation and the current `ssid=...` / `password=...` representation. A backend owns
only medium preparation, bounded read/write, and error mapping.

For Meditamer, the first backend remains inside the sole SD owner so credential I/O stays serialized
with uploads and other FAT work. It consumes the platform request and publishes the platform response;
its FAT engine, power sequencing, and upload-session exclusion remain storage concerns. Medinote uses
the same port with the persistent backend specified in the shared Wi-Fi plan. A bounded in-memory
backend provides host/bring-up coverage, not product persistence acceptance.

The reusable `NETCFG` parsing, policy sanitization, request sequencing, response classification, and
status formatting move with the configuration service. UART framing and command dispatch remain thin
transport adapters at the product/target boundary.

## Telemetry and logging

Move Wi-Fi connect/reassociation counters, link and IPv4 state, listener state, network-pipeline
counters, `WifiScanPhase`, and their recorders into `netstack::telemetry`. Split the current mixed
upload/network recorder on the real ownership boundary: HTTP request/body/SD timing stays with the
service owner; listener and network-pipeline state moves to netstack.

`netstack::telemetry::snapshot()` returns a plain `NetSnapshot`. Meditamer's observability snapshot
composes that value with product HTTP, SD, stack, and UI counters while preserving the serial metric
names and values. Other products may expose the network snapshot directly.

Move the generic log-filter mask, domain constants, and update operations to `platform/runtime`.
This avoids a callback from netstack into product observability and gives network adopters one diagnostic
control surface. Existing domain bits and serial commands remain stable during the extraction.

## Migration sequence

1. Add `platform/runtime` and `platform/netstack` manifests, target-specific dependency wiring, and
   public skeletons. Register both crates with reachability, formatting, code-size, and build checks.
2. Move the log filter and the complete network configuration domain. Repoint Meditamer's serial and
   SD adapters, then add host tests for validation, codec roundtrips, request correlation, timeouts,
   and the in-memory persistence backend.
3. Move network telemetry and split the mixed upload/network recorder. Compose Meditamer's snapshot
   from product and platform parts without changing emitted metrics.
4. Move `net/{mod.rs,host.rs,runtime.rs,runtime/off_resources.rs,wifi.rs,wifi/**}` wholesale. Replace
   product paths with the host, policy, allocator, configuration, logging, and telemetry APIs above.
5. Repoint `targets/meditamer-inkplate/src/net_host.rs`, product HTTP/storage consumers, serial
   commands, self-tests, and memory diagnostics. Remove the old `firmware::net` module and moved
   configuration/telemetry definitions once the census shows no consumers.
Each step leaves a buildable graph. The ledger records deviations and exact evidence; this document
owns the intended boundary and ordering.

Steps 1-5 are implemented and source/build/ELF-verified in ledger E-0007.

## Required Medinote adoption

The earlier networking deferral is replaced by the active
[connectivity and storage parity plan](../../plans/connectivity-storage-parity.md).
[Shared Wi-Fi services](../wifi/shared-wifi-services-and-waveshare-completed-2026-09-07.md) delivers S3 netstack composition,
persistent configuration, the shared HTTP upload service, and radio/sleep qualification;
[shared storage](../../plans/shared-storage-and-waveshare-sd.md) first proves the board's native 1-bit SD
transport and then extracts the common operation owner. Both require target-specific internal/DMA
memory and stack budgets before broad wiring. BLE integration follows the
[BLE roadmap](../../plans/ble/README.md), with a separately qualified radio policy per target.

The extraction's E-0007 implementation evidence is retained; E-0010 closes its Meditamer hardware gate.
S3 product qualification is independently required by the parity plans, not a retroactive proof
obligation for this extraction.

## Validation and exit gate

Before code motion, save the current module census and identified Meditamer release ELF section
totals. After each migration step, run formatting, orphan-module, code-analysis, Rust LOC, host-suite,
and affected target checks in the repository's `archbox` distrobox.

Phase 4 exits when:

- the census reports no `net` module under `products/meditamer` and no dependency from either
  platform crate to a product or board;
- the Meditamer target resolves and builds netstack with its ESP32 selection;
- configuration validation, codec, correlation, timeout, and persistence-port tests pass on host;
- Meditamer default, minimal, BLE, and updater build profiles retain their feature behavior;
- Meditamer's ELF comparison shows no unexplained section loss or material DRAM regression;
- linked static/channel movement is reconciled against the [DRAM budget](../../reference/dram/dram-budget.md),
  with no channel-depth increase accepted without measurement;
- the Meditamer network service preserves its serial protocol, credential persistence, listener,
  upload, and handoff behavior;
- the [Wi-Fi regression gate](../../guides/wifi-regression-gate.md) passes on the identified Meditamer
  artifact and its coverage is recorded in the
  [hardware test matrix](../../reference/hardware-test-matrix.md);
- Markdown links, documentation LOC, and whitespace checks pass.

Passing this gate delivers the parent plan's F1 network-operational-truth disposition. Public-surface
contraction and remaining compatibility cleanup stay in Phase 5.
