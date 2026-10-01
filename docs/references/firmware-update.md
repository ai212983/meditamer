# Firmware updater contracts

[Update guide](../guides/development/firmware-update.md) owns build/sign/staging/recovery.
The factory updater validates signature, target/layout, length ceiling and payload
digest before installing to `ota_0`.

## Bundle verification

Bundle build reports `bundle_bytes`, `firmware_len`, `firmware_digest`, `build_id`
and `public_key_hex`. The signing key must match the public key compiled into the
updater, or installation fails signature verification.

Inspection reports `signature_valid` and `payload_digest_matches` independently;
a signed header does not prove the payload remained intact. Omitting `--public-key`
skips signature verification. Both checks must pass before staging a bundle.

## Status and failed candidates

- `UPDATER_OTA_STATUS booted=<factory|ota_0> selected=<...> state=<...>` —
  which partition is running and the bootloader's opinion of `ota_0`'s
  candidate state.
- `UPDATER_BUNDLE_OK` / `UPDATER_BUNDLE_ERROR reason=<...>` — SD bundle
  verify outcome (signature, target/layout match, length ceiling, digest).
- `UPDATER_INSTALL_OK bytes=<n>` (followed by a reboot) /
  `UPDATER_INSTALL_ERROR reason=<...>` — install outcome, if verify passed.
- `UPDATER_INSTALL_BLOCKED reason=already_attempted` — this bundle's digest
  was recorded as attempted, successfully or not; it will not auto-retry.
  The digest covers only firmware payload bytes, so changing `--build-id` alone
  does not change it. Recording an attempt also renames the staged bundle to
  `/UPDATE.BIN.attempted`.
- `UPDATER_CANDIDATE_PENDING` / `UPDATER_CANDIDATE_CONFIRM confirmed=<bool>`
  — the installed candidate's post-reboot confirmation window.

The attempted digest is recorded and OTA selection is set to factory before
writing `ota_0`. An interrupted production write therefore returns to the updater
on reset; it does not require a valid production image. This recovery ordering
is owned by [the updater](../../targets/meditamer-inkplate/src/updater/mod.rs).
