# 2026-03-20 Asset Upload Transport Plan

- Status: Deferred — BLE-primary transport is blocked by the Phase 1S runtime-capacity gate
- Last-reviewed: 2026-08-14
- Current disposition: Wi-Fi STA + HTTP remains the implemented and supported upload transport.
  BLE remains a product-direction candidate, but BLE upload work cannot begin until the
  [BLE foundation plan](ble-foundation-plan.md) passes its current-hardware runtime gate.

## Current state and reopening direction

- The transport-independent SD/FAT upload authority and integrity boundaries remain useful and
  stay separate from HTTP framing.
- The active branch contains the exclusive Wi-Fi/BLE handoff implementation, but the binding
  source-branch Wi-Fi run reached 15,156 bytes of internal free memory against the unchanged
  16,384-byte floor. The active branch still needs an exact-artifact device rerun.
- Do not remove Wi-Fi, reduce the floor, repeat closed radio-buffer experiments, or begin a GATT
  uploader from this document.
- Reopen this transport decision only after the BLE foundation proves applicable internal-memory
  recovery or a vendor-supported bounded RX-allocation path. Hardware with more applicable internal
  RAM, or a separately accepted iPhone-compatible transport, may also supersede this proposal.

## Problem

The current asset upload path is built around Wi-Fi and an on-device HTTP listener.

That approach has three problems:

1. It consumes substantial firmware space and operational complexity.
2. It depends on scan/connect/DHCP/listener behavior that has already proven fragile and expensive to debug.
3. It is a poor fit for a future mobile upload flow, especially from iPhone, where ad hoc device-local Wi-Fi flows are significantly less predictable than a direct companion transport.

The project goal is not "networking" by itself. The real product need is reliable asset transfer onto the SD card.

## Goal

Support asset upload from:

- desktop during development
- mobile in product use

With these constraints:

- iPhone support is mandatory
- the product should prefer one upload protocol and one firmware-side upload engine
- the transport should be simpler and smaller than the current Wi-Fi stack
- uploads should write assets to SD card with integrity checks and clear failure handling

## Proposed direction (not decided)

This is the recommendation this document argues for, not a ratified decision.
Nothing below has been adopted, and no work should treat it as settled.

- Do not make Wi-Fi the primary long-term asset upload transport.
- Do not make Bluetooth Classic the primary transport either.
- Adopt a transport-agnostic upload protocol with BLE as the primary external
  transport.

### Candidates still on the table

| Transport | Status | Standing objection |
| --- | --- | --- |
| Wi-Fi STA + HTTP | Implemented and working today; see [guides/wifi-asset-upload.md](../guides/wifi-asset-upload.md) | Firmware size and complexity; weak fit for iPhone |
| BLE | Proposed below; not built | Throughput for large assets is unproven on this board |
| Bluetooth Classic | Argued against below | SPP/RFCOMM is not a viable iPhone path |

The transport-agnostic upload protocol is worth building regardless of which
transport wins, since it is what keeps the choice reversible.

## Why Not Wi-Fi

Wi-Fi is misaligned with the actual requirement.

The system only needs a way to move files to SD card. The current Wi-Fi solution adds:

- discovery and scan logic
- connection management
- DHCP and local IPv4 readiness
- socket handling
- HTTP parsing and request lifecycle handling
- recovery behavior around listener and connectivity failures

That is a large amount of code and state for a device whose actual requirement is bounded file transfer.

Even if Wi-Fi can eventually be made stable, it remains a heavy default path for a problem that does not require network semantics.

## Why Not Bluetooth Classic

The board hardware can support Bluetooth Classic because it is based on the original ESP32 dual-mode radio.

However, Bluetooth Classic is not the right primary product choice because iPhone support is mandatory.

Generic Classic Bluetooth file-transfer approaches such as SPP/RFCOMM are not a safe cross-platform basis for iPhone support. Even when technically possible on ESP32, they are not the portable path for a desktop-plus-iPhone upload product.

That makes Bluetooth Classic a dead end for the primary transport decision.

## Proposed Solution

Build one upload protocol and keep the transport separate from the file-transfer logic.

### Firmware

Implement a small upload service centered on SD card operations:

- `begin_upload(path, size, content_hash)`
- `write_chunk(session_id, offset, data, chunk_hash)`
- `commit_upload(session_id)`
- `abort_upload(session_id)`
- `query_upload(session_id)`
- `list_assets()`
- `delete_asset(path)`

This service should own:

- session state
- chunk ordering and offset validation
- integrity checks
- SD write scheduling
- final commit/publish behavior

### Transport

Use BLE GATT as the primary external transport.

Suggested shape:

- one control characteristic for commands and status
- one data characteristic for chunk payloads
- notifications or indications for ack, progress, and errors

The firmware upload service should not know whether commands arrived via BLE or any future dev-only transport.

## Host Strategy

Use the same upload protocol on desktop and mobile.

That means:

- a desktop CLI or desktop companion app talks to the device over BLE
- an iPhone app talks to the same firmware protocol over BLE

The host-side implementation can be split into:

- shared protocol logic
- desktop transport adapter
- iPhone transport adapter

This preserves one upload mechanism at the product level even if host integrations differ by platform.

## Development Convenience

If desktop BLE iteration proves too slow or awkward, add a dev-only USB/UART adapter later.

Important: if that happens, keep the same upload protocol and only swap the transport framing.

That keeps product behavior aligned while still giving development a faster fallback.

## Non-Goals

This plan does not require:

- general network connectivity
- a web server on the device
- browser-first upload support
- Bluetooth Classic as a product dependency

## Historical Proposed Migration Direction

The sequence below is not currently executable. It is retained so a future reopening can evaluate
the original proposal without treating it as accepted work.

1. Freeze expansion of the Wi-Fi upload path as the default long-term solution.
2. Define the upload session protocol independent of transport.
3. Implement the firmware-side upload service around SD writes and integrity checks.
4. Add a BLE transport adapter.
5. Build a minimal desktop uploader for development.
6. Build the iPhone uploader against the same protocol.
7. Retain Wi-Fi only if a later, explicitly justified use case needs it.

## Expected Outcome

If this plan is followed, the project should end up with:

- a smaller and more focused firmware upload surface
- one product-grade upload mechanism for desktop and iPhone
- less dependence on fragile Wi-Fi runtime behavior
- a clearer separation between transfer protocol and physical transport
