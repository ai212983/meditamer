# 2026-03-20 Asset Upload Transport Plan

- Status: Needs-triage
- Last-reviewed: 2026-08-10
- Note: Premise was that Wi-Fi upload is too fragile to keep. Wi-Fi is now fixed, so the trade-off this plan argued from has changed. Re-decide before acting on it.

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

## Decision

Do not make Wi-Fi the primary long-term asset upload transport.

Do not make Bluetooth Classic the primary transport either.

Adopt a transport-agnostic upload protocol with BLE as the primary external transport.

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

## Migration Direction

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
