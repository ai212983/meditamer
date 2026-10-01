# Observation validation

Run on an identified image supporting the selected fixture, with one serial owner
and the correct `$DEVICE_PORT`. These workflows retain UART and JSON evidence;
requests are correlated and never retried. Hardware sleep/panel coexistence and
runtime memory peaks require their own qualification.

## One-shot samples

```bash
HOSTCTL_PORT="$DEVICE_PORT" scripts/hostctl.sh test observation-fixture \
  --provider battery --output logs/<new-battery-run>.log
```

| Target | Provider | Result units |
| --- | --- | --- |
| Inkplate | `battery` (default) | percent |
| Inkplate | `bme688` | centidegrees, millipercent humidity |
| Medinote/S3 | `shtc3` | corrected millicelsius, millipercent humidity |
| Medinote/S3 | `adc` | millivolts, percent |

Provider IDs are product-local; identify board and kind/units, not ID alone.
The workflow checks PING, waits for RUNTIME_READY if attach resets the board,
and confirms PING without a fixed startup sleep. A pass requires the exact-ID
RESULT, a newer successful sample identity and completion within eligibility;
QUEUED alone is insufficient. No navigation is required.

`--request-id` overrides the generated host-nanosecond ID. IDs must be nonzero
and strictly increase within a boot, including busy-rejected attempts.
`--validity-ms` is 1–300000 (default 300000); `--timeout-ms` defaults to 300000
per wait. These are operator limits, not predicted sensor conversion times.
Disconnect/timeout leaves the one-shot to expire; a started conversion can finish
into cache, and UI stalls can delay terminal output.

`--observe-ms` keeps the same connection after success: use 360000 for Inkplate's
five-minute cadence or 150000 for Medinote's one-minute cadence. Inspect newer
acquisition identities/times; capture duration and cached delivery are not proof
of periodic acquisition. The report distinguishes sample success from capture.

## Periodic demand and restoration

```bash
HOSTCTL_PORT="$DEVICE_PORT" scripts/hostctl.sh test observation-fixture \
  --provider adc --period-ms 90000 --validity-ms 240000 --timeout-ms 270000 \
  --cancel-after-samples 2 --observe-ms 75000 --output logs/<new-periodic-run>.log
```

Supported for all four providers. Intervals are 60000–300000 ms; validity is at
most 900000 ms and must exceed interval. Allow enough time for two acquisitions
and restoration. Without `--cancel-after-samples`, the host waits for expiry;
with it (2–15), it sends one cancellation using the original session ID.

Require APPLIED, at least two increasing successful identities separated by the
requested interval, and expected Restored/Cancelled with live-demand metadata.
Closed is a failed periodic run with cleanup, not an expiry/sample pass.
The provider applies overrides between acquisitions and restores the latest live
demand on expiry, cancellation, suspension or stop. A current conversion may
finish first. One-shot/periodic fixtures share one slot per provider; a closed
provider rejects new fixtures. These checks do not qualify physical faults.

## Inkplate panel interruption

```bash
HOSTCTL_PORT="$DEVICE_PORT" scripts/hostctl.sh test observation-fixture \
  --provider battery --panel-cycle --period-ms 60000 --validity-ms 150000 \
  --timeout-ms 420000 --output logs/<new-panel-run>.log
```

Repeat for BME688 with new output. Medinote and `--cancel-after-samples` are
rejected. Request IDs must be at least 2; the preceding ID is the preflight repaint.

The workflow completes a correlated repaint, submits the periodic/repaint
request, and requires provider application before the UI owner repaints.
Queue time counts against validity. PANEL_FIXTURE END status=Completed follows
scan and resume waits; QUEUED/OK alone is insufficient.

Require this cycle's suspend/resume identities and acknowledgements, fixture
closure during that repaint, no pending override, retained live demand, and a
newer successful sample afterward. A fixture closed before this repaint or a
cached delivery fails. No retry, navigation, or observe-now request is sent.
Rejected preflight sends no fixture. The example allows the five-minute live
cadence plus slack; it does not promise firmware latency. Current full refresh
is permitted with upload enabled, subject to panel safety; use
[upload observation](#inkplate-acquisition-during-upload) for separate overlap evidence.

## Medinote sleep and synthetic cleanup rejection

Leave physical buttons untouched and use the identified native USB port:

```bash
HOSTCTL_PORT="$DEVICE_PORT" scripts/hostctl.sh test observation-sleep \
  --mode reject-environment --output logs/<new-sleep-run>
```

Repeat `reject-battery` and `deep`, each with a new output directory. Validity is
1–300000 ms (default 30000); timeout defaults to 90000 ms per wait.
The UI owner navigates Home and invokes normal sleep. IDs share the strictly
increasing observation-command watermark.

Rejection performs real cleanup, then injects CleanupFailed for the selected
provider/request. It stays suspended until resume; the next request is normal.
This does not simulate physical I²C/ADC faults or broken driver cleanup.

Deep mode requires both cleanups, actual entry, one CoreDeepSleep/Timer reset,
and fresh samples from both providers after reconnecting the same USB port.
Rejection requires the injected acknowledgement, newer resume IDs, no sleep/reset,
and newer successful samples from both existing generations. Wrong/cached IDs,
button input, or acquisition failures fail qualification. It covers deep sleep
and synthetic rejection, not light-sleep retention or loaded BLE/upload operation.

For periodic-fixture closure during rejection:

```bash
HOSTCTL_PORT="$DEVICE_PORT" scripts/hostctl.sh test observation-sleep \
  --mode reject-environment --period-ms 180000 --periodic-validity-ms 600000 \
  --timeout-ms 240000 --output logs/<new-periodic-sleep-run>
```

Repeat reject-battery. Deep mode is refused. Sleep ID must exceed 2; the two
preceding IDs arm providers. Interval must exceed Home's 60000 ms and be at most
300000; validity must exceed interval and be at most 900000. Both exact APPLIED
responses precede sleep. If preparation fails, no sleep is sent; armed leases expire.

Require both leases Closed during this sleep's BEGIN/cleanup interval, Home demand
withdrawn, then no override and restored one-minute demand after rejection.
Require another acquisition at least one live interval after the fresh entry
sample, before the former override interval and expiry; compare acquisition
timestamps rather than log arrival. No host refresh/cancel is sent after rejection.

## Inkplate acquisition during upload

After [network validation](../network/validation.md) leaves the listener ready:

```bash
HOSTCTL_PORT="$DEVICE_PORT" scripts/hostctl.sh test observation-upload \
  --firmware-elf logs/<recorded-flash>/firmware.elf --output logs/<new-upload-observation>
```

One passive connection paces a 64-byte HTTP PUT. After SD Begin, both providers
must return fresh correlated samples before final body bytes, followed by HTTP
completion and exact SD stat/readback. Another upload changing Begin invalidates
capture. Eligibility is 30 seconds; body lifetime is bounded to 60 seconds with
one byte/second maintaining the six-second idle timeout. Failure closes the
socket; SD logging is restored; the probe file remains. ELF hashing associates
recorded flash evidence, not device readback. This proves acquisition during body
ingestion, not sustained SD-write throughput.
