## 11.32 2026-03-04 Reqwest Burst Sender A/B (Regression; Not Promoted)

Change under test:

- replaced raw direct-burst `PUT /upload` sender with reqwest blocking body
  reader mode, preserving existing retry/diagnostic behavior.
- kept burst knobs:
  - `HOSTCTL_UPLOAD_DIRECT_BURST_SENDER`
  - `HOSTCTL_UPLOAD_DIRECT_BURST_BYTES=65536`

Bounded A/B profile:

- direct upload mode, 10 cycles, no boot-discovery gate:
  - `HOSTCTL_UPLOAD_MODE=direct`
  - `HOSTCTL_NET_CYCLES=10`
  - `HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE=0`

Runs:

- OFF (control): `logs/wifi_acceptance_burstab_reqwest_off_20260304_130807.log`
  - `avg_kib_s=142.56`, `stddev=5.30`
  - serial `read_wait_ms avg=2452.3`
  - serial `ingress_read_wait_empty_q_ms avg=2447.5`
- ON (reqwest burst): `logs/wifi_acceptance_burstab_reqwest_on_20260304_130910.log`
  - `avg_kib_s=83.78`, `stddev=10.05`
  - serial `read_wait_ms avg=2307.2`
  - serial `ingress_read_wait_empty_q_ms avg=2301.0`

Verdict:

- significant throughput regression (`-41.2%`) with worse variance when
  reqwest burst mode is enabled.
- despite slightly lower ingress wait counters (`~5.9%` lower), overall upload
  completion time increased materially.

Action:

- do not promote reqwest burst sender mode.
- keep `HOSTCTL_UPLOAD_DIRECT_BURST_SENDER=0` as the practical path for
  throughput-focused profiles until a different sender strategy demonstrates
  equal-or-better throughput without stability regressions.

## 11.33 2026-03-04 Ingress Try-Drain Cadence Tuning (Improvement)

Change under test:

- in firmware upload-body ingress loop
  (`src/firmware/storage/upload/http/connection/body.rs`), changed inflight
  pipeline drain polling from every read to a cadenced policy:
  - still poll immediately when `recv_queue==0` (avoid delaying completion when
    ingress is stalled),
  - otherwise poll every 4 reads (`INGRESS_TRY_DRAIN_INTERVAL_READS=4`).
- intent: reduce hot-path per-read overhead while preserving timely chunk
  completion checks under empty-queue waits.

Validation profile:

- direct mode, burst sender OFF:
  - `HOSTCTL_UPLOAD_MODE=direct`
  - `HOSTCTL_UPLOAD_DIRECT_BURST_SENDER=0`
  - `HOSTCTL_NET_CYCLES=10`
  - `HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE=0`

Comparison baseline:

- baseline (pre-change): `logs/wifi_acceptance_burstab_reqwest_off_20260304_130807.log`
  - `avg_kib_s=142.56`, `stddev=5.30`
  - `read_wait_ms avg=2452.3`
  - `ingress_read_wait_empty_q_ms avg=2447.5`
  - `ingress_read_calls avg=90.5`

Post-change runs:

- run A: `logs/wifi_acceptance_ingressdrain_tune_direct10_20260304_132054.log`
  - `avg_kib_s=146.50`, `stddev=5.54`
  - `read_wait_ms avg=2337.9`
  - `ingress_read_wait_empty_q_ms avg=2332.5`
  - `ingress_read_calls avg=88.2`
- run B (confirm): `logs/wifi_acceptance_ingressdrain_tune_direct10_confirm_20260304_132309.log`
  - `avg_kib_s=148.93`, `stddev=4.02`
  - `read_wait_ms avg=2327.2`
  - `ingress_read_wait_empty_q_ms avg=2320.6`
  - `ingress_read_calls avg=89.3`

Result:

- both post-change runs improved throughput vs baseline (`+2.8%` and `+4.5%`).
- ingress wait counters dropped by about `~4.7..5.2%`.
- no `req_read_body_reset` guard regressions during bounded runs.

Decision:

- keep the cadenced try-drain behavior as current optimization path.

## 11.34 2026-03-04 Try-Drain Cadence Sweep (`2/4/8`) and Default Selection

Change under test:

- made try-drain cadence compile-time configurable via:
  - `MEDITAMER_HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS`
  - fallback `HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS`
- objective: choose the best cadence for throughput vs variance while reducing
  ingress empty-queue wait.

Profile:

- `HOSTCTL_UPLOAD_MODE=direct`
- `HOSTCTL_UPLOAD_DIRECT_BURST_SENDER=0`
- `HOSTCTL_NET_CYCLES=10`
- `HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE=0`

Runs:

- cadence `2`: `logs/wifi_acceptance_ingressdraincadence2_direct10_20260304_132736.log`
  - host summary: `avg_kib_s=145.51`, throughput stddev `6.84`
  - firmware stats avg:
    - `read_wait_ms=2378.6`
    - `ingress_read_wait_empty_q_ms=2373.2`
    - `ingress_read_calls=87.6`
- cadence `4`: `logs/wifi_acceptance_ingressdraincadence4_direct10_20260304_133009.log`
  - host summary: `avg_kib_s=145.55`, throughput stddev `9.07`
  - firmware stats avg:
    - `read_wait_ms=2428.8`
    - `ingress_read_wait_empty_q_ms=2423.7`
    - `ingress_read_calls=89.3`
- cadence `8`: `logs/wifi_acceptance_ingressdraincadence8_direct10_20260304_133238.log`
  - host summary: `avg_kib_s=143.32`, throughput stddev `4.59`
  - firmware stats avg:
    - `read_wait_ms=2455.9`
    - `ingress_read_wait_empty_q_ms=2450.8`
    - `ingress_read_calls=89.8`

Interpretation:

- cadence `2` and `4` are near-tied on average throughput, but cadence `2`
  shows lower ingress waits, fewer ingress reads, and lower throughput
  variance than `4` in this sweep.
- cadence `8` reduces throughput despite slightly tighter spread.

Decision:

- promoted default try-drain cadence to `2` reads:
  - `HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS_DEFAULT = 2`
    (`src/firmware/types/base.rs`).
- post-promotion sanity run:
  - `logs/wifi_acceptance_ingressdrain_default2_sanity3_20260304_133613.log`
  - `cycles=3`, `avg_kib_s=149.07`, no `req_read_body_reset` guard regressions.

## 11.35 2026-03-04 Adaptive Ingress Fairness Mode (Implemented; Validation Blocked)

Change:

- added optional adaptive fairness mode in upload-body ingress loop:
  - file: `src/firmware/storage/upload/http/connection/body.rs`
  - helper: `src/firmware/storage/upload/http/connection/fairness.rs`
  - knob: `MEDITAMER_HTTP_INGRESS_ADAPTIVE_FAIRNESS` (fallback
    `HTTP_INGRESS_ADAPTIVE_FAIRNESS`), `0` default / `1` enable.
- adaptation behavior:
  - when repeated empty-queue waits are observed, temporarily lowers yield
    thresholds so the net runner gets scheduled more aggressively during
    subsequent ready-read bursts.
  - when non-empty reads stabilize, thresholds relax back toward baseline.
- added per-upload telemetry fields:
  - `ingress_adapt_enabled`
  - `ingress_adapt_switches`
  - `ingress_adapt_level_max`
  - `ingress_read_empty_streak_max`

Build validation:

- default build: `scripts/build/build.sh debug` passed.
- adaptive-on build:
  - `MEDITAMER_HTTP_INGRESS_ADAPTIVE_FAIRNESS=1 scripts/build/build.sh debug`
  - passed.

Live run status:

- matched A/B acceptance runs could not be completed because the current
  environment repeatedly fails before upload with discovery/recovery instability:
  - `failure_class=discovery_empty` / `failure_code=201`
  - `failure_class=post_recover_stall` / `failure_code=251`
- blocking logs:
  - `logs/wifi_adaptfairness_recover_discovery_20260304_174510.log`
  - `logs/wifi_adaptfairness_sanity1_base_20260304_180323.log`

Next execution gate:

- recover stable discovery first, then run matched direct A/B with absolute log
  paths:
  - baseline: `MEDITAMER_HTTP_INGRESS_ADAPTIVE_FAIRNESS=0`
  - variant: `MEDITAMER_HTTP_INGRESS_ADAPTIVE_FAIRNESS=1`
  - profile: `HOSTCTL_UPLOAD_MODE=direct`, `HOSTCTL_UPLOAD_DIRECT_BURST_SENDER=0`,
    `HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE=0`, `HOSTCTL_NET_CYCLES=10` (or
    `20` once stable).

## 11.36 2026-03-04 AP-Dense Discovery/Readiness Regression Mitigation

Root-cause refinement:

- in AP-dense environments, scan rounds can report non-zero AP counts while the
  target SSID candidate set is still empty.
- previous fallback/escalation checks keyed off `any AP seen`, which can mask
  `target missing` state and delay/derail discovery-empty recovery.

Firmware changes:

- scan fallback gating now keys off target-candidate visibility, not generic
  non-zero scan counts:
  - `src/firmware/storage/upload/wifi/scan.rs`
  - new `ScanOutcome` field: `saw_target_candidate`
- discovery-exhaustion streak resets now require target candidate visibility:
  - `src/firmware/storage/upload/wifi/connect/prepare/prepare_scan.rs`
  - `src/firmware/storage/upload/wifi/connect/error.rs`
  - `src/firmware/storage/upload/wifi/connect/error/error_recovery/main.rs`
  - `src/firmware/storage/upload/wifi/connect/error/error_recovery/discovery.rs`
- connect-error diagnostics now report both scan dimensions:
  - `scan_any_seen`
  - `scan_target_seen`

Validation:

- flashed debug firmware on `/dev/cu.usbserial-510`.
- bounded regression gate (discovery + acceptance 1-cycle + acceptance 3-cycle;
  soak skipped) passed:
  - run dir:
    `logs/wifi_regression_gate_apdense_targetfix_20260304_182226`
  - report:
    `logs/wifi_regression_gate_apdense_targetfix_20260304_182226/report.json`
  - `final_status=passed`, `failure_class=null`, `panic_detected=false`
- no recurrence of prior blocking classes in this run:
  - `discovery_empty` (`201`)
  - `post_recover_stall` (`251`)
  - `start_nomem` (`253`)

Result:

- discovery/readiness instability is currently unblocked for next-step adaptive
  ingress fairness A/B execution.

## 11.37 2026-03-04 Adaptive Ingress Fairness Matched A/B (Not Promoted)

Setup:

- baseline firmware: `MEDITAMER_HTTP_INGRESS_ADAPTIVE_FAIRNESS=0` (default)
- variant firmware: `MEDITAMER_HTTP_INGRESS_ADAPTIVE_FAIRNESS=1` (flashed)
- shared profile:
  - `HOSTCTL_UPLOAD_MODE=direct`
  - `HOSTCTL_UPLOAD_DIRECT_BURST_SENDER=0`
  - `HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE=0`
  - `HOSTCTL_NET_CYCLES=10`
- artifacts:
  - `logs/wifi_adaptfairness_targetfix_ab_off_20260304_182643.log`
  - `logs/wifi_adaptfairness_targetfix_ab_on_20260304_182854.log`

Measured deltas (per-upload `upload_http: upload stats` samples, `n=10`):

- `adaptive=0`:
  - `req_ms avg=3001.4`, `p95=3043`, `p99=3043`
  - derived throughput (`512000 / req_ms`) `avg=170.60 KiB/s`, `stddev=1.63`
  - `read_wait_ms avg=2315.1`, `p95=2375`
  - `ingress_read_wait_empty_q_ms avg=2310.6`, `p95=2372`
- `adaptive=1`:
  - `req_ms avg=3032.7`, `p95=3147`, `p99=3147`
  - derived throughput (`512000 / req_ms`) `avg=168.89 KiB/s`, `stddev=3.20`
  - `read_wait_ms avg=2320.0`, `p95=2461`
  - `ingress_read_wait_empty_q_ms avg=2314.8`, `p95=2458`
  - adaptation telemetry active (`ingress_adapt_switches avg=3`, `level_max avg=3`)

Decision:

- do not promote adaptive mode to default.
- keep `HTTP_INGRESS_ADAPTIVE_FAIRNESS` as non-default diagnostics knob.

## 11.38 2026-03-04 Listener Readiness Regression Closure (`Ready + listener=false`)

Pre-fix behavior:
- repeated `acceptance_1_cycle` pre-upload failures with `listener_not_ready`
  (attempt budget exhausted).
- logs showed `NET_STATUS` in `Ready/ListenerWait` with `listener=false` while
  listener gate was enabled.

Fixes applied:
- host: `wait_ready` deadline reset on attempt advance + pre-start recover when
  status is `Ready + ipv4 + listener_enabled + listener=false`.
- firmware: HTTP listener gate aligned to lease readiness
  (`wifi_link_connected + non-zero DHCP lease`; `LinkDown` only when lease absent).

Validation:
- flash target: `/dev/cu.usbserial-510`.
- bounded regression gate pass:
  `logs/wifi_regression_gate_link_gate_relax_20260304_192653/report.json`
- bounded soak gate pass (`soak=10`):
  `logs/wifi_regression_gate_link_gate_relax_soak10_20260304_192924/report.json`
  (all stages passed, no panic markers, no listener-timeout regression class).

Decision: keep these host + firmware changes as hardened defaults for
startup/listener readiness under AP-dense contention.
