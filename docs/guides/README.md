# Guides

Workflows grouped by topic, with one owner for humans and automation alike.
Current contracts belong in [references](../references/README.md); decisions
belong in [architecture](../architecture/README.md).

## Development

- [Host setup, hooks, and software checks](development/setup.md)
- [Build, flash, synchronize time, and monitor](development/build-and-flash.md)
- [Sign, install, and recover firmware updates](development/firmware-update.md)
- [Hostctl workflow development](../../tools/hostctl/DEVELOPMENT.md)

## Diagnostics

- [Troubleshooting and failure classification](diagnostics/troubleshooting.md)
- [Service, SD, UI lifecycle, and scheduling validation](diagnostics/device-validation.md)
- [Firmware tracing and Perfetto export](diagnostics/tracing.md)
- [Inkplate initialization, allocation, and panel fixtures](diagnostics/inkplate-fixtures.md)

## Network

- [Wi-Fi provisioning and SD asset upload](network/asset-upload.md)
- [Acceptance, regression, and Wi-Fi/BLE handoff](network/validation.md)

## Memory

- [Placement, headroom, and asset re-entry validation](memory/validation.md)

## Observations

- [Sensor fixtures, panel interruption, and sleep cleanup](observations/validation.md)

## Audio

- [Buzzer score authoring, listening, and A/B trials](audio/buzzer-listening.md)

## Reference lookup

[UART commands](../references/runtime/serial-control.md) ·
[Metrics](../references/runtime/metrics.md) ·
[Host transport and artifact retention](../references/hostctl.md)
