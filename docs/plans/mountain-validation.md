Status: Active
Last-reviewed: 2026-10-01

# Validate the Mountain storage workaround

## Outstanding work

The 2026-10-01 documentation review identified a scoped CPU/FIFO transport
workaround in the
[Mountain read handler](../../products/meditamer/src/firmware/storage/sd_task/mountain_read.rs).
Combined-workload qualification remained open. Confirm the implementation and
select the candidate artifact before running this plan; the documentation move
has not established device results.

## Acceptance

Use one identified production artifact across these checks:

- Validate, render, adopt and compose Mountain assets without panic or hang.
  Reuse validation metadata when the asset generation is unchanged.
- Exit and re-enter overlays repeatedly. Asset release must prevent growing
  external-memory usage.
- Run default/BLE composition builds and image-size, linked-stack, internal-memory
  reservation, waveform and IRAM checks using [memory validation](../guides/memory/validation.md).
- Run canonical one- and three-cycle Wi-Fi uploads and integrated asset-residency
  tests using the [Wi-Fi regression gate](../guides/network/validation.md).
- Check runtime stack headroom and memory against the
  [Inkplate budget](../references/memory/meditamer-inkplate/budget.md#acceptance),
  including its current admission requirements.
- Compare median upload throughput with a valid baseline; the recorded SD
  acceptance floor permits at most a 10 percent regression. Confirm baseline
  artifact and workload identity before comparing.

## Completion evidence

Record firmware/source identity, configuration, methods, logs, stack and memory
measurements, throughput comparison, panel observations and any limitations.
Use [runtime metrics](../references/runtime/metrics.md) for device measurements.
An isolated successful test does not complete combined-workload qualification.
