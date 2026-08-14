## 11.22 2026-03-04 Wi-Fi RSSI Context for Ingress Correlation

Changes:

- added connected-watchdog RSSI sampling via `WifiController::rssi()`.
- added `METRICS WIFI_LINK` line:
  - `rssi_last_dbm`, `rssi_min_dbm`, `rssi_max_dbm`, `rssi_samples`,
    `rssi_low_samples`
- added per-upload RSSI context fields to `upload_http: upload stats`:
  - `wifi_rssi_last_dbm`, `wifi_rssi_min_dbm`, `wifi_rssi_max_dbm`,
    `wifi_rssi_samples`, `wifi_rssi_low_samples`

Validation sequence:

- post-flash acceptance attempt hit boot discovery gate timeout (expected guard):
  - `logs/wifi_acceptance_ingress_rssi_direct3_20260304_082811.log`
- recovery proof:
  - `logs/wifi_discovery_rssi_recover_20260304_083129.log`
  - summary: `ready_rounds=8`, `zero_discovery_rounds=0`,
    `total_scan_nonzero_events=1`
- bounded direct sample:
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
- correlation checks:
  - `corr(rssi_last, read_wait_ms)=0.056` (weak in this sample band)
  - `corr(send_ms, read_wait_ms)=0.991` (strong)

Interpretation:

- ingress wait remains overwhelmingly empty-queue dominated.
- within observed RSSI band, link signal variation does not explain read-wait
  variance as strongly as host send pacing/transport behavior.

Specific next step:

- keep the RSSI context instrumentation.
- focus root-cause on direct-path transport cadence and first-attempt
  `transport_reset` behavior, with AP/radio factors treated as secondary unless
  wider RSSI variance appears.

## 11.23 2026-03-04 Host Retry Cause-Chain + Pre-PUT Pacing A/B

Changes:

- expanded `host_upload_retry_diag` with:
  - typed reqwest flags (`reqwest_*`)
  - typed IO flags (`io_*`)
  - compact full error chain (`err_chain=...`)
- added host knob:
  - `HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS` (`0` default), applied before each direct
    `PUT /upload` attempt.
- added host failure-class refinement:
  - `host_transport_connect_refused` (distinguishes connect-refused from generic
    send failure).

Runs (`HOSTCTL_UPLOAD_MODE=direct`, `HOSTCTL_UPLOAD_SEND_DIAG=1`, cycles=5):

- baseline (`HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=0`):
  - `logs/wifi_acceptance_preputdelay_off_direct5_20260304_084754.log`
  - `logs/wifi_acceptance_preputdelay_off_direct5_20260304_084754.log.hostdiag`
  - summary: `avg_upload_s=6.45`, `avg_kib_s=79.72`
- variant (`HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=120`):
  - `logs/wifi_acceptance_preputdelay_on120_direct5_20260304_084843.log`
  - `logs/wifi_acceptance_preputdelay_on120_direct5_20260304_084843.log.hostdiag`
  - summary: `avg_upload_s=5.11`, `avg_kib_s=103.96`

Host diagnostics delta:

- baseline:
  - `host_retry_count=5/5`
  - `avg_attempts=2.00`
  - repeated first-attempt chain:
    `client error (Connect) <- tcp connect error <- Connection refused (os error 61)`
- variant (`120 ms` pre-PUT delay):
  - `host_retry_count=0/5`
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

- dominant first-attempt failure signature is now explicit: connect-refused on
  direct `PUT /upload` before body transfer.
- a short bounded host pre-PUT delay suppresses that failure class in this
  sample and improves end-to-end throughput by removing retry overhead.
- core ingress bottleneck remains empty-queue read wait; pacing does not reduce
  per-success request `read_wait_ms` materially.

Specific next root-cause target:

- instrument and isolate firmware-side listener availability around the
  `mkdir -> upload` transition (accept-loop readiness window), then validate
  whether a firmware-side fix can remove connect-refused without host delay.

## 11.24 2026-03-04 `NET_ACCEPT` Microsecond Gap Evidence + Keep-Alive Fix (Validated)

Completed changes:

- upgraded accept-arm telemetry to microsecond granularity:
  - `METRICS NET_ACCEPT arm_gap_n arm_gap_us arm_gap_us_max ...`
- implemented firmware keep-alive/multi-request socket handling:
  - response helper uses `HTTP/1.1` + `Connection: keep-alive`
  - socket cycle serves multiple requests per accepted socket
  - short keep-alive idle guard (`500 ms`) prevents idle monopolization.

Bounded direct validation (`HOSTCTL_UPLOAD_MODE=direct`, `HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=0`):

- keep-alive ON (cycles=3):
  - summary: `avg_upload_s=3.62`, `avg_kib_s=141.86`
- forced close (cycles=3):
  - summary: `avg_upload_s=3.40`, `avg_kib_s=150.67`
- keep-alive ON repeat (cycles=3):
  - summary: `avg_upload_s=3.53`, `avg_kib_s=145.39`
- matched warmed pair (cycles=3):
  - forced close: `avg_kib_s=149.52`
  - keep-alive ON: `avg_kib_s=147.06`
- matched warmed pair (cycles=6):
  - keep-alive ON: `avg_upload_s=3.45`, `avg_kib_s=148.58`
  - forced close: `avg_upload_s=3.45`, `avg_kib_s=148.30`

Interpretation:

- keep-alive fix is runtime-stable in bounded acceptance (no persistent
  connect-refused class observed in final paired runs).
- throughput impact is small and inconsistent across short runs; current signal
  indicates parity rather than a clear gain.

## 11.25 2026-03-04 Host Cross-Cycle Upload-Client Reuse (Bounded Result)

Host changes:

- added reusable direct-upload client APIs:
  - `make_direct_upload_client`
  - `upload_file_direct_fast_with_client`
- wired wifi-acceptance to optionally reuse one client across cycles via:
  - `HOSTCTL_NET_REUSE_UPLOAD_CLIENT=1`
  - default remains `0` (off) to avoid promoting a non-winning path.
- pooled client is dropped on upload failure or recovery path.

Bounded evidence:

- reuse-enabled 6-cycle run (strict reset guard, `max_delta=0`) hit one
  first-attempt send timeout in cycle 3:
  - `HOST_FAILURE class=host_transport_send_fail`
  - retry recovered upload, but guard failed on `req_read_body_reset delta=1`.
- reuse-enabled 6-cycle run (relaxed guard, `max_delta=2`) completed:
  - keep-alive ON: `avg_upload_s=3.64`, `avg_kib_s=142.61` (one slow send outlier)
  - forced close: `avg_upload_s=3.50`, `avg_kib_s=146.36`
- default mode (reuse off) sanity run (cycles=3) remained stable:
  - `avg_upload_s=3.49`, `avg_kib_s=146.90`

Decision:

- do not promote host cross-cycle client reuse as a throughput optimization.
- keep it as an opt-in diagnostic/experiment knob while primary optimization
  focus returns to firmware ingress empty-queue `read_wait_ms`.

## 11.26 2026-03-04 Firmware Ingress Empty-Queue Mitigation (Cooperative Fairness Yield)

Firmware change:

- added cooperative fairness yield in upload body read loop:
  - file: `src/firmware/storage/upload/http/connection/body.rs`
  - behavior: while draining immediately-ready socket reads, yield periodically
    (`HTTP_INGRESS_COOP_YIELD_BYTES` or `HTTP_INGRESS_COOP_YIELD_READS`;
    initial bounded run used `12 KiB` / `24`)
    so the net runner can execute and refill RX queue.
  - rationale: reduce starvation bursts in cooperative scheduling where
    back-to-back ready reads can delay network runner progress and amplify
    empty-queue read wait.

Validation runs (`HOSTCTL_UPLOAD_MODE=direct`, `HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=0`, cycles=3):

- baseline (pre-change):
  - summary: `avg_upload_s=3.53`, `avg_kib_s=145.27`
  - last three upload stats `read_wait_ms`: `2368`, `2389`, `2493`
  - last three `ingress_read_wait_empty_q_ms`: `2362`, `2383`, `2483`
- post-change run A:
  - summary: `avg_upload_s=3.43`, `avg_kib_s=149.33`
  - `read_wait_ms`: `2304`, `2295`, `2293`
  - `ingress_read_wait_empty_q_ms`: `2302`, `2291`, `2285`
- post-change run B (confirmation):
  - summary: `avg_upload_s=3.42`, `avg_kib_s=149.57`
  - `read_wait_ms`: `2280`, `2397`, `2255`
  - `ingress_read_wait_empty_q_ms`: `2276`, `2391`, `2247`

Observed bounded delta:

- throughput (`avg_kib_s`): `145.27 -> 149.33/149.57` (`+2.8..+3.0%`)
- `read_wait_ms` average over compared samples:
  - pre: `2416.7`
  - post (6 samples): `2304.0` (`-4.7%`)

Interpretation:

- this firmware-side scheduler fairness tweak is a promising mitigation for the
  empty-queue ingress bottleneck in bounded runs.
- effect size is moderate and should be confirmed in longer-cycle/soak runs
  before declaring stable promotion.

