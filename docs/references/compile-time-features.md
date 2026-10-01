# Compile-time features

Feature selection and defaults are owned by the target manifests:
[Inkplate](../../targets/meditamer-inkplate/Cargo.toml) and
[Medinote/Waveshare](../../targets/medinote-waveshare/Cargo.toml).
The tables below cover application composition and commonly used diagnostics.
Build commands and wrapper profiles live in [build and flash](../guides/development/build-and-flash.md#build).

## Inkplate

LVGL, PSRAM allocation, the ESP-HAL runtime and the 3-second
[panel-power lease](../../products/meditamer/src/firmware/display/panel/lease.rs)
are unconditional parts of the application.

| Feature | Selection | Effect |
| --- | --- | --- |
| `cpu-load` | Default | Per-core busy/IRQ counters, typed observations and Ambient diagnostics; selects `context-probe`. |
| `asset-upload-http` | Default | Wi-Fi HTTP upload service; selects `wifi-backend-esp-radio`. |
| `asset-upload-http-pipeline` | Default | Pipelined uploads; implies `asset-upload-http`. |
| `wifi-backend-esp-radio` | Via upload | Explicit ESP radio backend seam. |
| `wifi-debug-slim-app` | Opt-in | Reduced diagnostic application; implies `asset-upload-http`. |
| `telemetry-defmt` | Opt-in | Defmt telemetry. |
| `ble-foundation` | Default | Diagnostic peripheral with exclusive Wi-Fi/BLE handoff; hardware qualification remains separate. |
| `shared-ble-runtime` | Opt-in | Shared central runtime and controller, scan and connect probes. |

### Diagnostics and fixtures

All are diagnostic selections; restore a normal image after a fixture or standalone probe.

| Feature | Effect / instructions |
| --- | --- |
| `context-probe` | Temporary reset diagnosis; selected by `cpu-load`. |
| `cpu-load-probe` | Standalone CPU bench; implies `cpu-load`. [CPU load](runtime/metrics.md#cpu-load-inkplate). |
| `firmware-trace` | Bounded application event capture; implies `cpu-load`. [Tracing](../guides/diagnostics/tracing.md). |
| `ui-interaction-trace` | Touch, LVGL and presentation correlation; implies `firmware-trace`. Hooks/storage are absent from ordinary builds. |
| `ui-initialization-fixture` | Display-model/LVGL initialization, rejection and cleanup checks. [Fixtures](../guides/diagnostics/inkplate-fixtures.md#ui-initialization). |
| `sd-runner-allocation-fixture` | FAT/runner allocation rejection and unpolled cleanup checks. [Fixtures](../guides/diagnostics/inkplate-fixtures.md#sd-runner-allocation). |
| `panel-waveform-fixture` | Early-boot Gray4 or binary pattern and panel phase timings, then parks. [Fixtures](../guides/diagnostics/inkplate-fixtures.md#panel-waveform). |

Reset diagnostics retain the first implausible IRQ task pointer and RTOS task-context
copy per core, last dispatched IRQ ID/phase and last Wi-Fi connect phase for the
next boot. These records supply context; they do not identify a writer or fix a crash.

## Medinote / Waveshare

| Feature | Selection | Effect |
| --- | --- | --- |
| `cheertok-controls` | Default | CheerTok input; selects `shared-ble-runtime`. |
| `shared-ble-runtime` | Via CheerTok | Shared BLE central, security/legacy pairing and controller, scan, connect and diagnostic-client probes. |
| `hourglass-block-gravity` | Default | Shared Margolus phase with per-block cardinal transitions from local gravity; reports `gravity_schedule=block-weighted`. |
| `sd-storage` | Default | Native SD with shared FAT service, lazy enumeration, bounded SDFAT commands and sleep quiescence; no auto-format. |
| `sd-transport-probe` | Opt-in | Isolated SDMMC probe; rejects radio/crash-screen combinations. Writes require explicit scratch settings. See the [probe source](../../targets/medinote-waveshare/src/bin/sd_transport_probe.rs). |
| `wifi-storage` | Opt-in | SD-backed Wi-Fi configuration and shared HTTP uploads; composes SD, netstack and HTTP upload. [Build/configuration](../guides/network/asset-upload.md#build-and-provision). |
| `crash-screen` | Explicit production selection | Panic display hook; kept out of Cargo defaults because other binaries do not supply the hook. |

Disable default features for isolated peripheral qualification. The target manifest
also owns experimental variants and each probe's required features.

## Radio composition constraints

The [BLE manifest](../../platform/connectivity/ble/Cargo.toml) owns chip and
central/peripheral role selection; Cargo unifies selected features.
Both Inkplate BLE selections enable the product's internal
`classic-bt-memory-reclaim`: irreversible for the boot and incompatible with
Classic-capable images. Medinote's ESP32-S3 has no corresponding Classic region.

Measure each target/composition independently against its
[memory budget](memory/README.md). Upload token and runtime network policy are
owned by [Wi-Fi upload](../guides/network/asset-upload.md#build-and-provision).
