## 2026-03-04: firmware ingress empty-queue mitigation via cooperative read fairness

Firmware change:

- in `src/firmware/storage/upload/http/connection/body.rs`, added periodic
  cooperative yield while draining immediately-ready socket reads:
  - initial bounded-run thresholds: `12 KiB` / `24 reads`
  - later promoted to configurable firmware defaults via
    `HTTP_INGRESS_COOP_YIELD_{BYTES,READS}`.
- intent: give the net runner scheduling opportunities during long ready-read
  bursts so RX queue refill cadence improves and empty-queue stalls shrink.

Bounded validation (`direct`, `HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=0`, cycles=3):

- baseline (pre-change):
  - summary: `avg_upload_s=3.53`, `avg_kib_s=145.27`
  - last three `upload_http: upload stats` `read_wait_ms`: `2368`, `2389`, `2493`
  - last three `ingress_read_wait_empty_q_ms`: `2362`, `2383`, `2483`
- post-change run A:
  - summary: `avg_upload_s=3.43`, `avg_kib_s=149.33`
  - `read_wait_ms`: `2304`, `2295`, `2293`
  - `ingress_read_wait_empty_q_ms`: `2302`, `2291`, `2285`
- post-change run B (immediate confirmation):
  - summary: `avg_upload_s=3.42`, `avg_kib_s=149.57`
  - `read_wait_ms`: `2280`, `2397`, `2255`
  - `ingress_read_wait_empty_q_ms`: `2276`, `2391`, `2247`

Observed bounded deltas:

- throughput improved by about `+2.8..+3.0%` in the two post-change runs.
- compared sample-average `read_wait_ms` moved:
  - pre: `2416.7`
  - post: `2304.0` (`-4.7%`)

Interpretation:

- cooperative fairness in the firmware read loop appears to reduce empty-queue
  ingress wait in short bounded runs, with corresponding throughput lift.
- next step is longer-cycle and soak confirmation to separate persistent effect
  from RF variance.

## 2026-03-04: ingress fairness threshold tuning and default promotion

Tuning setup:

- threshold knobs made build-time configurable:
  - `MEDITAMER_HTTP_INGRESS_COOP_YIELD_BYTES` / fallback
    `HTTP_INGRESS_COOP_YIELD_BYTES`
  - `MEDITAMER_HTTP_INGRESS_COOP_YIELD_READS` / fallback
    `HTTP_INGRESS_COOP_YIELD_READS`
- mode: direct upload (`HOSTCTL_UPLOAD_MODE=direct`)

Bounded matrix (cycles=6):

- `4096/16`: `avg_kib_s=148.05`, `stddev=3.24`
- `6144/20`: `avg_kib_s=147.81`, `stddev=4.01`
- `8192/24`: `avg_kib_s=149.05`, `stddev=3.51`

Extended confirmation (cycles=10):

- `4096/16`: `avg_kib_s=147.78`, `stddev=4.09`
- `8192/24`: `avg_kib_s=149.67`, `stddev=2.30`

Decision:

- promoted firmware defaults to `8192/24` in `src/firmware/types/base.rs`:
  - `HTTP_INGRESS_COOP_YIELD_BYTES_DEFAULT = 8 * 1024`
  - `HTTP_INGRESS_COOP_YIELD_READS_DEFAULT = 24`
- reason: in longer A/B, `8192/24` produced both higher throughput and lower
  variance than `4096/16`.

## 2026-03-04: bounded soak validation for promoted ingress fairness defaults

Artifact:

- `logs/wifi_acceptance_ingressfairness_soak10_20260304_113208.log`

Run profile:

- direct upload mode (`HOSTCTL_UPLOAD_MODE=direct`)
- `HOSTCTL_NET_CYCLES=10`
- `HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE=0`
- `HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=0`
- promoted defaults under test:
  - `HTTP_INGRESS_COOP_YIELD_BYTES_DEFAULT = 8 * 1024`
  - `HTTP_INGRESS_COOP_YIELD_READS_DEFAULT = 24`

Observed:

- summary: `avg_upload_s=3.47`, `avg_kib_s=147.71`, `total_s=68.75`
- per-cycle throughput (`n=10`): mean/stddev `147.72 ± 3.89 KiB/s`
  (min/max `136.57 / 150.59`)
- warmed-cycle view (`cycles 2..10`): `148.95 ± 1.21 KiB/s`
- failure class / guard signal:
  - no `HOST_FAILURE` markers
  - no listener-not-ready or host health/send-failure markers
  - `req_read_body_reset delta=0` across cycles

Interpretation:

- promoted ingress fairness defaults stay stable through bounded soak with no
  new host/device failure-class regressions.
- the known first-cycle startup/listener outlier remains visible, while
  steady-state cycle variance is low.

## 2026-03-04: listener/dhcp readiness instability fix and gate validation

Pre-fix failure evidence:

- full gate run `logs/wifi_regression_gate_ingressfairness_default_20260304_113541`
  failed at `acceptance_1_cycle` before soak.
- serial trace showed repeated `ListenerWait` loops and recurring
  `listener_timeout` recovery despite active link.

Root cause:

- listener readiness timeout was measured from `dhcp_wait_started_at` (session
  connect-entry timestamp), not from the start of the current listener wait.
- after long-lived connected sessions, a temporary listener-off window could
  immediately exceed timeout budget and force reconnect churn.

Fix:

- `src/firmware/storage/upload/wifi/connect/success.rs`
- `src/firmware/storage/upload/wifi/connect/success/success_progress.rs`
- added `listener_wait_started_at` and switched listener timeout accounting to
  this dedicated wait-window baseline.
- reset baseline when listener is disabled, when listener becomes ready, or
  when lease is absent.

Validation:

- focused check:
  - `logs/wifi_acceptance_listenerfix_1cycle_20260304_114536.log`
  - pass; no repeated listener-timeout churn signatures.
- full regression gate with soak:
  - `logs/wifi_regression_gate_listenerfix_20260304_114631/report.json`
  - `discovery_debug`: passed (`110224 ms`)
  - `acceptance_1_cycle`: passed (`7049 ms`)
  - `acceptance_3_cycle`: passed (`52818 ms`)
  - `acceptance_soak` (10 cycles): passed (`54907 ms`)
  - soak summary: `avg_upload_s=3.43`, `avg_kib_s=149.35`

Conclusion:

- stale listener-timeout baseline was the primary trigger for the pre-soak
  listener/DHCP readiness instability in this campaign.
- with per-wait listener timeout baseline, the full gate shape is stable again.

## 2026-03-04: pinned unstable `Ready`-but-unreachable state after burst run

Failure signature (burst run):

- host side: `HOST_FAILURE class=host_transport_send_fail` on `PUT /upload`
  followed by repeated `HOST_FAILURE class=host_health_send_fail` while
  `NET_STATUS` still reported:
  - `state=Ready`
  - `listener=true`
  - `failure_class=none`
- serial trace around failure edge:
  - `upload_http: accepted connection`
  - `upload_http: request method=PUT path=/upload`
  - `sd_upload: begin ...`
  - then no `upload_http: upload stats`, no `request_ok`, and no immediate
    `request_err` before host health probes started failing.

Pinned root cause:

- the upload listener serves requests on a single socket loop; once it enters
  direct `/upload` body ingest, health requests cannot be served until that
  route returns.
- upload body reads were governed by the global socket timeout
  (`HTTP_SOCKET_TIMEOUT_SECS=60`), so a half-open/stalled sender could keep the
  route blocked long enough for host health/send failures even though radio
  state remained `Ready`.

Mitigation implemented:

- added dedicated upload-body idle timeout (`6_000 ms`) and applied it only
  around body forwarding:
  - `src/firmware/storage/upload/http/connection.rs`
    - `HTTP_UPLOAD_BODY_READ_TIMEOUT_MS = 6_000`
  - `src/firmware/storage/upload/http/connection/routes.rs`
    - `handle_upload`
    - `handle_upload_chunk`
    - set body-read timeout before `forward_upload_body_or_http_error(...)`
      and restore standard socket timeout afterward.

Expected effect:

- stalled body ingress exits as `read body` error in seconds (not up to `60s`),
  request is aborted/closed, and listener returns to accept loop quickly enough
  to keep `/health` reachable.

Post-flash validation:

- flashed debug firmware with the timeout guard (`scripts/device/flash.sh debug`)
  to `/dev/cu.usbserial-510`.
- bounded burst validation:
  - run id: `postflash_burst10_timeoutguard_20260304_124337`
  - summary: `cycles=10`, `avg_kib_s=104.89`, `stddev=8.72`
  - serial averages: `read_wait_ms=2302.4`,
    `ingress_read_wait_empty_q_ms=2297.0`
  - no `HOST_FAILURE` markers and no `host_health_send_fail` recurrence.

Interpretation:

- the prior collapse pattern (transport send failure followed by `/health`
  unreachability while `NET_STATUS` stayed `Ready`) did not reproduce in the
  post-flash burst stress run.

