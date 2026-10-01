# Radio qualification

Qualification evidence is specific to the target, artifact and workload. Follow
[exclusive radio ownership](../architecture/wifi-ble-radio-exclusivity.md) and the
[diagnostic protocol](ble-diagnostic-protocol.md). Runnable procedures live in
[network validation](../guides/network/validation.md); unfinished acceptance work
lives in the [radio plan](../plans/ble/radio-policy-and-qualification.md).

## Inkplate handoff evidence — 2026-10-01

On 2026-10-01, release/default build `qualification-20261001-07` passed all
20 Wi-Fi/BLE exclusive cycles on one boot, including uploads before and after
each BLE window. Both cumulative UART-drop intervals remained zero, the exact
original telemetry mask was restored, and lifecycle/memory gates passed.
The profile was explicitly `minimal`; this is not zero-drop proof for verbose
all-domain logging. Wi-Fi regression passed discovery and one/three-cycle
upload acceptance; its optional soak was skipped.

ELF SHA-256: `12e7c85b06621bb223ac79b744c50ea2d5722c616015205cf00ce9e7ac72e4c4`.
Application SHA-256: `bff85a63f52b6e6a2e96321cd4e0b4f476251da1a720e3d79b20b8599d2edb1c`.
Artifact metadata records `source_dirty=true`. Safe reports are retained at
`logs/qualification_20261001_validation/ble-07-result-safe.json` and
`logs/qualification_20261001_validation/wifi-07-result-safe.json`.

This closes the bounded Inkplate handoff repair and qualification effort; it does
not establish GATT interoperability, power compliance, reset/sleep behavior or
wider workload/soak acceptance. Waveshare was compile-checked, not physically
qualified. The intermittent RTC failure remains a separate
[known issue](../../KNOWN_ISSUES.md#intermittent-rtc-time-sync-queuetimeout).

## Inkplate power qualification

These criteria were recorded on 2026-08-25 for the Inkplate foundation. Confirm
their applicability to the supported configuration before qualification. They are
not measured results or Waveshare limits.

| State | Maximum average | Maximum peak |
| --- | ---: | ---: |
| BLE linked, controller off | 2 mA | 10 mA |
| BLE-default non-advertising idle | 70 mA | 250 mA |
| Advertising | 70 mA | 250 mA |
| Connected idle | 70 mA | 250 mA |
| Rate-limited Echo | 90 mA | 300 mA |

For linked/controller-off delta, compare paired builds from the same source with
and without BLE. Other states use the same artifact's immediately preceding
default-owner baseline.

The advertising ceiling assumes 250 ms intervals and 0 dBm transmit power. The
current service does not explicitly set transmit power; verify it for the run.
BLE-default idle depends on the
[default-owner work](../plans/ble/radio-policy-and-qualification.md#configurable-default-owner).

A 120-second visibility window adds at most 50 J against the baseline. Other
window lengths require an accepted energy ceiling and measurement.

After default-owner restoration, current must return within 2 mA of its pre-window
baseline within two seconds and remain there for ten seconds.

- Use a calibrated fixed-range analyzer sampling voltage and current at least
  1,000 times per second. Record uncertainty and actual radio configuration.
- Measure three runs per supported state. Retain raw samples, artifact identity,
  average/peak deltas, window energy and return to baseline. Record display, touch
  and SD activity so another run can reproduce the workload.
- Compare the worst run, accounting for instrument uncertainty, with these limits.
  Unsupported states remain unqualified; a missing measurement is not a pass.
