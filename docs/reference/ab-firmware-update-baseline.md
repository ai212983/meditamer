# A/B firmware-update baseline (pre-cutover inventory)

Recorded 2026-08-17, ahead of ADR-0014 Phase 4's cutover to the single-production layout. This is a
factual inventory of everything the current A/B system (ADR-0009) owns, so Phase 4/5 changes have a
checklist to work against and nothing gets silently orphaned. Not a decision document — see
[ADR-0009](../architecture/0009-ab-firmware-update-foundation.md) for why A/B was adopted and
[ADR-0014](../architecture/0014-single-production-sd-recovery-updater.md) for why it's being replaced.

## Partition

- `config/partitions-ab.csv` — the layout in production today: `nvs` (`0x9000`/`0x6000`), `otadata`
  (`0xf000`/`0x2000`), `phy_init` (`0x11000`/`0x1000`), `app_state` (`0x12000`/`0x2000`), `ota_0`
  (`0x20000`/`0x1f0000`), `ota_1` (`0x210000`/`0x1f0000`) — no `factory` partition, two equal
  `0x1f0000` (1,966,080-byte) application slots.
- Replaced by `config/partitions-single-production.csv` (Phase 2, not yet wired into the default
  build): `factory` (`0x20000`/`0x60000`) + `ota_0` (`0x80000`/`0x380000`), same `nvs`/`otadata`/
  `phy_init`/`app_state` region.

## Firmware (device side)

- `src/firmware/update.rs` (925 lines) — the A/B OTA core: `Slot::{Ota0,Ota1}` (Phase 2 added
  `Factory`, currently unreachable from this path), `validate_layout()`, chunked `write_chunk()`,
  `confirm_pending_image()`, `status()`, session/transport-quiet tracking. Phase 2 made this
  layout-aware (auto-detects A/B vs single-production by whether `ota_1` exists) rather than
  rewriting it, so it already serves both layouts — this file is not being removed, only narrowed in
  Phase 5.
- `src/firmware/serial/firmware_stream.rs` (248 lines) — the A/B binary streaming protocol over UART
  (`MF` magic, chunked frames with CRC, `STREAM_BAUD=460800`) that feeds `update.rs::write_chunk`.
- `src/firmware/serial/command_dispatch.rs`, `command_family.rs`, `task_state.rs` — serial command
  wiring for the update session: `SerialCommand::FirmwareAbort`, hardware-lease
  begin/end/active-tracking (`*_firmware_update_hardware_lease*`), transport-quiet coordination with
  Wi-Fi/BLE so a firmware stream and a radio session never run concurrently.
- `src/firmware/net/runtime.rs` — checks `firmware_update::transport_quiet()` /
  `SessionPhase::Idle` before allowing certain network activity to proceed (shared hardware-lease
  concern, not A/B-specific logic itself).

## Host (hostctl)

- `tools/hostctl/src/workflows/firmware_update.rs` (689 lines) — the A/B host workflow: builds and
  signs an image, drives the serial streaming protocol via `SerialConsole`, and verifies
  confirmation.
- `tools/hostctl/src/workflows/mod.rs`, `src/main.rs` — registration and CLI wiring for the above.

## CI

- `scripts/ci/check_network_owner_source.sh` — guards that the serial dispatch/family files still
  contain the exact hardware-lease and transport-quiet call sites listed above (line-content greps,
  not line-number greps — tolerant of reformatting, not of the calls disappearing). Needs updating in
  lockstep with any Phase 5 removal, not before.

## Documentation

- [ADR-0009](../architecture/0009-ab-firmware-update-foundation.md) — the decision this baseline
  inventories; stays as historical record after cutover (superseded, not deleted).
- [`docs/guides/build-and-flash.md`](../guides/build-and-flash.md) — operator-facing build/flash
  instructions; currently A/B-only, needs the single-production flows added (Phase 4 documentation
  task).
- [`docs/reference/dram/dram-budget.md`](dram/dram-budget.md) — cites the A/B slot size as part of the
  image-size budget.
- [`docs/reference/hardware/inkplate/sensors.md`](hardware/inkplate/sensors.md) — tangential hardware
  reference cross-linked from ADR-0009.
- `docs/architecture/README.md` — architecture index; already lists both ADR-0009 and ADR-0014.
  ADR-0009's `Status` cell will need a `Superseded by [0014]` note once Phase 5 actually removes the
  A/B path (matching the convention row 2 uses for ADR-0002) — not yet, since A/B keeps working
  through Phase 4.

## Tests

- `update.rs` has an inline `#[cfg(all(test, not(target_os = "none")))] mod tests` (e.g.
  `ota_crc_matches_esp_idf_examples`) — noted during Phase 2 as **not currently exercised by any
  established host-test path**, since the crate has `[lib] test = false`. Pre-existing gap, not
  introduced by ADR-0014 work; worth fixing in Phase 5 rather than carrying it forward silently.
- No dedicated `host-suites.tsv` entry for `firmware_update`/A/B streaming — coverage today is
  hostctl's own workflow (exercised live against hardware) plus whatever `update.rs`'s untested inline
  module would provide if it ran.

## What Phase 4/5 changes and what it doesn't

Phase 4 adds the single-production build/sign/SD-copy tooling and cuts one board over; it does **not**
remove any of the above — the A/B path keeps working (for boards not yet migrated) until Phase 5, which
removes `ota_1`, inactive-slot selection, the two-slot capacity constants, the serial A/B streaming
protocol, and the host `firmware-update` workflow, while retaining the signing/validation/bounded-write/
read-back/candidate-health logic the new flow reuses.
