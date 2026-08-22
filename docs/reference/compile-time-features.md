# Compile-Time Features

The LVGL UI, PSRAM allocation, the ESP-HAL runtime, and the 3-second
[panel-power lease](display-refresh.md#binary-panel-power-lease) are part of
every firmware build. They are not selectable Cargo features.

## Supported Cargo Features

- `asset-upload-http` (default): Wi-Fi asset-upload service.
- `asset-upload-http-pipeline` (default): pipelined upload processing; implies
  `asset-upload-http`.
- `wifi-backend-esp-radio`: selected by `asset-upload-http`; retained as the
  explicit backend seam.
- `wifi-debug-slim-app`: reduced diagnostic application; implies
  `asset-upload-http`.
- `telemetry-defmt`: optional `defmt` telemetry.
- `ble-foundation`: non-default Phase 1 BLE controller/host and diagnostic GATT
  probe. It adds the bounded `BLEPROBE START`/`BLEPROBE STATUS` Phase 1D baseline
  controls, but it is not approved for default inclusion.

## Common Profiles

Default production build:

```bash
scripts/build/build.sh release default
```

BLE fixed-cost candidate (size-optimized, still non-default):

```bash
CARGO_FEATURES=ble-foundation scripts/build/build.sh ble-release default
```

`ble-release` inherits the production release settings but optimizes the
first-party firmware for size while retaining the audited radio/storage package
overrides. The ordinary `release` profile remains unchanged until BLE promotion.

No-Wi-Fi build:

```bash
scripts/build/build.sh debug minimal
```

Slim Wi-Fi diagnostic build:

```bash
scripts/build/build.sh debug slim
```

Telemetry build:

```bash
scripts/build/build.sh debug telemetry
```

All supported features together:

```bash
scripts/build/build.sh debug all-features
```

The build wrapper uses `--locked` by default. Set `CARGO_LOCKED=0` only while
intentionally updating a lockfile. Legacy `CARGO_FEATURES` and
`CARGO_NO_DEFAULT_FEATURES` overrides remain available for experimental builds.

## Software Baseline

Run the complete software baseline with:

```bash
scripts/ci/check_software_baseline.sh all
```

The canonical CI lanes are `source`, `host`, `firmware`, `static`, and
`quality`. Firmware Clippy is strict in both `minimal` and `all-features`
profiles so conditional code cannot hide warnings in the opposite profile.
LOC reports in `quality` are advisory; architecture and code-analysis ratchets
remain blocking.

Hardware validation is a separate promotion gate; a clean software baseline
does not claim that firmware has been flashed or exercised on a device.
