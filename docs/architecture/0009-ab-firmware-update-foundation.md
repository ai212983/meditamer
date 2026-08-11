# ADR-0009: Adopt a signed A/B firmware-update foundation

- Status: Accepted
- Author: Codex
- Date: 2026-08-10
- References: [UI/app rework plan](../plans/ui-app-structure-rework-plan.md),
  [implementation ledger](../plans/ui-app-structure-rework-ledger.md),
  [DRAM budget](../reference/dram-budget.md),
  [build and flash guide](../guides/build-and-flash.md)

## Context

The board has one 4 MiB ESP32 flash device. The Phase 5 release used a factory-style single-image
layout and stored device lifecycle state in the final flash sector. That layout cannot safely stage a
second complete image, and the previous app-only recovery path assumed application offset `0x10000`.

The accepted Phase 5A signed release image is 1,831,392 bytes. Two aligned `0x1f0000` application
slots fit only if there is no factory application and no native-module region. The measured image
leaves 200,224 bytes (10.9%) in each slot. Future firmware needs an enforced minimum rather than
relying on the current spare space as an informal promise.

The ESP-IDF bootloader supports an OTA select record with `NEW`, `PENDING_VERIFY`, `VALID`, and
rollback states. The Rust application can own staging and confirmation without giving flash or boot
metadata ownership to LVGL. Authenticating update content does not make the device resistant to a
physical attacker who can enter the ROM downloader and replace the bootloader.

## Decision

Use a no-factory A/B layout, the pinned ESP-IDF v5.5.2 rollback bootloader, and a base-owned signed
update service.

### Stable flash map

| Label | Offset | Size | Purpose |
| --- | ---: | ---: | --- |
| `nvs` | `0x9000` | `0x6000` | ESP NVS |
| `otadata` | `0xf000` | `0x2000` | Alternating OTA selection sectors |
| `phy_init` | `0x11000` | `0x1000` | PHY initialization data |
| `app_state` | `0x12000` | `0x2000` | Alternating lifecycle-state sectors |
| `ota_0` | `0x20000` | `0x1f0000` | Application slot A |
| `ota_1` | `0x210000` | `0x1f0000` | Application slot B |

Every accepted application image must leave at least `0x20000` bytes (128 KiB) unused in its slot.
The firmware receiver and host full-flash/update paths reject larger images. This floor leaves room
for bounded product growth but is not a promise that every future feature will fit.

There is no native-module partition. ADR-0006's native-loader branch is parked unless a successor
decision finds space without reducing either slot below this floor.

### Boot and recovery authority

Build the bootloader from `tools/ota_bootloader/` with the repository script and the checked-in
partition CSV. Pin ESP-IDF v5.5.2 and enable `CONFIG_BOOTLOADER_APP_ROLLBACK_ENABLE`. The generated
bootloader, partition-table binary, application binary, hashes, and build metadata are archived by
the full-flash workflow.

The ROM serial downloader remains the recovery root. A host full flash writes the pinned bootloader,
accepted partition table, initial OTA data, and `ota_0`. Its primary stub-assisted attempt may use a
proven faster baud, but automatic recovery repeats the same complete transaction through a
conservative ROM-only path; it does not silently degrade to app-only. Explicit app-only recovery
resolves `ota_0` from the CSV. No production path may retain a fixed `0x10000` application address.
This decision does not burn Secure Boot, flash encryption, or anti-rollback eFuses.

### State migration

Move `AppStateStore` to two sectors in `app_state`. On the first A/B boot, read the compatible legacy
version-2 or version-3 record at `0x3ff000`, write a version-4 generation and CRC record to
`app_state`, and read it back. Version 3 state is preserved; version 2 is migrated to safe defaults
because the intervening version-3 firmware deliberately treated it as obsolete.
Only a verified new record permits writing the inactive OTA slot, because `ota_1` covers the legacy
sector. Retain the legacy source until migration succeeds. Repeated or interrupted migration is
idempotent, and later saves alternate the two new sectors.

### Update transaction and authenticity

The base service, not a UI provider, owns the flash peripheral and OTA session. It:

1. derives the inactive slot from the actually booted partition;
2. retains sequential aligned 48-byte hex chunks for compatibility and negotiates CRC-framed binary
   payloads of at most 112 bytes at 460800 for capable firmware; every complete binary frame fits
   UART0's 128-byte FIFO, and two payloads coalesce into at most 224-byte internal-RAM writes split at
   256-byte flash page boundaries;
3. pre-erases only the aligned inactive-slot image range before binary streaming, while compatible
   hex staging retains lazy sector erase;
4. checks ESP32 image structure, exact length, chip identity, and the appended-image-hash shape;
5. hashes the received bytes, verifies an Ed25519 signature, reads the staged bytes back from flash,
   and verifies the same SHA-256 before activation;
6. writes one complete OTA select record into the older metadata sector and verifies the selected
   slot and state before reboot.

The signed message is the ASCII domain `MEDITAMER-FIRMWARE-V1`, followed by the little-endian `u32`
image length and SHA-256 of the complete application binary. The public key is compiled into the
firmware through `MEDITAMER_FIRMWARE_PUBLIC_KEY_HEX`; the private 32-byte seed is never stored in the
repository. A build without a configured key boots normally but refuses update staging. Signed
downgrades remain possible; monotonic version enforcement is outside this foundation.

Before the transaction, the runtime suppresses unrelated serial traces, suspends the shared
panel-bus clients (IMU plus touch acquisition and processing), and cooperatively parks core 1 once;
the single base flash owner holds transport quiet through the digest and verified-status
acknowledgements, then queues client resume without waiting for the first recovered sensor sample.
Abort similarly acknowledges before releasing the lease.
The update transcript records maximum sector-erase and chunk-write call durations plus the full
read-back duration. Firmware accepts an exact resend of only the immediately previous chunk, making
a bounded host retry after a missing chunk acknowledgement idempotent. Explicit errors and an
ambiguous activation acknowledgement stop without retry.

### Candidate confirmation

The rollback bootloader changes a newly selected image to `PENDING_VERIFY`. The application confirms
it only after the display task reaches `RUNTIME_READY` and remains alive for another five seconds.
Reset before confirmation causes the bootloader to mark the candidate unusable and select the prior
valid slot. Serial readiness and this software gate are not evidence that the physical panel or touch
path is correct; identified-artifact physical observation remains separate release evidence.

Production network transport and launcher update UX are not part of this decision. They may submit
content to the base service only after separate bounded transport and product decisions.

## Consequences

- Interrupted staging cannot overwrite the running slot, and activation retains an independently
  valid metadata sector.
- Automatic rollback covers candidates that reset or fail before the health boundary.
- Host full-flash and app-only recovery now share one explicit partition authority.
- OTA authenticity protects the application update channel, but not a device in the hands of an
  attacker who can use ROM serial recovery.
- Every update erases the used sectors of the inactive slot and one OTA metadata sector; metadata
  sectors alternate, while application wear alternates with slots.
- A/B consumes the flash region previously considered for native modules and leaves roughly ten
  percent current-image headroom. Capacity must be remeasured on every release.
- Older firmware can still be updated through the slower hex transport. Capable firmware uses the
  FIFO-safe binary transport; a production network transport and update UX remain separate decisions.

## Alternatives considered

- **Keep one image and overwrite it:** rejected because interruption can leave no bootable product
  image.
- **Add a factory image:** rejected because three current images do not fit in 4 MiB.
- **Use a custom Rust bootloader:** rejected for this phase because the pinned ESP-IDF bootloader
  already supplies the required rollback state machine and ROM recovery remains available.
- **Trust a content hash without a signature:** rejected because integrity alone does not establish
  update authority.
- **Burn Secure Boot and anti-rollback eFuses now:** rejected because they are irreversible production
  provisioning decisions and are not required to prove recoverable A/B mechanics.
- **Reserve native-module flash by shrinking both slots:** rejected because it would violate the
  accepted application-capacity floor.
