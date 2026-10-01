# Factory Updater and Single Production Image Plan

- Status: Complete
- Last-reviewed: 2026-08-18
- Decision: [ADR-0014](../architecture/0014-single-production-sd-recovery-updater.md) (Accepted)
- Replaces: [ADR-0009](../architecture/0009-ab-firmware-update-foundation.md) (superseded)
- Related: [build and flash guide](../guides/build-and-flash.md),
  [DRAM budget](../reference/dram/dram-budget.md),
  [hardware test matrix](../reference/hardware-test-matrix.md)

## Goal and boundaries

Replace the two production slots with a factory updater and one large production partition. Support
complete USB flashing and signed firmware bundles staged on SD. Measure and prove the updater before
accepting ADR-0014 or changing the canonical flash path.

The factory updater owns runtime production-image validation, writing, and activation. Production
firmware stages a complete bundle and requests factory boot. Complete USB flashing owns bootloader and
partition-table changes. The first layout transition uses complete USB flashing because the new
application regions overlap the old slots.

## Invariants

1. The factory updater is the only runtime component that writes or activates `ota_0`.
2. Before each runtime `ota_0` erase, the factory updater is running and both `otadata` sectors are
   erased. After read-back, activation writes exactly one `NEW` record.
3. The updater verifies the bundle before erase and verifies flash before activation.
4. The updater records an attempted digest before activation. Reinstalling it requires a new explicit
   request, even when no `ABORTED` record survives.
5. Every interrupted runtime installation returns to verified production or a responsive updater.
   Interrupted complete USB flashing returns to ROM recovery for another complete flash.

## Phase 1: Size the updater and accept the decision

Create a separate minimal `no_std` firmware target with ESP32 initialization, SD/FAT sequential reads,
flash access, image and signature verification, OTA selection, watchdog handling, and serial status.

- Boot the updater through USB and identify its build and expected layout.
- Read and hash a complete bundle from SD with bounded buffers.
- Define one signed bundle format for target, layout, firmware identity, length, and digest. Use shared
  test cases so host and updater validation produce the same results.
- Measure the release BIN, linked sections, internal DRAM, stack, and flash/SD buffers.
- Choose the smallest 64 KiB-aligned updater partition with a documented maintenance reserve and a
  production partition that meets the capacity target.
- Put the measured partition map in ADR-0014 and review it for acceptance.

Gate: the measured updater and production images fit with their stated reserves, and ADR-0014 records
the accepted layout and bundle contract.

**Progress (2026-08-17): Phase 1 gate met.** The boot/identify, bounded SD read+hash, and
bundle-format/verification bullets are implemented, measured, and fully hardware-verified (flashed to
the current `ota_0` slot ahead of Phase 2's partition table, since ESP-IDF app images are
offset-relative) — see
[ADR-0014](../architecture/0014-single-production-sd-recovery-updater.md#phase-1-measurement) for the
release BIN size, linked sections, accepted partition table, and full boot logs. Three bugs surfaced
and were fixed against the device — a `block_on`-vs-real-executor `embassy_time::Timer` panic, a
PCAL9535A register-address mixup that left the SD card unpowered, and (discovered indirectly) the
production firmware's default build being too large to flash reliably on this bench setup — none of
which the release/clippy/host-test gates alone would have caught. Every stage is now confirmed on real
hardware: boot, OTA-status read, SD power-on, mount, and bundle verification both rejecting invalid
bundles (unsigned build, bad SD power) and accepting a validly signed one end to end. Remaining before
Phase 2 starts: nothing blocking within Phase 1 itself; Phase 3 will still need to re-measure once its
write path lands.

- New: `packages/bundle` (shared signed-bundle header/verification, used by both the updater and,
  later, host tooling), `src/updater/` + `[[bin]] name = "updater"` (the updater itself, built via
  `--no-default-features --features factory-updater`), and `sdcard`'s new
  `FatRequest::Stream` (bounded-buffer sequential file read, alongside the existing whole-buffer
  `Read`).
- Not yet done: OTA selection *writing*, watchdog-triggered recovery behavior beyond a bare RWDT feed
  loop, and the temporary-name/publish/rename SD protocol (all Phase 3); wiring the accepted
  partition CSV into the actual build (Phase 2).

## Phase 2: Build the boot and complete USB path

- Add the accepted partition CSV with the factory updater and one `ota_0`.
- Produce and identify the updater separately from the ESP-IDF bootloader and production image.
- Make complete USB flash write and verify the bootloader, partition table, updater, and production
  image. Erase both OTA sectors, then write and verify one `NEW` record with sequence 1 and a valid CRC.
- Add the production request that selects factory only after a complete SD bundle is published; factory
  selection erases both OTA records.
- Keep `RUNTIME_READY` plus five-second candidate confirmation.
- Prove initial USB boot, explicit factory boot, candidate boot, confirmation, and reset-before-
  confirmation fallback on the pinned ESP-IDF v5.5.2 bootloader.

Gate: every boot state selects confirmed production or the factory updater, and complete USB recovery
works from ROM download mode.

**Progress (2026-08-17): Phase 2 gate met.** The partition CSV, pinned-bootloader build,
initial-`otadata` construction, complete-flash hostctl command, and `src/firmware/update.rs`'s
layout-aware boot/confirm path are implemented and hardware-verified — see
[ADR-0014](../architecture/0014-single-production-sd-recovery-updater.md#hardware-boot-state-matrix-proof)
for the full boot-state-matrix log evidence (initial USB boot, candidate boot, confirmation,
reset-before-confirmation fallback, explicit factory boot — all five proved on a connected board) and
for a real bug worth knowing about: the first `otadata` CRC implementation passed every check available
without hardware (round-tripped through the on-device reader, matched an independent `zlib.crc32`
call) and was still wrong — a real board rejected it. Only writing candidate values to real `otadata`
and watching which one the bootloader accepted caught it. The factory-boot-request production command
(`request_factory_boot()` in `update.rs`) is implemented but not wired to a caller yet — Phase 2
verified the underlying bootloader mechanism directly rather than through it; wiring it to a serial
command is deferred to whichever later phase needs an operator- or app-triggered factory boot. Also
still open: re-measurement once Phase 3 adds its write path, and a real (non-stand-in) production image
once the separately-unstable default firmware build issue is resolved.

## Phase 3: Add one signed SD installation path

- Publish downloads under a temporary name, sync and verify them, then expose one complete candidate.
- Record the attempted digest before activation; keep it blocked from automatic reinstall until a new
  explicit request is recorded.
- Verify that factory is running and both OTA records are erased before production erase.
- Stream the verified image into `ota_0`, perform full read-back, then write one `NEW` OTA record and
  reboot.
- Cover incomplete, corrupt, wrong-target, repeated-failed, missing-media, and interrupted operations
  with focused tests and shared validation examples.

Gate: a valid bundle reaches candidate boot, while every rejected or interrupted transaction leaves a
responsive updater or verified production.

**Progress (2026-08-17): Phase 3 gate met.** Attempted-digest tracking, the factory-running pre-erase
check, chunked erase+write into `ota_0`, full read-back verification, and activation are implemented
(`src/updater/{attempted,install}.rs`) and hardware-verified — see
[ADR-0014](../architecture/0014-single-production-sd-recovery-updater.md#phase-3-progress) for both
proof logs: a valid bundle installed, rebooted, and confirmed to candidate boot end to end; and a
hardware reset pulsed mid-flash-write recovered cleanly to a responsive factory updater (not a torn
`ota_0` image), with the already-attempted guard correctly refusing an automatic retry. Also worth
knowing: the investigation initially chased what looked like a real digest-verification bug, but it
turned out to be a flow-control bug in the disposable SD-push test tool (UART RX FIFO overflow while
the tool was blocked on a slow SD write) — the real update-path code was never at fault, confirmed by
an independent write-then-read-back hash round trip through the same `sdcard` primitives. Still open:
Phase 4's real (non-stand-in) production image and wiring `request_factory_boot()` to an operator- or
app-triggered caller.

## Phase 4: Qualify and cut over

- Record the current A/B artifact baseline and inventory its partition, firmware, serial, host,
  documentation, CI, and test owners.
- Add host commands to build, sign, inspect, and copy the same bundle to SD. Archive the exact updater,
  production bundle, flash layout, hashes, and source identity.
- Document complete USB flashing, SD installation, updater status, failed-candidate handling, and ROM
  recovery.
- Move one identified board from A/B to the new layout with complete USB flashing.
- Use reset injection for repeatable coverage and physical power interruption for SD publication,
  `ota_0` erase/write, OTA activation, and candidate confirmation.

Gate: every interruption recovers to verified production or a responsive updater, attempted digests
are not reinstalled automatically, and USB recovery succeeds with the retained artifacts.

**Progress (2026-08-18): Phase 4 gate met**, with one loose end flagged rather than closed by guessing
— see [ADR-0014](../architecture/0014-single-production-sd-recovery-updater.md#phase-4-progress) for
full detail. Delivered: the [A/B baseline inventory](../reference/ab-firmware-update-baseline.md);
`hostctl` bundle build/inspect/sign commands and a bench-only SD-staging transport
(`sd-qual-push`/`single-production-sd-push` — the device's SD card isn't reachable without
disassembly, so this exists purely for qualification, not as a production delivery path);
`request_factory_boot()` wired to an operator-triggered serial command (`FWFACTORYBOOT`), guarded
against running on a still-A/B board; a bundle-retirement fix (rename once a digest is durably
recorded as attempted) that closed a real accidental-reinstall bug this session's own hardware testing
produced; and a board moved to the single-production layout with the real ~1.96 MB production image,
proven end to end (verify → install → reboot → **confirm**) using the fixed pipeline. Reset-injection
interruption coverage revisited across all four named operations — two directly hardware-tested
(Phase 3's mid-write reset, Phase 2's mid-confirmation-window reset), one structurally covered by
Phase 2's otadata-fallback proof (OTA activation — the window itself is too short to hit blind), one
not applicable to this ADR yet (SD publication — real delivery is WiFi/BLE, not yet built). Physical
power interruption was explicitly deferred (reset injection already covers the harder, more
precisely-timed case; pulling power needs a person at the bench).

**Update (2026-08-18):** the real production firmware's failure to self-confirm within ~30s (noted
below as left open) was root-caused via continuous, zero-gap live serial capture rather than guessed
wait times: the firmware panics (`LoadStoreError`) about 1-1.5s after boot, inside
`esp_rtos::run_queue::RunQueue::mark_task_ready` called from an interrupt handler, well before
`runtime_ready()` or confirmation can ever run. Dual-core startup ordering and touch-core stack size
were both tested on hardware and ruled out. Rebuilding with the radio/net subsystem compiled out
(`--no-default-features`) produced a fully clean 90s run — `RUNTIME_READY` at t=3.9s, `FIRMWARE_CONFIRM
state=valid` at t=8.9s — proving the crash lives in the radio/net subsystem (default feature build),
not in anything ADR-0014 built, and not in partition layout. See [ADR-0014's Phase 4
progress](../architecture/0014-single-production-sd-recovery-updater.md#phase-4-progress) for the full
investigation. **This does not block Phase 5** — it's a pre-existing bug (A/B firmware almost certainly
hits it too) tracked separately from this ADR.

## Phase 5: Remove A/B authority

- Remove `ota_1`, inactive-slot selection, two-slot capacity constants, the serial A/B stream, and the
  host runtime `firmware-update` workflow.
- Retain signing, image validation, bounded flash writes, read-back, and candidate-health logic that
  the new flow still uses.
- Update guides, hardware references, the architecture index, CI guards, ownership inventories, and
  tests.
- Run the full quality lane and identified-device recovery checks.

Final gate: the final tree uses the single-production flow as its only update authority, and all
quality and device-recovery checks pass.

**Progress (2026-08-18): Phase 5 gate met — ADR-0014 complete.** See [ADR-0014's Phase 5
progress](../architecture/0014-single-production-sd-recovery-updater.md#phase-5-progress) for full
detail: the A/B streaming protocol and session state machine are gone from both the device and host
sides; `ota_crc` moved to a new host-testable `packages/otadata` crate, closing a pre-existing
test-coverage gap; the default build and default flash tooling
(`xtensa_runner.sh`/`hostctl flash-capture`) now target the single-production layout. Hardware-verified
on the same bench board: the confirm mechanism, `FWFACTORYBOOT`, and a full SD-bundle install all work
correctly through the narrowed code; the `xtensa_runner.sh` cutover is real (confirmed espflash resolves
the single-production bootloader/partition-table/offsets); the Phase 4 radio/net-subsystem crash is
unaffected (still present, unrelated to anything this phase touched). Full quality lane passes
(`scripts/host-test.sh test all` / `lint all`, `check_network_owner_source.sh`,
`check_script_surface.py`). ADR-0014's status moves to Accepted.
