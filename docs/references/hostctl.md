# Hostctl transport and artifacts

Use `scripts/hostctl.sh` for host commands. It prepares the host Cargo target and
`stable` toolchain, then the CLI sets its working directory to the repository
root: relative typed paths and default evidence paths are repository-relative.
It does not load `.env.local`; Wi-Fi hardware wrappers do.

Explicit command ports take precedence over command-specific environment
(`HOSTCTL_PORT`, or `HOSTCTL_NET_PORT` for network workflows), then a valid cached
port, then unambiguous detection. Keep one serial owner and use new output paths
where workflows refuse existing evidence. Passive transport preserves DTR/RTS;
some boards/adapters still reset on attachment, so workflows have their own
readiness/settling procedure.

## Flash capture

The [flash guide](../guides/development/build-and-flash.md) owns operation.
`flash.log`, `capture.log`, and `summary.txt` identify the flash and startup.
The image ceiling is `ota_0` capacity minus 128 KiB; read capacity from the
[partition table](../../config/partitions-single-production.csv).

Full flash writes the bootloader, partition table, OTA selection and application,
preserving the factory image. Fallback repeats the complete flash at a conservative
baud without the stub. App-only preserves OTA boot selection and reads the
application offset from the partition table. Boot capture opens serial before its
single reset and retains that descriptor through time synchronization.

| Control | Default / meaning |
| --- | --- |
| `ESPFLASH_BAUD` | `460800` full-flash stub baud |
| `ESPFLASH_FALLBACK_BAUD` | `115200` conservative full/app-only baud |
| `FLASH_TIMEOUT_SEC` | `360` |
| `ESPFLASH_ENABLE_FALLBACK` | `1`, complete ROM-only fallback |
| `ESPFLASH_SKIP_UPDATE_CHECK` | `1` |
| `HOSTCTL_FLASH_CAPTURE_BOOT_WINDOW_MS` | `8000` |
| `HOSTCTL_FLASH_CAPTURE_BOOT_TARGET` | Wrapper: `production`; direct CLI: `factory` |
| `HOSTCTL_FLASH_CAPTURE_LOG_PATH` | Optional artifact directory |
| `FLASH_SET_TIME_AFTER_FLASH` | `1`; `0` disables sync; CLI `--no-time-sync` always wins |

Time sync answers the device's bounded `TIME_REQUEST`, generally retained from
boot capture. It appends `RTC_DIAG`/`TIME_SYNC` lines to `capture.log`;
`capture_bytes` counts the original capture window. Summary fields:

- `time_sync=ok|skipped|failed` and `time_sync_reason` (`n/a` when inapplicable).
- `time_sync_requested_utc` / `time_sync_requested_offset_min`: requested pair.
- `time_sync_utc` / `time_sync_offset_min`: verified delayed readback; UTC can
  differ from the request by a second or two.

Failed sync preserves flash artifacts, skips `--post-command`, and exits nonzero
with `flash=ok time_sync=failed`. Explicitly skipped sync permits post-command.

## Monitor

[Monitor workflow](../guides/development/build-and-flash.md#monitor): `scripts/device/monitor.sh`.

| Variable | Default / purpose |
| --- | --- |
| `ESPFLASH_BAUD` | `115200` |
| `ESPFLASH_MONITOR_MODE` | `passive`; `espflash` for defmt; `raw` for direct reading |
| `ESPFLASH_MONITOR_BEFORE` / `AFTER` | `no-reset-no-sync` / `no-reset` |
| `ESPFLASH_MONITOR_PERSIST_RAW` | `1`, reconnect after unplug |
| `ESPFLASH_MONITOR_RAW_BACKEND` | `auto`, prefers `tio`, then `cat` |
| `ESPFLASH_MONITOR_OUTPUT_MODE` | `normal`; `hex` diagnoses garbled bytes |

## Clock synchronization

`time-status` succeeds on a parsed `TIMEGET OK`, including `valid=off`; command
success alone does not establish clock validity. `time-set` opens a fresh
device-demand synchronization session.

## Artifact retention

`artifacts inventory` reports sizes, classes, retention, and due reviews.
`artifacts prune` defaults to dry-run thinning of recognized flash payloads:
`firmware.elf`, `app.bin`, `bootloader.bin`, and `partition-table.bin`.
`--apply` removes candidates and writes `logs/.prune-reports/`;
`--ignore-age` bypasses ages while respecting retention.

| Class | Default age |
| --- | --- |
| Recognized flash payload | 7 days |
| Passed Wi-Fi-regression-gate run unit | 30 days |
| Failed/inconclusive run unit | 90 days |
| Standalone log | 30 days |
| Prior prune report | 90 days |

Whole-run/file expiry requires `--runs`; the [cleanup guide](../guides/development/setup.md#evidence-cleanup)
describes candidate review. Retention metadata lives in `<run>/.retain.json`, or
`<file>.retain.json` for a standalone file:

```json
{
  "scope": "debug",
  "reason": "Candidate awaiting physical qualification",
  "owner": "firmware",
  "review_after": "YYYY-MM-DD"
}
```

`evidence` prevents whole-run expiry; `reflash` also retains application,
bootloader, and partition images; `debug` also retains ELF evidence. An overdue
review is reported but does not remove retention. Required constraints and qualification summaries must survive deletion of raw artifacts.
