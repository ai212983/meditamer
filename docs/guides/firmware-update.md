# Firmware Update (ADR-0014)

A small factory updater partition plus one large production partition; the
factory updater installs signed bundles staged on the SD card. See
[ADR-0014](../architecture/0014-single-production-sd-recovery-updater.md) for
the design and hardware proof. This is the only update mechanism — Phase 5
removed the earlier two-slot A/B layout and its serial streaming protocol; see
[ADR-0009](../architecture/0009-ab-firmware-update-foundation.md) (superseded)
and the [A/B baseline inventory](../reference/ab-firmware-update-baseline.md)
for that history.

Building and flashing in general — including the ordinary
`scripts/device/flash.sh` path used for day-to-day development — is in
[Build, Flash, and Monitor](build-and-flash.md).

## Complete USB flash

Builds the pinned bootloader/partition-table once, then writes bootloader,
partition table, a freshly constructed initial `otadata`, the factory
(updater) image, and the production (`ota_0`) image in one `esptool.py`
call. The updater is a separate, size-minimal build target
(`--features factory-updater`); the production image is the plain default
build — both must share the same public key and build id:

```bash
scripts/build/single_production_bootloader.sh

MEDITAMER_FIRMWARE_PUBLIC_KEY_HEX=<64-hex-public-key> \
MEDITAMER_FIRMWARE_BUILD_ID=<build-id> \
CARGO_NO_DEFAULT_FEATURES=1 CARGO_FEATURES=factory-updater CARGO_LOCKED=0 \
scripts/build/build.sh release default
espflash save-image --chip esp32 \
  target/xtensa-esp32-none-elf/release/updater target/updater.bin

MEDITAMER_FIRMWARE_PUBLIC_KEY_HEX=<64-hex-public-key> \
MEDITAMER_FIRMWARE_BUILD_ID=<build-id> \
scripts/build/build.sh release default
espflash save-image --chip esp32 \
  target/xtensa-esp32-none-elf/release/meditamer target/production.bin

cargo run --manifest-path tools/hostctl/Cargo.toml -- single-production-flash \
  --port /dev/cu.usbserial-540 \
  --factory target/updater.bin \
  --production target/production.bin
```

`--bootloader`/`--partition-table` default to
`target/single-production-bootloader/{bootloader/bootloader.bin,partition_table/partition-table.bin}`.

## Build and sign a bundle

```bash
scripts/device/generate_firmware_signing_key.sh target/updater-signing.seed
cargo run --manifest-path tools/hostctl/Cargo.toml -- single-production-bundle-build \
  --firmware target/production.bin \
  --key target/updater-signing.seed \
  --build-id <build-id> \
  --out target/production-bundle.bin
```

Prints `bundle_bytes`, `firmware_len`, `firmware_digest`, `build_id`, and
`public_key_hex` — the public key must match what the updater was built
with (`MEDITAMER_FIRMWARE_PUBLIC_KEY_HEX` above), or every install attempt
will fail signature verification. Inspect any bundle (built here or
elsewhere) without touching a device:

```bash
cargo run --manifest-path tools/hostctl/Cargo.toml -- single-production-bundle-inspect \
  target/production-bundle.bin --public-key <64-hex-public-key>
```

Reports the header fields, whether the signature verifies against the given
key (omit `--public-key` to skip that check), and whether the file's actual
payload bytes still hash to what the header committed to
(`payload_digest_matches` — independent of signature validity: a bundle can
carry a genuine signature over a header whose digest field no longer
matches payload bytes truncated or corrupted after signing).

## Getting a bundle onto the device's SD card

The device's SD card is not reachable without disassembly. Real bundle
delivery to a fielded device is WiFi or BLE — a separate feature, not part
of this ADR. For bench qualification, `hostctl` can push a bundle over the
same serial link used for console capture, but only to a board flashed with
a distinct **bench-only** build variant that trades the updater's normal
verify/install behavior for a serial receiver:

```bash
CARGO_NO_DEFAULT_FEATURES=1 CARGO_FEATURES=sd-qual-push CARGO_LOCKED=0 \
scripts/build/build.sh release default
espflash save-image --chip esp32 \
  target/xtensa-esp32-none-elf/release/updater target/updater-sdqualpush.bin

cargo run --manifest-path tools/hostctl/Cargo.toml -- single-production-flash \
  --port /dev/cu.usbserial-540 \
  --factory target/updater-sdqualpush.bin \
  --production target/updater-sdqualpush.bin

cargo run --manifest-path tools/hostctl/Cargo.toml -- single-production-sd-push \
  --port /dev/cu.usbserial-540 \
  target/production-bundle.bin
```

Reflash `--factory` with the *normal* updater build (the first section
above, without `sd-qual-push`) before the next reset — the qual-push
variant never verifies or installs anything; it only stages the file.

## Updater status and failed-candidate handling

Attach a monitor (`scripts/device/monitor.sh`, raw mode is fine — the
updater's own status lines are plain text, not defmt) to watch the boot
sequence:

- `UPDATER_OTA_STATUS booted=<factory|ota_0> selected=<...> state=<...>` —
  which partition is running and the bootloader's opinion of `ota_0`'s
  candidate state.
- `UPDATER_BUNDLE_OK` / `UPDATER_BUNDLE_ERROR reason=<...>` — SD bundle
  verify outcome (signature, target/layout match, length ceiling, digest).
- `UPDATER_INSTALL_OK bytes=<n>` (followed by a reboot) /
  `UPDATER_INSTALL_ERROR reason=<...>` — install outcome, if verify passed.
- `UPDATER_INSTALL_BLOCKED reason=already_attempted` — this bundle's digest
  was already recorded as attempted (successfully or not); it will not
  auto-retry. The digest covers only the firmware payload bytes — changing
  `--build-id` alone does not change it. Publish a bundle wrapping different
  firmware content to force a fresh attempt. A successfully- or
  unsuccessfully-attempted bundle is also renamed off its staged path
  (`/UPDATE.BIN.attempted`) once its digest is recorded, so a repeat
  `single-production-bundle-inspect`/re-push cycle won't find the old file
  sitting there either.
- `UPDATER_CANDIDATE_PENDING` / `UPDATER_CANDIDATE_CONFIRM confirmed=<bool>`
  — the installed candidate's post-reboot confirmation window.

Every rejected or interrupted install leaves a **responsive updater**, not
a bricked device: the attempted digest is recorded and `otadata` is pointed
at `factory` *before* any flash write to `ota_0` begins (see ADR-0014's
Phase 3 interruption-recovery proof), so a reset at any point during the
write recovers to `booted=factory` on the next boot, with the SD card and
bundle still intact.

## ROM recovery

USB flashing through the ESP32 ROM downloader is always available
independent of `otadata` or SD state — the complete-flash command above
recovers a device in any state, including one still on the old A/B layout
(no `factory` partition). This is the recovery path of last resort
described in ADR-0014's Context.
