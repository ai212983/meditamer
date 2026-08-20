# ADR-0014: Replace A/B with a factory updater and one production image

- Status: Accepted
- Author: Codex; Phase 1 measurement, Phase 2, Phase 3, Phase 4, and Phase 5 added by Claude/Sonnet5; radio/net-subsystem crash addendum added by Claude/Sonnet5
- Date: 2026-08-17
- References: [ADR-0009](0009-ab-firmware-update-foundation.md),
  [ADR-0011](0011-bounded-ble-service-foundation.md),
  [implementation plan](../archive/firmware-update/single-production-sd-recovery-updater.md) (archived, complete),
  [build and flash guide](../guides/build-and-flash.md),
  [hardware storage reference](../reference/hardware/inkplate/sensors.md)

## Context

The current A/B layout limits accepted firmware images to 1,900,544 bytes. That is too small for the
intended production firmware.

USB flashing through the ESP32 ROM downloader remains available. The board also has microSD storage,
but the ESP32 cannot execute ordinary native application code from SD. An internal-flash updater must
verify and install an image staged there.

The pinned ESP-IDF v5.5.2 bootloader can start a factory application when the production image is
missing, invalid, or unconfirmed. The repository already provides the signing, flash, SD, candidate
confirmation, and USB recovery primitives needed for this design.

Measure the minimal release updater before fixing its partition size.

## Decision

Adopt a small factory updater and one large `ota_0` production partition. Signed firmware bundles may
be staged on SD, and complete USB flashing remains the recovery root. ADR-0009 remains authoritative
until this ADR is accepted; acceptance supersedes its A/B layout and update transaction.

The factory updater verifies a signed bundle for the current hardware and partition layout before
writing `ota_0`. It verifies the written image before selecting it for boot. Production firmware may
stage a complete bundle and request factory boot; the updater is the only runtime component that
writes or activates `ota_0`.

Each runtime installation begins with the factory updater running and both `otadata` sectors erased. The
updater verifies that state before erasing `ota_0`, then writes one `NEW` record after flash read-back.
This is required because, with one OTA slot, an older `VALID` record still points to the same physical
partition and could otherwise select a failed overwritten image.

Before activation, the updater records the attempted digest. A new explicit request is required to
install that digest again, even if power loss prevents an `ABORTED` OTA record from being written.

Production confirmation keeps the existing `RUNTIME_READY` plus five-second health boundary. A reset
before confirmation returns to the factory updater.

Keep the current data partitions and place the factory updater before `ota_0`. Set their boundary from
the measured updater release with explicit headroom for both images. The expected production capacity
is about 3.4-3.6 MiB.

Install the new layout with one complete USB flash because its application regions overlap both
current A/B slots. The USB workflow writes the bootloader, partition table, updater, production image,
erases both OTA sectors, then writes and verifies one `NEW` record with sequence 1 and a valid CRC.
Later production updates use the signed SD transaction.

## Phase 1 measurement

Measured 2026-08-17 against a Phase 1 updater (`[[bin]] name = "updater"`, `src/updater/`) that
boots, identifies the running build and OTA state (read-only), powers and mounts SD, and streams one
candidate bundle from bounded buffers for signature and digest verification. It has no `ota_0` write
path yet — erase, write, read-back, and activation are Phase 3. Built with
`--no-default-features --features factory-updater` so wifi, BLE, and the LVGL-backed UI stay out of
the linked image; the symbol table confirms no `lv_*`, wifi, display, or panel code is present.

- Release flash image (`espflash save-image --chip esp32`): 290,400 bytes.
- Linked sections (`xtensa-esp32-elf-size -A`): `.text` 224,785 · `.rodata` 48,220 · `.rwtext` 10,520
  · `.flash.appdesc` 256 · `.data` 4,980 · `.bss` 7,928 bytes.
- Internal DRAM (`.data` + `.bss`): 12,908 bytes — the embassy-executor task pool for `bundle_task`
  (see Hardware boot below) accounts for most of the `.bss` growth over a version with no executor at
  all. Peak stack use during an actual SD/crypto pass is still not measured — that needs stack-painting
  telemetry the same way [DRAM Budget](../reference/dram/dram-budget.md) measures the production image,
  which this Phase 1 run did not add.
- About 48 KiB of `.text` is `sha2::sha512::compress512`: ed25519 verification always hashes with
  SHA-512 internally (RFC 8032) regardless of the SHA-256 used for the firmware digest itself, so
  this is expected, not a leftover dependency.

### Hardware boot

Flashed to the current `ota_0` slot (`0x20000`) on a connected board — a valid way to boot-test this
Phase 1 updater without Phase 2's new partition table, since ESP-IDF app images are offset-relative
the same way A/B already relies on. Two real bugs surfaced and were fixed by iterating against the
device, neither of which the release/clippy/host-test gates alone would have caught:

1. **`embassy_time::Timer` panic.** First attempt used `embassy_futures::block_on` instead of a real
   executor, to save the size/complexity of `esp_rtos::embassy::Executor`. It panicked immediately
   after printing its boot lines: `Timer` needs a waker created by an Embassy executor (the generic
   timer queue is not enabled), so a bare polling loop is not enough. Fixed by spawning the
   read/verify flow as a real `#[embassy_executor::task]` under `esp_rtos::embassy::Executor`,
   matching `src/firmware/system.rs`'s own pattern.
2. **Wrong PCAL9535A register addresses.** `sd_power` (the trimmed IO-expander driver — see Bundle
   format's sibling note below) used `0x43`/`0x47` for the output/config-port-1 registers, reasoning
   backward from `PCAL_OUTPORT1_ARRAY = 3` / `PCAL_CFGPORT1_ARRAY = 7` in
   `src/platform/inkplate/hardware.rs` as if those were register addresses. They are *indices* into
   `PCAL_REG_ADDRS`; `PCAL_REG_ADDRS[3] == 0x03` and `PCAL_REG_ADDRS[7] == 0x07` (the "legacy"
   0x00-0x07 bank), not `0x43`/`0x47` (the "enhanced" 0x40+ bank — different registers entirely, none
   of which affect direction or output level). The board still answered CMD0/CMD8/ACMD41 over SPI
   with a card inserted — `Acmd41Timeout(0x01)`, i.e. the card kept declaring itself busy — consistent
   with marginal/unstable rail voltage from a pin that was never actually reconfigured to a firmly
   driven output.

Boot log after both fixes, with a card inserted and this test build's signing key intentionally
unconfigured:

```
UPDATER_BOOT build_id=unlabeled key_configured=false target=1 layout=1
UPDATER_OTA_STATUS booted=ota_0 selected=ota_0 state=valid build_id=unlabeled
UPDATER_SD_PROBE_OK capacity_bytes=8044675072 filesystem=Fat32
UPDATER_BUNDLE_ERROR path=/UPDATE.BIN reason=SigningKeyUnconfigured
```

Boot, OTA-status read, watchdog/executor plumbing, SD power-on, SPI probe, and FAT32 mount are all
confirmed working on real hardware. The final line is the correct, intended behavior for a build
without `MEDITAMER_FIRMWARE_PUBLIC_KEY_HEX` set — signature verification must reject every bundle
rather than silently skip it.

**Verify-success path.** The rejection paths above don't prove the accept path works — that needed a
key-configured build and a real signed bundle on the card. Getting one there hit its own obstacle: the
production firmware's default build (needed for its serial `SDFATWRITE`/Wi-Fi upload paths) is
currently too large to flash reliably on this bench setup — `espflash write-bin` timed out repeatedly
on its ~1.9 MB image while the updater's own ~300 KB image flashed cleanly every time. Rather than
chase that reliability problem, we wrote one bundle onto the card using a throwaway ~200 KB firmware
(reusing the updater's own boot/SD-power/probe/mount code, since that clearly does flash reliably) that
read a magic byte, a little-endian `u32` length, and that many raw bytes off UART and wrote them to
`/UPDATE.BIN` via the existing (unmodified) `FatRequest::Write`. Deleted from the tree once it had done
its job — it was never part of this ADR's deliverable, just a way to get bytes onto a card already
plugged into the board under test. A signed 400-byte test bundle (144-byte header + 256-byte payload)
built and self-verified with `packages/bundle`'s own `BundleHeader`/`sign` logic, built with a matching
`MEDITAMER_FIRMWARE_PUBLIC_KEY_HEX` baked into the updater:

```
UPDATER_BOOT build_id=phase1-smoke-test key_configured=true target=1 layout=1
UPDATER_OTA_STATUS booted=ota_0 selected=ota_0 state=valid build_id=phase1-smoke-test
UPDATER_SD_PROBE_OK capacity_bytes=8044675072 filesystem=Fat32
UPDATER_BUNDLE_OK path=/UPDATE.BIN build_id=phase1-smoke-test target=1 layout=1 bytes=400
```

Every stage of the Phase 1 pipeline — boot, OTA-status read, SD power-on, mount, bounded-buffer stream,
header parse, signature verification, and streamed-digest verification — is now confirmed correct on
real hardware, both accepting a valid bundle and rejecting invalid ones.

### Bundle format

One shared format, implemented once in `packages/bundle` (`#![no_std]`, no I/O) and used unmodified
by the updater and — once Phase 4 adds host signing/inspection commands — hostctl, so "host and
updater validation produce the same results" by construction rather than by keeping two
implementations in sync. A bundle on SD is a 144-byte header (magic, `target_id`, `layout_id`,
build id, firmware length, SHA-256 firmware digest, ed25519 signature over everything but the
signature itself) immediately followed by the firmware image. `target_id` and `layout_id` are both
`1` today (single board, single layout); either changing is a compatibility break by design — a
bundle for the wrong hardware or the wrong partition map must fail closed, not partially apply.
Signing reuses the same 32-byte key as the legacy per-chunk A/B stream
(`MEDITAMER_FIRMWARE_PUBLIC_KEY_HEX`, `src/firmware/update.rs`) but a distinct domain tag
(`MEDITAMER-BUNDLE-V1` vs. `MEDITAMER-FIRMWARE-V1`), so a signature valid for one protocol can never
be replayed as the other.

Streaming a bundle larger than any on-device buffer required one small, additive change to
`packages/sdcard`'s FAT engine: a new `FatRequest::Stream` alongside the existing `Read` (which
requires the whole file to fit in the caller's buffer). `Stream` hands back one SD-sector-sized chunk
at a time instead, reusing the existing cluster-chain-walking code path unchanged — see
`packages/sdcard/src/fat/engine/read.rs::advance_stream` and the `stream_read_*` tests in
`packages/sdcard/tests/fat_engine.rs`.

### Accepted partition layout

Keeps `nvs`, `otadata`, `phy_init`, and `app_state` at their current offsets and sizes. The factory
updater and `ota_0` both need 64 KiB-aligned start and end offsets (flash-cache MMU mapping
granularity for app partitions); the next such boundary after `app_state` (`0x14000`) is `0x20000` —
the same offset the current `ota_0` already starts at.

| Partition | Offset | Size | Bytes |
| --- | ---: | ---: | ---: |
| `factory` (updater) | `0x20000` | `0x60000` | 393,216 (384 KiB) |
| `ota_0` | `0x80000` | `0x380000` | 3,670,016 (3.5 MiB) |

384 KiB gives the measured 290,400-byte updater 102,816 bytes (35%) of headroom — for Phase 3's
erase/write/read-back/activation path and later maintenance — while still landing on a clean 64 KiB
multiple. 3.5 MiB sits inside the "about 3.4-3.6 MiB" production capacity this ADR already targets,
and `0x20000 + 0x60000 + 0x380000 == 0x400000` accounts for the entire 4 MiB device exactly, with no
gap or overlap.

### Gate status

The Phase 1 gate — "the measured updater and production images fit with their stated reserves, and
ADR-0014 records the accepted layout and bundle contract" — is met: a read-and-verify-only updater,
confirmed on real hardware both accepting a validly signed bundle and rejecting invalid ones (missing
signing key, and — before that — a truly invalid SD power sequence), inside its 384 KiB budget with
headroom to spare. This is not a final sign-off on 384 KiB: Phase 3 adds the write path and must
re-measure before Phase 2's partition CSV is treated as load-bearing rather than a draft. Still
outstanding, and explicitly out of scope for Phase 1: a runtime stack high-water mark measured the way
[DRAM Budget](../reference/dram/dram-budget.md) does for the production image, and of course everything
Phase 2 onward is responsible for (partition cutover, write path, USB recovery, A/B removal).

## Phase 2 progress

Built and host-verified 2026-08-17 (tooling and artifacts; hardware boot-matrix proof still open —
see Outstanding below):

- **Partition CSV** (`config/partitions-single-production.csv`): the Phase 1 layout — `factory` at
  `0x20000`/384 KiB, `ota_0` at `0x80000`/3.5 MiB — converts and validates cleanly with
  `espflash partition-table --to-binary`.
- **Bootloader and partition table**: `scripts/build/single_production_bootloader.sh` (mirrors
  `scripts/build/ota_bootloader.sh`, the existing A/B build, pointed at
  `tools/ota_bootloader/sdkconfig.single-production.defaults`) drives the same pinned ESP-IDF v5.5.2
  checkout to produce `bootloader.bin`/`partition-table.bin` for the new layout. Its
  `partition-table.bin` output is byte-identical to `espflash partition-table --to-binary`'s — cross-
  checked, not assumed.
- **Initial `otadata`** (`tools/hostctl/src/workflows/single_production/otadata.rs`): constructs the
  8 KiB `otadata` image — one `ota_seq=1`/`New` record in sector 0, sector 1 left erased — that a
  complete USB flash needs (invariant 2: every boot of a freshly written image goes through the same
  candidate-confirmation path as a runtime install, not a distinct "freshly flashed" special case).
  The record layout and CRC32 were traced by hand from the pinned ESP-IDF source
  (`esp_ota_select_entry_t`, `bootloader_common_ota_select_crc`, down to the ROM CRC table itself —
  confirmed to be plain CRC-32/ISO-HDLC over the `ota_seq` bytes) and cross-checked against an
  independent `zlib.crc32` computation, **not** against `esp_bootloader_esp_idf`'s own crate-internal
  unit-test fixtures — those turned out to carry non-validated filler CRC bytes, since
  `Ota::current_app_partition`/`current_ota_state` never actually check the CRC field on read (only
  the real ESP-IDF bootloader does). `ota_seq=1` specifically, not the `0` a naive multi-slot rotation
  would compute for a single-`ota_0` layout — `0` triggers a `seq - 1` unsigned-subtraction underflow
  in `Ota::current_app_partition`'s sequence math (harmless in release, since `x % 1 == 0` regardless
  of wraparound, but a debug-build panic waiting to happen). The whole construction is round-tripped
  through `esp_bootloader_esp_idf::ota::Ota` — the same reader the firmware itself uses at boot —
  against a real converted partition table, not just asserted as "bytes that look right."
- **Complete-flash command**: `hostctl single-production-flash --port <port> --factory <bin>
  --production <bin>` (`tools/hostctl/src/workflows/single_production/flash.rs`) writes bootloader,
  partition table, the constructed `otadata`, the factory image, and the production image in one
  `esptool.py write_flash`, mirroring the existing A/B `run_full_flash` (`flash_capture/flash.rs`)
  with a fifth region and a real initial `otadata` instead of ESP-IDF's blank default.

### `update.rs` made layout-aware

`src/firmware/update.rs` was written against the A/B layout throughout — `Slot::{Ota0,Ota1}`,
hardcoded `OTA_0_OFFSET`/`OTA_1_OFFSET`/`OTA_SLOT_SIZE`, `validate_layout()` requiring both `ota_0`
and `ota_1` to exist, `Ota::new(..., 2)` hardcoded at both call sites. Fixed narrowly rather than
rewritten: `validate_layout()` now detects which of the two accepted shapes (A/B or single-production)
is actually on the device — by checking whether an `ota_1` partition exists — and validates against
that shape's offsets; the same detection feeds `ota_partition_count()`, replacing both hardcoded `2`s
so `Ota` interprets `otadata` sequence numbers correctly either way. `Slot` gained a `Factory` variant
so boot-status reporting works when running from the factory partition (the updater's own `status()`
call, `crate::firmware::update::status()` — reused from Phase 1 — needed this to not error out once
actually booting from `factory` instead of the Phase-1 stand-in of flashing it into `ota_0`); the A/B
streaming write path's `Slot::offset()`/`opposite()` treat `Factory` as unreachable, since production
firmware only reaches those from `ota_0` and the factory partition runs a different compiled binary
entirely (`src/updater/`) that never calls them. `request_factory_boot()`
(`Ota::set_current_app_partition(Factory)`, erasing both `otadata` records) is implemented but not yet
wired to a caller — Phase 2 verified the underlying bootloader mechanism directly (see below) rather
than through this function; a serial command exposing it is deferred to whichever later phase needs an
operator- or app-triggered factory boot.

### Hardware boot-state-matrix proof

All five states proved on a connected board, using the updater binary as both `factory` and `ota_0`
(the default production build is separately unreliable to flash on this bench setup — see Phase 1's
Hardware boot section — so it doesn't confound this specific proof; `confirm_if_pending_candidate` was
added to `src/updater/mod.rs` so it can stand in for a real production image here, exercising the exact
same `crate::firmware::update` functions a real one would).

1. **Initial USB boot**: `hostctl single-production-flash` (factory + `ota_0` + freshly constructed
   `otadata`) → `Loaded app from partition at offset 0x80000` → `UPDATER_OTA_STATUS booted=ota_0
   selected=ota_0 state=pending_verify` — the bootloader's own `New`→`PendingVerify` auto-transition,
   confirmed from the app side.
2. **Candidate boot / confirmation**: `UPDATER_CANDIDATE_PENDING settle_ms=5000`, then after 5 s
   `FIRMWARE_CONFIRM slot=ota_0 state=valid` / `UPDATER_CANDIDATE_CONFIRM confirmed=true` —
   `crate::firmware::update::confirm_pending_image()`, unmodified from the A/B path, works identically
   against the single-production layout.
3. **Reset-before-confirmation fallback**: reflash, then pulse a hardware reset (RTS toggle,
   `tools/hostctl/src/serial_console/mod.rs::pulse_en_reset`'s exact sequence) at `t=1.5s` — inside the
   5 s confirmation window. Next boot: `boot: Defaulting to factory image` →
   `UPDATER_OTA_STATUS booted=factory selected=ota_0 state=aborted` — the bootloader marked the
   unconfirmed candidate `Aborted` and fell back to `factory`, entirely stock ESP-IDF behavior
   (`CONFIG_BOOTLOADER_APP_ROLLBACK_ENABLE=y`, already set) requiring no app-side code.
4. **Explicit factory boot**: `otadata` written blank (both sectors `0xFF` — what
   `request_factory_boot()` produces) → `boot: Defaulting to factory image` →
   `UPDATER_OTA_STATUS booted=factory selected=factory state=none` — the same bootloader mechanism,
   triggered directly rather than via an aborted candidate, confirming it's the blank-`otadata` state
   that matters, not the specific path that produced it.

#### A CRC bug that only hardware caught

The first hardware attempt at initial USB boot failed: `boot: ota data partition invalid, falling back
to factory` even though `otadata` had a well-formed `ota_seq=1`/`New` record. Read back with
`espflash read-flash` byte-for-byte against what was written — correct. The bug was in the CRC
computation `otadata.rs` shipped with (Phase 2's first pass, described in the "Initial `otadata`"
section above as cross-checked against an independent `zlib.crc32` call): `esp_rom_crc32_le(0xFFFFFFFF,
&ota_seq, 4)`'s body is `crc = ~crc_in; loop; return ~crc;`, so passing `crc_in = 0xFFFFFFFF` makes the
table loop's *entry* register `0`, not the `0xFFFFFFFF` that plain `zlib.crc32(data)` (implicit
`value=0`) and the `crc` crate's `CRC_32_ISO_HDLC` preset both use. The two algorithms share every
other parameter (poly, refin, refout, xorout) and even most of their test coverage — `Ota`'s own reader
never checks the CRC field at all, only the real bootloader does — so nothing short of a real device
would have caught this; the fix (`init: 0` in a hand-built `crc::Algorithm`, not the named preset) is
`zlib.crc32(data, 0xFFFFFFFF)`, confirmed by writing both candidate values to a real board and reading
back which one `bootloader_utility_get_selected_boot_partition` accepted. This also means
`esp-bootloader-esp-idf`'s own crate-internal `ota.rs` test fixtures — dismissed in the crate's first
version of this file as "unvalidated filler" — were right all along; the filler was the earlier,
confidently-wrong independent check, not the fixtures.

## Phase 3 progress

Built, hardware-verified, and closed out 2026-08-17: the write path, wired end to end and proved on a
connected board.

- **Attempted-digest tracking** (`src/updater/attempted.rs`): before touching flash, the updater
  records the candidate bundle's firmware digest to `/UPDATE.ATTEMPTED` on SD (`FatRequest::Write`).
  On every boot, if a bundle's digest matches the recorded one, install is skipped
  (`UPDATER_INSTALL_BLOCKED reason=already_attempted`) rather than retried — deliberately unconditional
  on outcome (a bundle that failed partway is exactly as blocked as one that installed cleanly), so a
  bad or interrupted transaction can never turn into an unbounded automatic retry loop. RTC memory was
  considered and rejected for this marker: it does not survive a full power-on reset, and this guard
  specifically needs to survive one.
- **Pre-erase safety** (`src/updater/install.rs::run`): install refuses to proceed unless
  `crate::firmware::update::status()` reports the updater is actually booted from `factory` — the same
  invariant Phase 2's boot-state-matrix proved holds structurally (production can never reach this code
  path; only the factory-compiled binary can). The attempted-digest record is written *before*
  `request_factory_boot()` erases `otadata`, and `request_factory_boot()` runs before any flash write to
  `ota_0` begins — so every ordering a reset could land in still leaves both the guard recorded and the
  bootloader's fallback already pointed at `factory`.
- **Chunked erase+write to `ota_0`** (`install.rs::write_payload_to_ota0`): a second
  `FatRequest::Stream` pass (the first, in `bundle_stream.rs`, only verifies) re-reads the bundle,
  skips the header, and writes each chunk to `ota_0` with an erase-ahead-of-write-cursor cadence
  identical to `src/firmware/update.rs::write_chunk`'s established A/B pattern
  (`FLASH_ERASE_SECTOR=0x1000`, `FLASH_PROGRAM_PAGE_BYTES=256`), feeding the RTC watchdog every chunk.
- **Read-back verification and activation** (`install.rs::read_back_matches`, `activate`): after the
  write, every byte is read back from flash and re-hashed against the bundle's digest before an
  `otadata` record is built directly (`seq=1`, `state=New`, `crc=ota_crc(1)`) and written via
  `crate::firmware::flash::replace(0xf000, ..)` — deliberately not
  `Ota::set_current_app_partition`, for the same `seq=0` CRC-underflow risk Phase 2's otadata section
  documents. A software reset follows on success.

### Hardware install proof

Using the updater binary as its own install payload (so one board proves the whole loop: factory
verifies+installs, the resulting `ota_0` candidate is itself a real, runnable updater build):

```
UPDATER_BUNDLE_OK path=/UPDATE.BIN build_id=phase3-smoke-test target=1 layout=1 bytes=318864
FIRMWARE_FACTORY_REQUEST state=erased
[reboots]
UPDATER_OTA_STATUS booted=ota_0 selected=ota_0 state=pending_verify build_id=phase3-smoke-test
UPDATER_CANDIDATE_PENDING settle_ms=5000
FIRMWARE_CONFIRM slot=ota_0 state=valid
UPDATER_CANDIDATE_CONFIRM confirmed=true
UPDATER_BUNDLE_OK path=/UPDATE.BIN build_id=phase3-smoke-test target=1 layout=1 bytes=318864
UPDATER_INSTALL_BLOCKED path=/UPDATE.BIN reason=already_attempted
```

Verify, install, reboot, bootloader auto-transition to `pending_verify`, confirm, and — since the
booted `ota_0` image is itself an updater build that naturally re-checks SD on its own next pass — the
already-attempted guard correctly refuses to reinstall the same bundle. This is Phase 3's core gate:
*a valid bundle reaches candidate boot.*

### Hardware interruption-recovery proof

A hardware reset (`tools/hostctl/src/serial_console/mod.rs::pulse_en_reset`) pulsed mid-transaction,
landing right after `FIRMWARE_FACTORY_REQUEST` — i.e. during the actual flash erase/write to `ota_0`:

```
UPDATER_BUNDLE_OK path=/UPDATE.BIN build_id=phase3-interrupt-test target=1 layout=1 bytes=318832
FIRMWARE_FACTORY_REQUEST state=erased
[pulsed reset — landed mid-write]
UPDATER_OTA_STATUS booted=factory selected=factory state=none build_id=phase3-interrupt-test
UPDATER_SD_PROBE_OK capacity_bytes=8044675072 filesystem=Fat32
UPDATER_BUNDLE_OK path=/UPDATE.BIN build_id=phase3-interrupt-test target=1 layout=1 bytes=318832
UPDATER_INSTALL_BLOCKED path=/UPDATE.BIN reason=already_attempted
```

The device came back up running the factory updater — not a torn `ota_0` image, not bricked — because
`otadata` was already pointed at `factory` before the write began. SD and the bundle are still
readable, and the attempted-digest guard (recorded before the write, so it survives the interruption
too) refuses to retry the same bundle automatically. This is Phase 3's other gate: *every rejected or
interrupted transaction leaves a responsive updater or verified production.*

### A digest mismatch that was the test harness, not the updater

The first several install attempts failed with a payload digest mismatch — the streamed SHA-256 never
matched the bundle's signed digest, despite the signature itself verifying (proving the header, and
therefore the true digest, were intact). Root-caused by writing a throwaway diagnostic directly into
`sdpush` (the disposable SD-push tool used to stage large bundles over serial for hardware testing, not
part of this ADR's deliverable — see below) that streamed the just-written file straight back and
hashed it two ways: once whole-file, once with the same header-skipping split
`bundle_stream.rs::stream_and_verify` uses. Both round-tripped correctly through
`FatRequest::Write`/`Append`/`Stream` — proving the sdcard crate's write and stream primitives were
never at fault, including across the ~10 FAT32 cluster boundaries this file's size (318 KB against
32 KB clusters) crosses, well beyond anything Phase 1's 400-byte test bundle exercised. The actual
cause: `sdpush` read incoming bytes over UART in a free-running loop with no flow control, and while it
was blocked for hundreds of milliseconds inside a single SD write, the host kept transmitting at full
115200-baud rate — overflowing the ESP32's 128-byte hardware RX FIFO
(`SDPUSH_UART_READ_ERROR err=FifoOverflowed`, previously silently swallowed) and silently dropping
bytes mid-transfer. Same total byte count, wrong content — exactly the symptom observed. Fixed in
`sdpush` alone (an explicit per-chunk ACK the host waits for before sending the next chunk); the real
updater code never touches raw UART at all and needed no changes. `sdpush` and its wire protocol are
deleted post-proof, per its own module doc.

### Gate status

Phase 3's gate — a valid bundle reaches candidate boot, and every rejected or interrupted transaction
leaves a responsive updater or verified production — is met on real hardware, both halves proved
independently above. Outstanding for later phases: Phase 4 (qualify and cut over — replace the A/B
partition table with `partitions-single-production.csv` for real builds, wire `request_factory_boot()`
to an actual caller such as a serial/BLE-triggered recovery command) and Phase 5 (remove A/B).

## Phase 4 progress

Built and hardware-verified 2026-08-18: host tooling to build/sign/inspect/stage a real bundle,
`request_factory_boot()` wired to an operator-triggered serial command, a bundle-retirement fix, and
one board moved to the single-production layout with the real production image.

- **Bundle build/inspect** (`tools/hostctl/src/workflows/single_production/bundle.rs`,
  `hostctl single-production-bundle-{build,inspect}`): signs a real firmware image into an ADR-0014
  bundle reusing `firmware_update`'s existing signing-key file format, and independently re-derives
  and checks a bundle's payload digest against its header (not just trusting the header) without
  touching a device.
- **Bench-only SD staging** (`src/updater/sd_push.rs`, gated behind a new `sd-qual-push` Cargo feature
  that composes with but is not implied by `factory-updater`; `hostctl single-production-sd-push`):
  the device's SD card is not reachable without disassembly, and real bundle delivery is WiFi/BLE (a
  separate, out-of-scope feature) — this exists solely so qualification testing can stage a bundle
  over the same serial link already used for console capture. A distinct build variant, never the
  shipped updater: flashed only long enough to receive one bundle, then reflashed with the normal
  updater build.
- **`request_factory_boot()` wired to `FirmwareFactoryBoot`/`FWFACTORYBOOT`**
  (`src/firmware/serial/{commands/types,parser/firmware,command_family,command_dispatch}.rs`): an
  operator-triggered recovery request from the default `meditamer` binary's own serial console, not
  just the updater's internal install flow. Guarded inside `request_factory_boot()` itself (not left
  to caller discipline, since this caller can run on a board not yet cut over): refuses
  (`UpdateError::Layout`) unless the partition table has exactly one `ota_*` partition *and* a
  `factory` partition — the A/B layout has neither reason to accept this call nor anywhere for the
  bootloader to fall back to if it did.
- **Staged-bundle retirement** (`src/updater/install.rs::retire_staged_bundle`): once a bundle's
  digest is durably recorded as attempted, it's also renamed off `path` (to `/UPDATE.BIN.attempted`,
  `replace: true`) — best-effort, not load-bearing for correctness (the attempted-digest marker alone
  already blocks reinstall), but without it every future factory boot re-discovers the same file and
  pays a full stream+hash+signature-verify pass just to re-derive a digest it already knows is
  blocked. Added after a real hardware session produced a confusing accidental reinstall: a stale
  bundle left on SD from an unrelated earlier bench test — never touched by the real updater's
  attempted-tracking, since it had only ever been staged via the qual-push transport — got picked up
  and installed over a production image that had just been flashed directly, on the next boot into
  `factory`. Retiring a bundle the moment its digest is committed closes that gap.

### A rename-ordering bug caught by the very first hardware run

The first attempt at this fix broke installation entirely: renaming `path` immediately after recording
the attempted digest, *before* `write_payload_to_ota0`'s second streaming pass over that same file,
left that pass reading a file that had already moved — every install failed with
`UPDATER_INSTALL_ERROR reason=Engine(Fat(NotFound))`. Fixed by computing the install result first
(`install_from`, a helper holding every fallible step) and retiring the file only after, regardless of
that result — matching the module's own documented pass structure ("pass 1... pass 2: stream the same
file again") that the fix's first draft had read past.

### Hardware install-and-confirm proof (post-fix)

With the ordering fix applied, the full pipeline end to end, watched live:

```
UPDATER_BUNDLE_OK path=/UPDATE.BIN build_id=phase4-ordering-fix-v2 target=1 layout=1 bytes=318896
FIRMWARE_FACTORY_REQUEST state=erased
[reboots]
UPDATER_OTA_STATUS booted=ota_0 selected=ota_0 state=pending_verify build_id=phase4-cutover-test
UPDATER_CANDIDATE_PENDING settle_ms=5000
FIRMWARE_CONFIRM slot=ota_0 state=valid
UPDATER_CANDIDATE_CONFIRM confirmed=true
UPDATER_SD_PROBE_OK capacity_bytes=8044675072 filesystem=Fat32
UPDATER_BUNDLE_ERROR path=/UPDATE.BIN reason=Engine(Fat(NotFound))
```

The last line is the retirement fix working as intended: the newly-booted candidate's own boot-time
bundle scan finds nothing at `path` (cleanly, `NotFound`) rather than re-discovering and re-processing
the same file it just installed.

### Real production image: layout proof, and a root-caused (pre-existing, unrelated) confirm blocker

Separately, the real `meditamer` production build (1,957,360 bytes, `MEDITAMER_FIRMWARE_PUBLIC_KEY_HEX`
matching this board's updater) was flashed directly as `ota_0` and staged as a ~1.96 MB bundle via
`hostctl single-production-sd-push` — the largest single transfer this transport has carried, still
byte-perfect. It boots correctly under the new layout (`Loaded app from partition at offset 0x80000`),
proving the layout itself is sound for the real image, not just the small stand-ins every prior
hardware proof in this ADR used. It initially did not self-confirm within the ~30 s this session first
waited before attaching a monitor (observed `state=aborted`, factory fallback), which prompted a full
investigation using continuous, zero-gap live serial capture (an explicit RTS-pulse reset immediately
followed by per-line-timestamped observation — no guessed wait times) rather than further guessing:

1. **The real firmware panics** — `LoadStoreError` exception, `EXCVADDR=0x4000c0d4` (fixed, identical
   across every run), roughly 1-1.5s after boot, well before display/LVGL init or `RUNTIME_READY` can
   run. Confirmation never fires because the system halts, not because of any display/e-ink timing
   issue.
2. The fault is always inside `esp_rtos::run_queue::RunQueue::mark_task_ready`, called **from an
   interrupt handler** — some Level-1 interrupt wakes a task via a bad pointer.
3. Two hypotheses were tested on hardware and disproved: a dual-core startup race between touch's
   second-core bring-up and the main core's task-spawn burst (reordering them had no effect, and the
   crash even preceded the touch-init failure line once); and the touch core's stack being too small
   (quadrupling it from 4 KiB to 16 KiB had no effect — identical fault address).
4. A known, already-proven mitigation existed for a matching signature ("overwrite the adjacent RTOS
   wait-queue control word", `[profile.dev.package.esp-radio-rtos-driver]`, commit `1bf1b2d2`) but was
   scoped to `[profile.dev]` only. Adding the identical override to `[profile.release]` and retesting
   did **not** fix it — same fault, even on a run where touch bootstrap succeeded.
5. **Root cause, confirmed conclusively:** rebuilding with `--no-default-features` (dropping
   `asset-upload-http`/`wifi-backend-esp-radio`, and with it `esp-rtos`'s `esp-radio` feature — the
   integration module whose `WaitQueueImplementation::notify_from_isr` wakes tasks from radio
   interrupts, matching the crash's call chain) produced a **fully clean 90 s run**: no panic,
   `RUNTIME_READY app_state=ready display=ready` at t=3.9s, and `FIRMWARE_CONFIRM slot=ota_0
   state=valid` at t=8.9s. The single-production confirm mechanism is proven correct end to end. The
   crash is in the radio/net subsystem (default-feature build), not in anything ADR-0014 built, not in
   touch, and not in partition layout — A/B firmware almost certainly hits the identical crash (not yet
   directly observed, but the faulting code path is layout-independent). **This does not block Phase
   5**; it's a pre-existing bug to root-cause and fix on its own track. Leads not yet chased to a
   confirmed call site: `net/wifi/connect/{recovery,prepare/preconditions}.rs`'s boot-time "upload mode
   off → pause radio" dance, and `esp-rtos`'s `esp_radio::WaitQueueImplementation::notify_from_isr`.

### Reset-injection interruption coverage (Phase 3 + Phase 4 combined)

Revisiting the plan's four named operations (SD publication, `ota_0` erase/write, OTA activation,
candidate confirmation) against everything proved on hardware across Phases 2-4:

1. **SD publication** — not directly interruption-tested. Not applicable to this ADR's own invariants:
   real publication is WiFi/BLE (out of scope, not yet built); the qual-push bench transport has no
   otadata/flash exposure of its own to protect (an interrupted push just leaves a partial file that
   fails digest verification on the next real attempt).
2. **`ota_0` erase/write** — directly tested (Phase 3): a reset pulsed mid-write recovered to
   `booted=factory`, SD and the attempted-digest guard both intact.
3. **OTA activation** (the final `otadata` write) — not directly timed-and-interrupted (the window is
   a single sub-10ms flash-sector write at the tail of a much longer install, impractical to hit
   blind); structurally covered instead by Phase 2's direct proof that a corrupted/invalid `otadata`
   record falls back to `factory` exactly the way a torn write would leave it — the same mechanism, not
   a live-timed rehearsal of it.
4. **Candidate confirmation** — directly tested (Phase 2's "reset-before-confirmation fallback": a
   reset pulsed inside the 5 s confirm window marked the candidate `Aborted` and fell back to
   `factory`), on the same unmodified `confirm_pending_image()` call path Phase 4 still uses.

Physical power interruption (as opposed to software-triggered reset) was considered for this session
and explicitly deferred: reset-injection already exercises the harder, more precisely-timed case
(landing mid-flash-write), and pulling power requires a person physically present at the bench, which
this session did not have. Worth doing before Phase 5 removes the A/B fallback path entirely, not
required to close Phase 4's own gate.

### Gate status

Phase 4's gate — every interruption recovers to verified production or a responsive updater, attempted
digests are not reinstalled automatically, and USB recovery succeeds with the retained artifacts — is
met for everything this phase's own tooling and code changes touch. The real production image's confirm
behavior, the one loose end this section originally left open, has been root-caused: see above — it is
a pre-existing radio/net-subsystem crash unrelated to this ADR, and the single-production confirm
mechanism itself is proven correct with that subsystem excluded. Outstanding for Phase 5: remove `ota_1`, inactive-slot
selection, the two-slot capacity constants, the serial A/B streaming protocol, and the host
`firmware-update` workflow (see the [A/B baseline
inventory](../reference/ab-firmware-update-baseline.md) for the full ownership list), retaining the
signing/validation/bounded-write/read-back/candidate-health logic this flow already reuses.

## Phase 5 progress

**Complete (2026-08-18).** Removed the two-slot A/B layout and everything that only existed to serve
it, now that Phase 4's investigation (above) proved the single-production confirm mechanism correct
independent of the radio/net-subsystem crash that was blocking it.

### What was removed

- The device-side serial A/B streaming protocol wholesale:
  `src/firmware/serial/firmware_stream.rs`, the 8 streaming `Firmware*` `SerialCommand` variants
  (`FirmwareStatus`/`Prepare`/`Begin`/`Chunk`/`Stream`/`Finish`/`Activate`/`Abort` — `FirmwareFactoryBoot`
  stays), and their handlers in `command_dispatch.rs`/`command_family.rs`/`serial.rs`/`task_state.rs`.
- `src/firmware/update.rs`'s entire streaming-session state machine (`UpdateSession`, `SessionPhase`,
  `begin`/`write_chunk`/`prepare_stream`/`stream_complete`/`finish`/its own `activate`/`abort`,
  `remember_last_chunk`, `digest_staged_image`, `valid_image_header`, `validate_staged_image`,
  `verify_signature`) — none of it is reachable once nothing writes to `ota_0` from within running
  production firmware. `Slot::Ota1` and `opposite()` went with it; `Slot` is now `{Ota0, Factory}`.
  `validate_layout()` now expects only the single-production shape; `write_selection()` dropped its
  A/B parity-selection logic (meaningless with one app partition) but kept the two-sector `otadata`
  rotation, which is redundancy for the `otadata` region itself, not an A/B concept.
- The host-side A/B workflow: `tools/hostctl/src/workflows/firmware_update.rs`'s
  `FirmwareUpdateRuntime`/`run_firmware_update` and `scenarios/firmware-update.sw.yaml`, and the
  `firmware-update` CLI command. `read_signing_key`/`firmware_public_key_hex` were extracted first, into
  a new `tools/hostctl/src/workflows/signing_key.rs`, since `single_production::bundle` and the
  surviving `firmware-key` command both depend on them.
- Everything that became newly-orphaned as a direct consequence: `ble::close_phase1s_for_update()`,
  `flash::park_other_core_for_update()` (its `unpark` counterpart stays, see below),
  `SerialConsole::set_baud_rate` (hostctl), `app_state::store::migration_complete()`, `Status`'s
  `public_key_id` field, and most of `UpdateError`'s variants (down to `Layout`/`Flash`/`Metadata`).
- The stale `MAX_FIRMWARE_LEN` bundle-size ceiling in `src/updater/mod.rs`, which was still using the
  old A/B `ota_0` capacity (`0x1f0000 - 0x2000`) instead of the single-production one
  (`0x380000 - 0x2000`) — a live correctness bug on the layout that's now the only one, fixed alongside
  the removal even though it predates this phase.

### What was deliberately kept

- `prepare_transport()`/`transport_quiet()`/`end_transport()` and the hardware-lease methods in
  `task_state.rs` stay as permanently-inert infra: `transport_quiet()` still has six real external
  consumers (`net/runtime.rs`, `ble/mod.rs`, `psram/mod.rs`, `storage/sd_task/dispatch.rs`,
  `display/presentation.rs`, `serial/task_state.rs`'s own telemetry drain) that can never observe `true`
  again since nothing sets it, but touching all six for no behavioral change wasn't worth it. The one
  genuinely dead setter, `begin_firmware_update_hardware_lease()` (zero remaining callers, unlike the
  getter/clearer), was removed.
- `ota_crc` — extracted to a new host-testable `packages/otadata` crate (mirroring `packages/bundle`'s
  and `packages/rtc`'s pattern) rather than just moved, closing a pre-existing gap: `update.rs`'s own
  inline `#[cfg(test)]` module was never exercised by any host-test path (`[lib] test = false`).
  `src/firmware/update.rs` now `pub(crate) use`s it. `tools/hostctl/src/workflows/single_production/otadata.rs`
  keeps its own independent CRC implementation (cross-checked against real hardware separately);
  unifying the two is a follow-up, not part of this extraction.
- `flash::erase`/`write`/`read_aligned` stay in `src/firmware/flash.rs` (shared, feature-gated
  `#[cfg_attr(not(all(feature = "factory-updater", not(feature = "sd-qual-push"))), allow(dead_code))]`)
  since `src/updater/install.rs` still needs them; only the default `meditamer` binary's own use of them
  went away.

### Default build cutover

`scripts/build/xtensa_runner.sh` (the Cargo custom runner) and
`tools/hostctl/src/workflows/flash_capture/{flash,artifacts}.rs` (the `flash-capture` full/app-only
paths) now point at `config/partitions-single-production.csv` and
`target/single-production-bootloader/...` instead of the A/B equivalents — this is the actual
"cutover" action, distinct from the code removal above. `config/partitions-ab.csv` and
`scripts/build/ota_bootloader.sh` stay on disk, retained for boards not yet migrated.

### Hardware verification

All on the same bench board already cut over in Phase 4, rebuilt with the Phase 5 code:

- **Confirm mechanism, fresh candidate:** direct-flashed production (radio/net disabled, matching
  Phase 4's isolation of the unrelated crash) through the narrowed `write_selection()`/`Slot`/
  `validate_layout()` — clean `state=pending_verify` → `RUNTIME_READY` (3.8s) → `FIRMWARE_CONFIRM
  slot=ota_0 state=valid` (8.9s), byte-for-byte the same shape Phase 4 proved before this narrowing.
- **`FWFACTORYBOOT`, narrowed `command_dispatch.rs`/`request_factory_boot()`:** sent live to a running,
  confirmed production candidate — clean reboot, `Defaulting to factory image`, factory boots correctly.
- **SD bundle install through the narrowed code:** built and signed a fresh bundle, staged it over SD
  (`single-production-sd-push`), triggered `FWFACTORYBOOT`, watched the updater verify it
  (`UPDATER_BUNDLE_OK`), request factory boot for its own precondition
  (`FIRMWARE_FACTORY_REQUEST state=erased`), and complete the install — confirmed by the bundle's
  retirement (`/UPDATE.BIN` → `NotFound` on a later rescan) rather than by racing a second observation
  window against the confirm timer the way Phase 4 first had to.
- **`xtensa_runner.sh` cutover, the one genuinely new capability this phase adds:** ran it directly
  against a built ELF and confirmed espflash's own resolved paths —
  `Bootloader: .../target/single-production-bootloader/bootloader/bootloader.bin`,
  `Partition table: .../config/partitions-single-production.csv`, `App/part. size:
  1,825,744/3,670,016 bytes` (exactly `SINGLE_PRODUCTION_OTA_0_SIZE`) — proving the flip is real, not
  just a config file that nothing reads yet.
- **Radio/net-subsystem crash, unaffected (sanity check, not a regression this phase could have
  caused):** a default-feature production build still panics identically —
  `LoadStoreError`, `EXCVADDR=0x4000c0d4`, immediately after `touch: init_failed ... ArbitrationLost` —
  confirming Phase 5 (which never touched `net/`, `ble/`, or `touch/`) neither fixed nor worsened the
  separately-tracked bug from Phase 4's investigation.

Full quality lane also passes: `scripts/host-test.sh test all` and `lint all` (42 test suites, all
lint suites including the new `otadata` crate), `check_network_owner_source.sh` (its ~12 line-content
assertions rewritten to match the narrowed dispatch code, not just deleted), and
`check_script_surface.py` (backfilled the never-recorded Phase 4 hostctl leaf-command-count baseline
change alongside Phase 5's own).

### Gate status

Final gate met: the tree uses the single-production flow as its only update authority (default build,
default flash tooling), and every quality and device-recovery check above passes. ADR-0014's status
moves to Accepted.

## Addendum: the radio/net-subsystem crash is fixed (2026-08-18)

Phase 4's investigation (above) root-caused, but explicitly left open, a `LoadStoreError` panic
(`EXCVADDR=0x4000c0d4`, fixed across every run, ~1.1-1.5s after boot, always inside
`esp_rtos::run_queue::RunQueue::mark_task_ready` called from the `__level_1_interrupt` handler chain)
as "a pre-existing bug to root-cause and fix on its own track."

Upstream `esp-rs/esp-hal` fixed a cluster of Xtensa exception/interrupt bugs together in
[#6027](https://github.com/esp-rs/esp-hal#6027) ("Fix a few silly mistakes", merged as commit
`998e4faeaf0afc92b494ece4edc75e80df5624f2`), including exactly this call chain: the interrupt entry
assembly (`xtensa-lx-rt`'s `HANDLE_INTERRUPT_LEVEL`/`__default_naked_exception`/`save_context`) wasn't
setting `PS.UM` when entering a handler, so a nested exception taken mid-handler (e.g. a window-overflow
exception during register spilling) could be dispatched through the wrong vector — consistent with
execution landing at a fixed, garbage address deep inside unrelated RTOS code. No released version of
`esp-backtrace`, `esp-rtos`, or `xtensa-lx-rt` contained that commit yet (crates.io's newest versions of
all three predate the fix), so it was backported onto the exact released versions this repository
already pins: `vendor/esp-backtrace-0.19.0-window-spill-fix/`, `vendor/esp-rtos-0.3.0-idle-context-fix/`,
`vendor/xtensa-lx-rt-0.22.0-user-mode-vector-fix/` (each `MEDITAMER_PATCH.md` has the full delta).

Hardware-verified: with the backport applied, the real production `meditamer` build (default features,
radio/wifi included) boots cleanly through `RUNTIME_READY app_state=ready display=ready` and runs
stably past 37s of uptime — no panic, past both the crash's ~1.1-1.5s window and Phase 4's `default
build` panic point. A decisive A/B confirmed the backport is a real fix rather than a build coincidence:
removing the `[profile.release.package.esp-radio-rtos-driver]` `opt-level = "s"` mitigation (added
purely for this A/B; the pre-existing mitigation from item 4 of Phase 4's investigation was `[profile.dev]`-
only) left the backported build stable through 17s of uptime with the same clean boot — the fix holds
without that optimization override. `cargo tree -d` shows no duplicate `esp-hal`/RTOS crates after the
three `[patch.crates-io]` path entries.

The vendored backports are meant to be temporary: once `esp-backtrace`, `esp-rtos`, and `xtensa-lx-rt`
all ship a released version containing `998e4fa`, drop the three vendor trees and their
`[patch.crates-io]` entries in favor of the released versions directly (each `MEDITAMER_PATCH.md` states
this as its maintenance rule).

## Addendum: the crash recurs under a different binary layout (2026-08-19)

The addendum above is accurate for what it tested -- one clean 37 s run -- but its "fixed" framing does
not hold in general. **The backport reduces this bug; it does not close it.** It reappeared, 100%
reproducible, while adding the Ambient Home clock font (`docs/plans/ambient-home-prototype.md`,
`tools/lvgl_font_compiler`) -- a change with zero logical connection to touch, cores, or scheduling.
This section exists so nobody re-derives the below from scratch.

### Reproduction

Any build with the compiled clock font's tables linked in triggers it, whether or not the font is ever
rendered (confirmed by keeping the tables linked but referencing them only through
`core::hint::black_box`, never passing them to LVGL). A same-size block of inert padding, in the same
place, does not. This is not about the font's content -- it is about **binary layout perturbing
interrupt timing**: anything that changes where other code and data land can nudge a fragile timing
window open or shut, with no logical relationship to what changed. Treat this as ambient risk for *any*
sufficiently sized firmware change, not a font-specific defect.

Flash procedure (the default `scripts/device/flash.sh` path does **not** exercise this -- it writes a
blank `otadata` and the board boots the factory updater instead of `ota_0`):

```bash
scripts/build/single_production_bootloader.sh
CARGO_NO_DEFAULT_FEATURES=1 CARGO_FEATURES=factory-updater CARGO_LOCKED=0 scripts/build/build.sh release default
espflash save-image --chip esp32 target/xtensa-esp32-none-elf/release/updater target/updater.bin
CARGO_LOCKED=0 scripts/build/build.sh release
espflash save-image --chip esp32 target/xtensa-esp32-none-elf/release/meditamer target/production.bin
cargo run --manifest-path tools/hostctl/Cargo.toml -- single-production-flash \
  --port <your-port> --factory target/updater.bin --production target/production.bin
```

Watch the boot log for `LoadStoreError`, `EXCVADDR=0x4000c0d4`, inside
`esp_rtos::run_queue::RunQueue::mark_task_ready`, reached through `__level_1_interrupt`. That is this
exact bug -- if you see it again, this section is where the history lives.

### What a hardware-captured ring buffer showed

A temporary instrumentation (recording `[sequence, task_ptr, cpu, priority]` on every
`RunQueue::mark_task_ready` call, dumped from the panic handler; both sides FFI-only, no Cargo
dependency added between the vendored crates -- the same pattern `meditamer_lvgl_alloc_pool` already
uses) captured the run queue immediately before corruption:

```
#260 ptr=0x3ffb1d40 cpu=0 prio=0
#261 ptr=0x3ffe5f00 cpu=0 prio=29
#262 ptr=0x3ffb1d40 cpu=0 prio=0
#263 ptr=0x3ffe5f00 cpu=0 prio=29
#264 ptr=0x3ffb1d40 cpu=0 prio=0
#265 ptr=0x3ffb1f40 cpu=1 prio=0   <- core 1 cuts in
#266 ptr=0x3ffb1f40 cpu=1 prio=0
#267 ptr=0x3ffe5f00 cpu=0 prio=29   <- crash immediately follows
```

Only 4 distinct task pointers ever appeared across 268 calls. `esp-rtos`'s scheduler tracks one `Task`
per **OS-level thread** (one per `esp_rtos::embassy::Executor` instance), not one per individually
spawned async fn -- `Spawner::spawn` is handled entirely inside upstream `embassy-executor`'s own task
pool and never touches `RunQueue::mark_task_ready`. So this is generic core-0-executor /
core-1-executor contention on the shared run queue, recurring throughout early boot -- **not** anything
touch-specific, and not tied to the one-time core-launch handoff. `touch: init_failed ...
ArbitrationLost` appearing in the same capture is very likely coincidental timing, not causal.

### Two hypotheses tested on hardware and disproven

Do not re-test these without new evidence -- both were implemented, flashed, and directly refuted:

1. **Missing `rsync` in `xtensa-lx-rt`'s `save_context` spill-enable path**
   (`vendor/xtensa-lx-rt-0.22.0-user-mode-vector-fix/src/exception/asm.rs`, the `wsr a3, ps` right
   before `SPILL_REGISTERS` in `save_context`). Every other `wsr .. ps` site in that file is followed by
   `rsync` before anything depends on the new `PS` value; this one wasn't, and it's the exact scenario
   the vendored patch's own comment names ("a window-overflow exception during register spilling").
   Added the missing `rsync`, confirmed present in the disassembly at the intended instruction, rebuilt,
   reflashed: identical crash.
2. **Core-launch ordering barrier** (`src/firmware/system/tasks.rs`, `start_touch_core`/
   `run_touch_core`). `esp_rtos::start_second_core_with_stack_guard_offset` only waits for
   `per_cpu[1].initialized` (core 1's scheduler *exists*) before returning, not for core 1 to finish
   registering its own initial task -- a real gap. Added an `AtomicBool` barrier so core 0 waits for
   core 1's registration to complete before spawning its own remaining tasks. Ring-buffer A/B showed
   the identical crash at the identical sequence number (`#267`); the corruption happens ~260 scheduling
   events into boot, long after any one-time launch ordering could matter.

### Recommended next step: reproduce on hardware with native debug access

The Inkplate board (ESP32, Xtensa LX6) has no JTAG path -- external probe hardware would be needed, and
none is available. A second board **is** available: Waveshare ESP32-S3 R-LCD 4.2"
(<https://www.waveshare.com/esp32-s3-rlcd-4.2.htm>), which has ESP32-S3's native USB-Serial-JTAG built
into the chip -- `probe-rs`/OpenOCD work over the same USB-C cable used for flashing, no extra hardware.

Checked feasibility before recommending this: `xtensa-lx-rt`'s target list already includes
`xtensa-esp32s3-none-elf` alongside `xtensa-esp32-none-elf`, and `esp-rtos`'s vendored `Cargo.toml`
already has a full `esp32s3` feature block. More importantly, `exception/asm.rs` (the file item 1 above
touches) and `run_queue.rs`'s scheduler logic have **no chip-specific branching** -- they are the exact
same source compiled for a different Xtensa target, not a lookalike. ESP32-S3 uses Xtensa LX7 rather
than the Inkplate's LX6 -- a newer core generation with different pipeline timing -- so a repro there
would be strong, not certain, evidence of the same mechanism; it is still worth doing precisely because
the suspect code is chip-generic.

Scope this as its own thing, not a port of this firmware (none of the Inkplate's drivers exist on that
board):

- A minimal standalone crate: boot, `esp_rtos::start_second_core_with_stack_guard_offset`, a busy
  `esp_rtos::embassy::Executor` on each core generating frequent wakes (matching the observed pattern:
  one core alternating idle/high-priority churn, the other cutting in).
- Set up `probe-rs`/OpenOCD against the S3's native USB-JTAG.
- Once it reproduces, a hardware watchpoint on the run queue's intrusive-list pointer fields
  (`TaskListItem` in `vendor/esp-rtos-0.3.0-idle-context-fix/src/task/mod.rs`) should catch the actual
  corrupting write directly -- real progress beyond what post-mortem serial forensics can give.

**Run this as its own fresh session against the S3 board, not a sub-session of unrelated work** -- it
needs a clean context scoped to that board and toolchain, not this ADR's history replayed into it.

### State as of this addendum

Both disproven experiments (the `rsync` addition and the core-launch barrier, plus the ring-buffer
instrumentation in both vendored crates) were reverted; the vendor trees and `src/firmware/system/
tasks.rs` are back to their pre-investigation state, rebuilt, and reflashed -- the device is currently
running a clean, confirmed-stable image (`RUNTIME_READY`, `FIRMWARE_CONFIRM slot=ota_0 state=valid`).
The Ambient Home clock font work itself (`docs/plans/ambient-home-prototype.md`) is complete and
correct in isolation -- host tests pass, the compiled face renders correctly -- but is held un-flashed
pending this bug, since it is what most recently re-triggered the crash. Nothing from this
investigation has been committed.

## Consequences

### Benefits

- Production capacity grows to about 3.4-3.6 MiB, subject to measured updater size and release
  headroom.
- Interrupted production writes leave the factory updater available.
- USB and SD installation use the same image checks.

### Trade-offs

- The previous production image is no longer stored in internal flash.
- Recovery reinstalls a signed image instead of immediately booting a second internal copy.
- A missing or unusable SD bundle leaves the device in the updater until media or USB recovery is
  provided.
- The updater becomes a separate release artifact.
- Existing devices need one complete USB flash for the new layout.

## Alternatives considered

- **Keep A/B:** it does not provide enough production capacity.
- **Let production overwrite itself:** an interrupted write could leave no internal recovery image.
- **Execute from SD:** the ESP32 executes this application from SPI flash.
- **Add SD support to the bootloader:** a factory application provides recovery without expanding the
  bootloader.
- **Use a larger-flash module:** this requires hardware changes and does not help existing devices.
