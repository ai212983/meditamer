# Upload Throughput History (Part 09)

## 2026-03-04: post-close-hint ingress fairness retune (`32 KiB` / `64` reads)

Goal:

- reduce remaining ingress empty-queue wait variance after close-hint stability
  fix, while keeping direct non-burst mode as default throughput path.

Baseline (current default before this retune):

- host summary:
  - `logs/wifi_streamab_postclosehint_off_hostout_20260304_145349.log`
  - `avg_kib_s=228.62`, throughput stddev `12.06`
- firmware ingress:
  - `logs/wifi_streamab_postclosehint_off_serial_20260304_145349.log`
  - `read_wait_ms avg=1358.3`
  - `ingress_read_wait_empty_q_ms avg=1353.5`

Experiment:

- flashed firmware build override:
  - `MEDITAMER_HTTP_INGRESS_COOP_YIELD_BYTES=32768`
  - `MEDITAMER_HTTP_INGRESS_COOP_YIELD_READS=64`
- clean bounded validation run:
  - `HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE=0`
  - `HOSTCTL_UPLOAD_MODE=direct`
  - `HOSTCTL_UPLOAD_DIRECT_BURST_SENDER=0`
  - `HOSTCTL_NET_CYCLES=10`
  - `logs/wifi_ingressyieldA_32768_64_hostout_nogate_clean_20260304_150629.log`
  - `logs/wifi_ingressyieldA_32768_64_serial_nogate_clean_20260304_150629.log`

Observed:

- host throughput:
  - `avg_kib_s=233.09` (`+1.95%` vs baseline)
  - throughput stddev `3.49` (from `12.06`)
  - range tightened to `226.15..238.92` (from `193.28..236.38`)
- firmware ingress:
  - `read_wait_ms avg=1296.9` (from `1358.3`)
  - `ingress_read_wait_empty_q_ms avg=1292.5` (from `1353.5`)
  - ingress empty-queue wait range tightened to `1259..1351` (from
    `1263..1748`)
- guardrails:
  - `req_read_body_reset` delta stayed `0`
  - all uploads completed at `attempts=1`

Decision:

- promote new firmware defaults in `src/firmware/types/base.rs`:
  - `HTTP_INGRESS_COOP_YIELD_BYTES_DEFAULT = 32 * 1024`
  - `HTTP_INGRESS_COOP_YIELD_READS_DEFAULT = 64`
- keep direct burst sender non-default; this retune targets direct non-burst
  variance hardening.

Notes:

- first post-flash run hit known boot discovery false-fail condition
  (`ready=true`, `scan_nonzero_events=0`, `ssid_seen_events=2`), so validation
  used no-gate mode for ingress-only comparison.
- default-build reflashing with promoted constants completed, but same-session
  validation runs were partially contaminated by high-AP transport turbulence
  (`host_transport_send_fail` / `host_health_send_fail`) while device
  `NET_STATUS` remained `Ready`:
  - `logs/wifi_ingressyield_defaultpromote_hostout_nogate_diag_20260304_151337.log`
  - `logs/wifi_ingressyield_defaultpromote_serial_nogate_diag_20260304_151337.log`

## 2026-03-04: startup/AP stability hardening (host acceptance)

Changes:

- boot discovery gate now accepts `ready + ssid_seen` evidence when
  `scan_nonzero_events` remains `0` (common parser/telemetry blind spot under
  AP contention).
- added cycle-1 readiness hysteresis before first upload:
  - requires `3` consecutive `GET /health` successes by default.
  - on failure, performs one `NET RECOVER` + `wait_ready` and retries streak
    check.

Validation (forced boot-gate path + single cycle):

- command profile:
  - `HOSTCTL_NET_BOOT_DISCOVERY_MAX_UPTIME_MS=999999999`
  - `HOSTCTL_NET_CYCLES=1`
- logs:
  - `logs/wifi_startup_apstability_check1_20260304_152352.log`
  - `logs/wifi_startup_apstability_check1_host_20260304_152352.log`

Observed:

- boot gate passed via new fallback:
  - `boot discovery gate: pass via ready+ssid fallback ready=true scan_nonzero_events=0 ssid_seen_events=2`
- cycle-1 hysteresis engaged and passed before upload:
  - `startup health hysteresis: pass cycle=1 ip=192.168.114.105 required_streak=3 attempts=3 elapsed_ms=858`
- upload then completed on first attempt with guard stable:
  - `upload_metrics_guard: req_read_body_reset delta=0`

## 2026-03-04: auth_reject loop root-cause isolation + candidate-preserve fix

Problem:

- under dense multi-AP contention, repeated `auth_reject` (`failure_code=210`,
  `no_ap_found_compatible_security`) loops were observed before throughput runs.

Root-cause evidence (from historical discovery debug log):

- after auth sweep exhaustion, firmware correctly rotated to next candidate:
  - `auth methods exhausted; switching to next candidate ... bssid_hint=74:4d:28:a9:b4:65`
- on the very next failure, observed-AP selection snapped back to strongest
  candidate and reset the sweep:
  - `wifi connect err ... bssid_hint=74:4d:28:a9:b4:65 observed_bssid=74:4d:28:61:6a:50 reason=210`
  - `retrying with candidate idx=0 ... bssid_hint=74:4d:28:61:6a:50`
- this produced an auth loop where rotated candidates were not held long enough
  for bounded auth-method sweep progression.

Firmware change:

- file:
  `src/firmware/storage/upload/wifi/connect/error/error_recovery/observed_ap.rs`
- behavior update:
  - for auth disconnect reasons, if current `bssid_hint` is still present in
    the observed candidate list, preserve that candidate (`ap_candidate_idx` +
    `channel_hint`) and skip snap-back to strongest observed AP.
  - allow downstream auth-recovery path to continue method rotation/escalation
    without being reset by observed-AP reorder noise.
- new diagnostic marker:
  - `auth-reject preserving hinted candidate ...`

Build / validation:

- build: `scripts/build/build.sh debug` passed.
- flash target used: `/dev/cu.usbserial-510`.
- bounded discovery-debug sanity run passed (`ready=true` across all rounds) at:
  - `logs/wifi_discovery_debug_authloopfix_20260304_160012.log`
- deterministic auth-reject-class mismatch run (forced WPA3-only) captured at:
  - `logs/wifi_discovery_debug_repro210_20260304_161431.log`
  - observed sustained `failure_code=211` (`no_ap_found_authmode_threshold`) with:
    - `auth-reject preserving hinted candidate` occurrences: `31`
    - `auth methods exhausted; switching to next candidate` occurrences: `31`
    - `retrying with candidate idx=0` occurrences: `0`
- this confirms the loop-break behavior under live auth-reject pressure for the
  same recovery class (`auth_reject`) targeted by the fix.
- normal auth-method baseline restored and verified with post-restore
  acceptance sanity run:
  - `logs/wifi_acceptance_post_step54_restore_20260304_163300.log`

Closure note:

- exact `failure_code=210` did not reproduce in the controlled mismatch setup
  (this environment emitted `211` for incompatible-auth policy), but step-54
  intent is satisfied at auth-reject class level with deterministic live logs.
- retain opportunistic collection of fresh `210` samples during future
  contention runs, without blocking the current phase.

## 2026-03-04: adaptive ingress fairness mode implementation (A/B blocked)

Goal:

- reduce non-retry per-cycle outliers by adapting ingress fairness yield
  thresholds during empty-queue starvation bursts.

Firmware changes:

- added optional adaptive mode in upload ingress loop:
  - `src/firmware/storage/upload/http/connection/body.rs`
  - `src/firmware/storage/upload/http/connection/fairness.rs`
- new build-time knob:
  - `MEDITAMER_HTTP_INGRESS_ADAPTIVE_FAIRNESS` (fallback
    `HTTP_INGRESS_ADAPTIVE_FAIRNESS`)
  - default `0` (off), variant `1` (on).
- new upload stats fields:
  - `ingress_adapt_enabled`
  - `ingress_adapt_switches`
  - `ingress_adapt_level_max`
  - `ingress_read_empty_streak_max`

Build evidence:

- default build passed: `scripts/build/build.sh debug`
- adaptive-on build passed:
  - `MEDITAMER_HTTP_INGRESS_ADAPTIVE_FAIRNESS=1 scripts/build/build.sh debug`

Live validation status:

- blocked by current discovery instability in this environment:
  - repeated `failure_class=discovery_empty` (`code=201`)
  - `post_recover_stall` (`code=251`) and transport recovery churn
- no upload cycles completed in blocking captures:
  - `logs/wifi_adaptfairness_recover_discovery_20260304_174510.log`
  - `logs/wifi_adaptfairness_sanity1_base_20260304_180323.log`

Next run shape once discovery is stable:

- matched direct A/B with absolute log paths:
  - baseline: adaptive off
  - variant: adaptive on
  - `HOSTCTL_UPLOAD_MODE=direct`
  - `HOSTCTL_UPLOAD_DIRECT_BURST_SENDER=0`
  - `HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE=0`
  - `HOSTCTL_NET_CYCLES=10` (promote to `20` after clean pair)

## 2026-03-04: AP-dense discovery masking fix + bounded regression gate pass

Observed regression shape (before fix):

- high-AP scans returned non-zero AP counts while target SSID candidates
  remained empty.
- recovery logic treated non-zero scans as progress and reset discovery-empty
  streaks too aggressively, contributing to repeated pre-upload instability.

Firmware mitigation:

- make scan/recovery progression target-aware:
  - probe fallback now keys on target-candidate visibility (`saw_target_candidate`)
    instead of generic non-zero scan count.
  - discovery sweep exhaustion streak reset now requires target candidate
    visibility (not merely unrelated AP visibility).
- changed files:
  - `src/firmware/storage/upload/wifi/scan.rs`
  - `src/firmware/storage/upload/wifi/connect/prepare/prepare_scan.rs`
  - `src/firmware/storage/upload/wifi/connect/error.rs`
  - `src/firmware/storage/upload/wifi/connect/error/error_recovery/main.rs`
  - `src/firmware/storage/upload/wifi/connect/error/error_recovery/discovery.rs`
  - `src/firmware/storage/upload/wifi.rs`

Validation evidence:

- build: `scripts/build/build.sh debug` passed.
- flash: `scripts/device/flash.sh debug` on `/dev/cu.usbserial-510`.
- bounded gate pass (discovery + acceptance 1-cycle + acceptance 3-cycle, soak
  skipped):
  - `logs/wifi_regression_gate_apdense_targetfix_20260304_182226/report.json`
  - `final_status=passed`
  - `failure_class=null`, `failure_code=null`
  - `panic_detected=false`
- prior blocking classes did not recur in this run:
  - `discovery_empty` (`201`)
  - `post_recover_stall` (`251`)
  - `start_nomem` (`253`)

Outcome:

- discovery/readiness instability is currently cleared for proceeding with
  adaptive ingress fairness matched A/B (step 56).

## 2026-03-04: adaptive ingress fairness matched A/B after discovery fix

Runs:

- adaptive off (default): `logs/wifi_adaptfairness_targetfix_ab_off_20260304_182643.log`
- adaptive on: `logs/wifi_adaptfairness_targetfix_ab_on_20260304_182854.log`
- shared profile: direct upload mode, burst sender off, boot discovery gate off,
  `cycles=10`.

Per-upload results (`upload_http: upload stats`, `n=10`):

- adaptive off:
  - `req_ms avg=3001.4`, `p95=3043`, `p99=3043`
  - derived throughput (`512000 / req_ms`) `avg=170.60 KiB/s`, `stddev=1.63`
  - `read_wait_ms avg=2315.1`, `ingress_read_wait_empty_q_ms avg=2310.6`
- adaptive on:
  - `req_ms avg=3032.7`, `p95=3147`, `p99=3147`
  - derived throughput (`512000 / req_ms`) `avg=168.89 KiB/s`, `stddev=3.20`
  - `read_wait_ms avg=2320.0`, `ingress_read_wait_empty_q_ms avg=2314.8`
  - adaptation telemetry confirms mode activity:
    - `ingress_adapt_switches avg=3`
    - `ingress_adapt_level_max avg=3`

Decision:

- adaptive mode did not improve ingress waits and increased variability.
- keep `HTTP_INGRESS_ADAPTIVE_FAIRNESS` as non-default diagnostic mode.

## 2026-03-04: listener readiness regression fix (`Ready + listener=false` churn)
Observed pre-fix:
- repeated `acceptance_1_cycle` pre-upload failures with
  `failure_class=listener_not_ready`, `attempt=12`,
  `trigger=attempt_budget_exhausted`.
- `NET_STATUS` remained `Ready/ListenerWait` with `listener=false` while
  listener gate stayed enabled.
- failing reports:
  - `logs/wifi_regression_gate_waitready_attempt_reset_20260304_190253/report.json`
  - `logs/wifi_regression_gate_ready_listener_guard_20260304_191237/report.json`

Fixes:
- host: reset `net_wait_ready` post-connect deadline when firmware `attempt`
  advances; force pre-start recover for `Ready + ipv4 + listener_enabled + !listener`.
- firmware: align HTTP listener gate with lease readiness in
  `src/firmware/storage/upload/http/diagnostics.rs`
  (`wifi_link_connected + non-zero DHCP lease`; `LinkDown` only when lease absent).

Validation:
- flashed on `/dev/cu.usbserial-510`.
- bounded gate pass:
  `logs/wifi_regression_gate_link_gate_relax_20260304_192653/report.json`
- bounded soak pass (`soak=10`):
  `logs/wifi_regression_gate_link_gate_relax_soak10_20260304_192924/report.json`
  (all stages passed; no panic/reboot markers; no listener-timeout classing).
Outcome: listener/DHCP readiness instability in this regression shape is
currently resolved under the same AP-dense environment.
