# Known issues

## Active-input timing variability

During touch and screen navigation, some touch reads take 16–19 ms despite an
8 ms active polling schedule, and IMU sampling gaps can reach 17–19 ms. No
visible delay, missed gesture, or degraded IMU data has been established.

## Intermittent RTC time-sync QueueTimeout

RTC synchronization has failed at `invalidate_marker`, register `0x03`, address
`0x51`, with `QueueTimeout`. The request was not claimed; this does not establish
an electrical I2C failure. The root cause remains unresolved.

The default Inkplate build now emits bounded
[proxy/owner failure metadata](docs/references/runtime/metrics.md#persistence-and-rtc-diagnostics).
Automatic sync and five additional boot-sync probes on instrumented image 06,
and automatic sync on image 07, passed without reproducing the failure.
The retained investigation report is `logs/qualification_20261001_i2c_report.json`.

On recurrence, retain `I2C_PROXY_TIMEOUT`, `I2C_PROXY_OWNER`, `RTC_DIAG`,
`I2C_TIMEOUT`/`I2C_ADMISSION_ERROR` and `TIME_SYNC` records with the build identity
and boot timing. Use the recorded phase, owner age and physical-lock holder to
investigate where progress stopped; correlate proxy records by request ID.
Metadata lines may drop independently, and owner/physical-holder observations
are separate samples. Do not attribute a task or change timeouts/retries without
supporting evidence. Close this issue after reproducing and correcting the cause,
with a targeted regression check; successful probes alone do not establish a fix.
