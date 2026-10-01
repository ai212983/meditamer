# Firmware update

Inkplate uses a factory updater and one production partition. Install signed
bundles staged on SD; [updater contracts](../../references/firmware-update.md) own
verification, attempted-bundle handling and status. The ordinary development
loop is [build and flash](build-and-flash.md). Set `$DEVICE_PORT` explicitly.

## Complete USB flash

Builds the pinned bootloader/partition-table once, then writes bootloader,
partition table, a freshly constructed initial `otadata`, the factory
(updater) image, and the production (`ota_0`) image in one `esptool.py`
call. The updater is a separate, size-minimal build target
(`--features factory-updater`); the production image is the plain default
build — both must share the same public key and build id. Generate a signing
seed once (or use an existing protected seed), then use its printed public key
in both builds:

```bash
scripts/device/generate_firmware_signing_key.sh target/updater-signing.seed
scripts/build/single_production_bootloader.sh

MEDITAMER_FIRMWARE_PUBLIC_KEY_HEX=<64-hex-public-key> \
MEDITAMER_FIRMWARE_BUILD_ID=<build-id> \
FIRMWARE_BIN=updater CARGO_NO_DEFAULT_FEATURES=1 CARGO_FEATURES=factory-updater \
scripts/build/build.sh release default
espflash save-image --chip esp32 \
  target/xtensa-esp32-none-elf/release/updater target/updater.bin

MEDITAMER_FIRMWARE_PUBLIC_KEY_HEX=<64-hex-public-key> \
MEDITAMER_FIRMWARE_BUILD_ID=<build-id> \
scripts/build/build.sh release default
espflash save-image --chip esp32 \
  target/xtensa-esp32-none-elf/release/meditamer target/production.bin

scripts/hostctl.sh single-production-flash \
  --port "$DEVICE_PORT" \
  --factory target/updater.bin \
  --production target/production.bin
```

`--bootloader`/`--partition-table` default to
`target/single-production-bootloader/{bootloader/bootloader.bin,partition_table/partition-table.bin}`.

## Build and sign a bundle

```bash
scripts/hostctl.sh single-production-bundle-build \
  --firmware target/production.bin \
  --key target/updater-signing.seed \
  --build-id <build-id> \
  --out target/production-bundle.bin
```

Inspect the bundle without touching a device, using the public key compiled into
the updater:

```bash
scripts/hostctl.sh single-production-bundle-inspect \
  target/production-bundle.bin --public-key <64-hex-public-key>
```

Require both [verification checks](../../references/firmware-update.md#bundle-verification) before staging.

## Bench SD staging

Field bundle delivery is separate from updater installation. For bench staging,
use a distinct **bench-only** updater variant that receives serial bytes instead
of verifying/installing:

```bash
FIRMWARE_BIN=updater CARGO_NO_DEFAULT_FEATURES=1 CARGO_FEATURES=sd-qual-push \
scripts/build/build.sh release default
espflash save-image --chip esp32 \
  target/xtensa-esp32-none-elf/release/updater target/updater-sdqualpush.bin

scripts/hostctl.sh single-production-flash \
  --port "$DEVICE_PORT" \
  --factory target/updater-sdqualpush.bin \
  --production target/updater-sdqualpush.bin

scripts/hostctl.sh single-production-sd-push \
  --port "$DEVICE_PORT" \
  target/production-bundle.bin
```

Reflash `--factory` with the *normal* updater build (the first section
above, without `sd-qual-push`) before the next reset — the qual-push
variant never verifies or installs anything; it only stages the file.

## Status and ROM recovery

Attach the [passive monitor](build-and-flash.md#monitor); updater status is plain
text. Check OTA selection, bundle verification, install and candidate confirmation
using the [status reference](../../references/firmware-update.md).
For `already_attempted`, build a bundle containing different firmware content
before retrying.
USB ROM flashing is independent of OTA/SD state. A complete USB flash above
recovers the layout, including a device still using older partitions.
