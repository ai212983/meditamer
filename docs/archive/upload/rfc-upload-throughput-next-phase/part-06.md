## 11.27 2026-03-04 Ingress Fairness Threshold Tuning (`bytes`/`reads`)

Scope:

- converted ingress fairness thresholds to build-time tunables in
  `src/firmware/types/base.rs`:
  - `MEDITAMER_HTTP_INGRESS_COOP_YIELD_BYTES` (fallback
    `HTTP_INGRESS_COOP_YIELD_BYTES`)
  - `MEDITAMER_HTTP_INGRESS_COOP_YIELD_READS` (fallback
    `HTTP_INGRESS_COOP_YIELD_READS`)
- objective: optimize direct upload throughput while minimizing per-cycle
  variance under AP-dense network conditions.

Bounded matrix (cycles=6, direct mode):

- `4096/16`: `avg_kib_s=148.05`, `stddev=3.24`
- `6144/20`: `avg_kib_s=147.81`, `stddev=4.01`
- `8192/24`: `avg_kib_s=149.05`, `stddev=3.51`

Extended confirmation A/B (cycles=10, direct mode):

- `4096/16`: `avg_kib_s=147.78`, `stddev=4.09`
- `8192/24`: `avg_kib_s=149.67`, `stddev=2.30`

Decision:

- promote `8192/24` as new firmware default ingress fairness thresholds:
  - `HTTP_INGRESS_COOP_YIELD_BYTES_DEFAULT = 8 * 1024`
  - `HTTP_INGRESS_COOP_YIELD_READS_DEFAULT = 24`
- rationale: in the longer comparison run, `8192/24` improved throughput and
  also reduced variance versus `4096/16`.

## 11.28 2026-03-04 Bounded Soak Validation with Promoted Ingress Fairness Defaults

Run:

- artifact: `logs/wifi_acceptance_ingressfairness_soak10_20260304_113208.log`
- mode: direct upload (`HOSTCTL_UPLOAD_MODE=direct`)
- profile: `HOSTCTL_NET_CYCLES=10`,
  `HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE=0`,
  `HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=0`,
  `HOSTCTL_UPLOAD_SEND_DIAG=1`
- firmware defaults under test:
  - `HTTP_INGRESS_COOP_YIELD_BYTES_DEFAULT = 8 * 1024`
  - `HTTP_INGRESS_COOP_YIELD_READS_DEFAULT = 24`

Result:

- summary: `avg_upload_s=3.47`, `avg_kib_s=147.71`, `total_s=68.75`
- per-cycle throughput (`n=10`):
  - mean/stddev: `147.72 ± 3.89 KiB/s`
  - min/max: `136.57 / 150.59 KiB/s`
- warmed cycles only (`cycles 2..10`) to isolate the known first-cycle
  listener-ready outlier:
  - mean/stddev: `148.95 ± 1.21 KiB/s`
- reliability/failure-class:
  - no `HOST_FAILURE` markers
  - no listener-not-ready or host health/send-failure markers
  - upload reset guard remained stable (`req_read_body_reset delta=0`)

Interpretation:

- bounded soak confirms promoted ingress fairness defaults remain stable and do
  not introduce new failure classes.
- first-cycle startup/listener timing remains a separate known outlier source;
  steady-state upload cycles show low variance.

## 11.29 2026-03-04 Listener/DHCP Readiness Stability Fix (Stale Listener Timeout Baseline)

Observed failure mode:

- regression gate run `logs/wifi_regression_gate_ingressfairness_default_20260304_113541`
  failed in `acceptance_1_cycle` before soak.
- serial evidence showed repeated `ListenerWait` loops with eventual
  `listener_timeout` recovery (`reason=8` disconnect churn) despite link being
  active.

Root cause:

- listener readiness timeout used `dhcp_wait_started_at` (set once at connect
  success entry), so on long-lived connections any later listener-off period
  (for example after discovery/listener gate toggles) inherited a stale elapsed
  baseline and could trip `listener_timeout` almost immediately.

Firmware fix:

- files:
  - `src/firmware/storage/upload/wifi/connect/success.rs`
  - `src/firmware/storage/upload/wifi/connect/success/success_progress.rs`
- introduced dedicated `listener_wait_started_at` timer scoped to actual
  listener wait windows:
  - set when transitioning `DhcpWait -> ListenerWait` (`dhcp_ready`);
  - reset when listener is disabled, listener becomes ready, or lease is absent;
  - apply `listener_timeout_ms` against this timer instead of
    `dhcp_wait_started_at`.

Validation:

- focused acceptance check:
  - `logs/wifi_acceptance_listenerfix_1cycle_20260304_114536.log`
  - result: pass (`connect_ms=19636`, `upload_ms=3512`) without repeated
    `listener_timeout` churn markers in serial log.
- full regression gate:
  - `logs/wifi_regression_gate_listenerfix_20260304_114631/report.json`
  - stage results:
    - `discovery_debug`: passed (`110224 ms`)
    - `acceptance_1_cycle`: passed (`7049 ms`)
    - `acceptance_3_cycle`: passed (`52818 ms`)
    - `acceptance_soak` (`10 cycles`): passed (`54907 ms`)
  - soak summary: `avg_upload_s=3.43`, `avg_kib_s=149.35`

Outcome:

- listener/DHCP readiness instability observed pre-soak is no longer
  reproducible in the same gate shape after the timer-baseline fix.

## 11.30 2026-03-04 Ready-but-Unreachable Instability (Pinned Cause + Guard)

Observed failure shape during burst validation:

- host run progressed through multiple successful cycles, then failed on:
  - `HOST_FAILURE class=host_transport_send_fail` (`PUT /upload` send failure)
  - followed by repeated `HOST_FAILURE class=host_health_send_fail`
- concurrent `NET_STATUS` remained:
  - `state=Ready`, `listener=true`, `failure_class=none`
- serial edge markers showed request entry without request completion:
  - `upload_http: accepted connection`
  - `upload_http: request method=PUT path=/upload`
  - `sd_upload: begin ...`
  - then no immediate `upload_http: upload stats` / `request_ok` / `request_err`.

Pinned cause:

- while handling direct `/upload`, the single HTTP connection loop is occupied
  by body ingress and cannot serve `/health` in parallel.
- upload-body reads were bounded by the global socket timeout
  (`HTTP_SOCKET_TIMEOUT_SECS=60`), so half-open/stalled ingress could leave the
  route blocked long enough to produce host health/send failures despite radio
  readiness staying `Ready`.

Mitigation implemented:

- added dedicated upload-body idle timeout:
  - `HTTP_UPLOAD_BODY_READ_TIMEOUT_MS = 6_000`
- applied only during body forwarding in:
  - `src/firmware/storage/upload/http/connection/routes.rs`
    - `handle_upload`
    - `handle_upload_chunk`
- route now restores standard socket timeout after body-forward call returns.

Expected behavior after flash:

- stalled body ingest should fail fast (`read body`) in seconds rather than up
  to the full 60-second socket timeout window.
- listener should return to accept loop quickly enough to preserve `/health`
  reachability and avoid cascading `host_health_send_fail`.

Post-flash verification (bounded):

- flashed updated firmware and ran burst acceptance:
  - run id: `postflash_burst10_timeoutguard_20260304_124337`
  - `HOSTCTL_UPLOAD_DIRECT_BURST_SENDER=1`
  - `HOSTCTL_UPLOAD_DIRECT_BURST_BYTES=65536`
  - `HOSTCTL_NET_CYCLES=10`
- result:
  - pass (`cycles=10`)
  - no `HOST_FAILURE` / `host_health_send_fail` markers
  - `avg_kib_s=104.89`, `stddev=8.72`
  - serial `read_wait_ms avg=2302.4`,
    `ingress_read_wait_empty_q_ms avg=2297.0`

Conclusion:

- this reproducer no longer enters the prior unstable `Ready`-but-unreachable
  class under bounded burst validation.

## 11.31 2026-03-04 Burst Control-Path Pooling A/B (Regression; Reverted)

Change under test:

- tried a hybrid host mode where only `PUT /upload` used direct burst sender,
  while control routes returned to normal pooled reqwest behavior.

A/B runs:

- baseline: `hybrid_base10_20260304_125730`
  - `avg_kib_s=147.73`, `stddev=3.85`
  - `read_wait_ms avg=2316.9`
  - `ingress_read_wait_empty_q_ms avg=2309.9`
- burst + pooled control: `hybrid_burst10_64k_20260304_125834`
  - `avg_kib_s=75.79`, `stddev=7.88`
  - `read_wait_ms avg=2279.2`
  - `ingress_read_wait_empty_q_ms avg=2271.1`
  - repeated per-cycle first-attempt `Connection refused` on burst `PUT`,
    typically requiring `2..3` attempts.

Verdict:

- regression for throughput/variance due transport retry churn.
- inference: pooled control connection conflicts with immediate next raw burst
  connect on the current single-connection listener behavior.

Action taken:

- reverted to burst-mode control-path close/no-pool semantics.
- rollback sanity run:
  - `burst_revertcheck3_20260304_130203`
  - stable `attempts=1` across cycles, `avg_kib_s=100.21`.

