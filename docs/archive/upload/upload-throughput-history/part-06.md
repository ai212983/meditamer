## 2026-03-04: Wi-Fi RSSI context added to ingress diagnostics

Implementation:

- connected watchdog now samples STA RSSI (`WifiController::rssi()`).
- telemetry now emits:
  - `METRICS WIFI_LINK rssi_last_dbm=... rssi_min_dbm=... rssi_max_dbm=... rssi_samples=... rssi_low_samples=...`
- upload request stats now include Wi-Fi RSSI context:
  - `wifi_rssi_last_dbm`
  - `wifi_rssi_min_dbm`
  - `wifi_rssi_max_dbm`
  - `wifi_rssi_samples`
  - `wifi_rssi_low_samples`

Validation runs:

- guarded post-flash acceptance attempt:
  - `logs/wifi_acceptance_ingress_rssi_direct3_20260304_082811.log`
  - failed at boot discovery gate (`ready=false`, zero discovery evidence)
- discovery recovery proof:
  - `logs/wifi_discovery_rssi_recover_20260304_083129.log`
  - summary: `ready_rounds=8`, `zero_discovery_rounds=0`,
    `total_scan_nonzero_events=1`
- bounded direct correlation run:
  - `logs/wifi_acceptance_ingress_rssi_direct10_20260304_083419.log`
  - `logs/wifi_acceptance_ingress_rssi_direct10_20260304_083419.log.hostdiag`

Direct 10-cycle aggregate (`n=10`):

- request timing:
  - `read_wait_ms avg=2532.8`
  - `req_ms avg=3227.5`
  - `ingress_read_wait_empty_q_ms avg=2528.2`
  - `ingress_read_wait_nonempty_q_ms avg=4.6`
- Wi-Fi RSSI context:
  - `wifi_rssi_last_dbm avg=-62.5` (range `-68..-59`)
  - `wifi_rssi_min_dbm avg=-71.0`
  - `wifi_rssi_low_samples avg=1.0`
- host/firmware coupling:
  - `host retry_count=10`, `avg_attempts=2.00`
  - `corr(send_ms, read_wait_ms)=0.991`
  - `corr(rssi_last, read_wait_ms)=0.056`

Interpretation:

- ingress wait remains empty-queue dominated (~`99.8%` of read-wait).
- within this RSSI band, signal strength variation was not the primary driver of
  request-time variance.
- dominant coupling remains host send cadence and first-attempt transport-reset
  retry behavior.

## 2026-03-04: host retry cause-chain diagnostics + pre-PUT pacing A/B

Host tooling changes:

- expanded `host_upload_retry_diag` with typed reqwest/IO flags and compact
  full cause chain (`err_chain`).
- added `HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS` (`0` default), applied before each
  direct `PUT /upload` attempt.
- added host failure class `host_transport_connect_refused` for better
  post-run classification.

Runs (`HOSTCTL_UPLOAD_MODE=direct`, `HOSTCTL_UPLOAD_SEND_DIAG=1`, cycles=5):

- baseline (`HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=0`):
  - `logs/wifi_acceptance_preputdelay_off_direct5_20260304_084754.log`
  - `logs/wifi_acceptance_preputdelay_off_direct5_20260304_084754.log.hostdiag`
  - summary: `avg_upload_s=6.45`, `avg_kib_s=79.72`
- variant (`HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=120`):
  - `logs/wifi_acceptance_preputdelay_on120_direct5_20260304_084843.log`
  - `logs/wifi_acceptance_preputdelay_on120_direct5_20260304_084843.log.hostdiag`
  - summary: `avg_upload_s=5.11`, `avg_kib_s=103.96`

Hostdiag aggregate:

- baseline:
  - retries: `5/5`
  - `avg_attempts=2.00`
  - repeated first-attempt chain:
    `client error (Connect) <- tcp connect error <- Connection refused (os error 61)`
- variant:
  - retries: `0/5`
  - `avg_attempts=1.00`
  - no first-attempt retry lines observed.

Firmware upload-stats aggregate (`n=5`, last five upload requests per run):

- baseline:
  - `read_wait_ms avg=2546.8`
  - `req_ms avg=3224.6`
  - `ingress_read_wait_empty_q_ms avg=2541.4`
  - `ingress_read_wait_nonempty_q_ms avg=5.4`
- variant:
  - `read_wait_ms avg=2602.8`
  - `req_ms avg=3285.8`
  - `ingress_read_wait_empty_q_ms avg=2598.2`
  - `ingress_read_wait_nonempty_q_ms avg=4.6`

Interpretation:

- primary retry signature is now explicit and stable: first-attempt connect
  refused on direct `PUT /upload`.
- short host pre-PUT delay suppresses this class in bounded A/B and improves
  end-to-end throughput by avoiding retries.
- per-success ingress wait remains empty-queue dominated; next optimization
  target stays listener/accept readiness around the `mkdir -> upload` boundary.

## 2026-03-04: `NET_ACCEPT` microsecond gap evidence + firmware keep-alive fix candidate

Instrumentation change:

- upgraded listener re-arm telemetry to microseconds:
  - `METRICS NET_ACCEPT arm_gap_n arm_gap_us arm_gap_us_max arm_gap_after_mkdir_n arm_gap_after_mkdir_us arm_gap_after_mkdir_us_max`

Validation run (pre-fix evidence, direct mode):

- `logs/wifi_acceptance_acceptarmgapus_direct3b_20260304_090920.log`
- `logs/wifi_acceptance_acceptarmgapus_direct3b_20260304_090920.log.hostdiag`
- host summary:
  - `avg_upload_s=5.72`, `avg_kib_s=90.68`
  - retries: `3/3` (`avg_attempts=2.00`)
  - each first-attempt failure chain includes:
    `client error (Connect) <- tcp connect error <- Connection refused (os error 61)`
- firmware (last `METRICS NET_ACCEPT` line in run):
  - `arm_gap_n=22`
  - `arm_gap_us=4832` (aggregate)
  - `arm_gap_us_max=233`
  - `arm_gap_after_mkdir_n=3`
  - `arm_gap_after_mkdir_us=672` (aggregate)
  - `arm_gap_after_mkdir_us_max=226`

Interpretation:

- the observed listener re-arm window is sub-millisecond and still sufficient to
  race immediate reconnects in this direct path.
- this supports a protocol/connection-lifecycle fix over coarse delay-only
  mitigation.

Firmware fix candidate implemented (awaiting live validation):

- switched HTTP responses to `HTTP/1.1` keep-alive semantics (removed forced
  `Connection: close` behavior).
- updated listener socket cycle to serve multiple requests per accepted socket.
- added keep-alive idle guard (`500 ms` header timeout after first request) to
  avoid one idle peer blocking accepts.

Validation status:

- on-device validation blocked after flash attempt failed with serial transport
  loss (`Broken pipe`, then `Serial port not found`); USB serial node was absent
  on host (`/dev/cu.*`).
- next action is to restore serial visibility and rerun bounded direct acceptance
  at `HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=0`.

## 2026-03-04: keep-alive validation + host cross-cycle client reuse

Validation context:

- serial recovered on USB and firmware was flashed/validated in direct mode.
- acceptance runs used `HOSTCTL_UPLOAD_MODE=direct`,
  `HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=0`, `HOSTCTL_UPLOAD_SEND_DIAG=1`.
- acceptance profile currently pins `HOSTCTL_NET_LOG_PATH=acceptance_3_cycle`;
  evidence below is from appended run segments in that stream.

Firmware keep-alive validation:

- keep-alive ON (cycles=3): `avg_upload_s=3.62`, `avg_kib_s=141.86`
- forced close (cycles=3): `avg_upload_s=3.40`, `avg_kib_s=150.67`
- keep-alive ON repeat (cycles=3): `avg_upload_s=3.53`, `avg_kib_s=145.39`
- matched warmed pair (cycles=3):
  - forced close: `avg_kib_s=149.52`
  - keep-alive ON: `avg_kib_s=147.06`
- matched warmed pair (cycles=6):
  - keep-alive ON: `avg_upload_s=3.45`, `avg_kib_s=148.58`
  - forced close: `avg_upload_s=3.45`, `avg_kib_s=148.30`

Interpretation:

- keep-alive behavior is stable in bounded acceptance.
- throughput delta is small and inconsistent; practical outcome is near parity.

Host cross-cycle client reuse experiment:

- host tooling now supports reusable client path for wifi-acceptance:
  - `HOSTCTL_NET_REUSE_UPLOAD_CLIENT=1` (default `0`).
- strict-guard 6-cycle reuse run (`max_delta=0`) hit one first-attempt timeout:
  - `HOST_FAILURE class=host_transport_send_fail` on cycle 3,
  - retry recovered upload,
  - guard failed on `req_read_body_reset delta=1`.
- reuse run with relaxed guard (`max_delta=2`) completed:
  - keep-alive ON: `avg_upload_s=3.64`, `avg_kib_s=142.61`
  - forced close: `avg_upload_s=3.50`, `avg_kib_s=146.36`
- default mode (reuse off) sanity run (cycles=3) remained stable:
  - `avg_upload_s=3.49`, `avg_kib_s=146.90`

Decision:

- do not promote host cross-cycle client reuse as a default throughput
  optimization.
- retain it as opt-in for diagnostics while primary root-cause focus remains
  firmware ingress empty-queue `read_wait_ms`.

