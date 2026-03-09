## 11.43 2026-03-04 Ingress Fairness Threshold A/B (`24 KiB/48` vs `16 KiB/32`)

Profile:

- direct mode, burst sender off, boot discovery gate off, `cycles=20`.
- baseline reference: `logs/wifi_ingresstailbaseline20_direct_20260304_2002.log`.

Runs:

- variant A (`24576/48`):
  - flash env:
    - `MEDITAMER_HTTP_INGRESS_COOP_YIELD_BYTES=24576`
    - `MEDITAMER_HTTP_INGRESS_COOP_YIELD_READS=48`
  - log:
    `logs/wifi_ingresstailab_24576_48_direct20_20260304_2008.log`
  - host summary: `avg_kib_s=143.26`
  - upload stats: `req_ms avg=3103.4`, `p95=3331`
- variant B (`16384/32`):
  - flash env:
    - `MEDITAMER_HTTP_INGRESS_COOP_YIELD_BYTES=16384`
    - `MEDITAMER_HTTP_INGRESS_COOP_YIELD_READS=32`
  - logs:
    - `logs/wifi_ingresstailab_16384_32_direct20_20260304_2011.log`
    - `logs/wifi_ingresstailab_16384_32_direct20_confirm_20260304_2014.log`
  - host summaries:
    - run 1: `avg_kib_s=146.17`
    - confirm: `avg_kib_s=143.65`
  - combined upload stats (`n=40`):
    - `req_ms avg=3063.3`, `p95=3190`, `p99=3273`
    - `read_wait_ms avg=2368.7`, `p95=2488`
    - `ingress_read_wait_empty_q_ms avg=2363.0`, `p95=2482`
    - `ingress_read_wait_empty_q_max_ms avg=163.9`, `p95=187`
    - `ingress_read_empty_streak_ms_max avg=355.7`, `p95=458`

Comparison vs baseline:

- baseline (`n=20`): `req_ms avg=3095.8`, `p95=3283`, `p99=3283`
- `24576/48` regressed tails (`req_ms p95=3331`) with no throughput gain.
- `16384/32` improved tail metrics while staying non-regressive on throughput.

Decision:

- promote ingress fairness defaults to:
  - `HTTP_INGRESS_COOP_YIELD_BYTES_DEFAULT = 16 * 1024`
  - `HTTP_INGRESS_COOP_YIELD_READS_DEFAULT = 32`
  in `src/firmware/types/base.rs`.

## 11.44 2026-03-04 Promoted-Default Validation + Gate Attempt

Default-build validation (no override envs):

- flashed plain debug build.
- acceptance sanity run (`cycles=10`):
  - `logs/wifi_ingresstailpromote_default16384_32_direct10_20260304_2018.log`
  - host summary: `avg_kib_s=146.30` (guard clean)
  - upload stats (`n=10`):
    - `req_ms avg=3009.1`, `p95=3051`
    - `read_wait_ms avg=2311.5`, `p95=2355`
    - `ingress_read_wait_empty_q_ms avg=2306.3`, `p95=2350`

Regression-gate follow-up:

- attempted full gate + soak:
  - run dir:
    `logs/wifi_regression_gate_ingresstailpromote_default16384_32_20260304_2020`
- discovery stage passed, but `acceptance_1_cycle` hit extended in-session
  readiness churn (`ListenerWait`, `ipv4=0.0.0.0`) before upload and did not
  produce a clean gate completion in this run.

Action:

- keep promoted `16 KiB`/`32` ingress defaults.
- track readiness-churn recurrence as a separate gate blocker; re-run full gate
  once the listener/DHCP loop clears in-session.

## 11.45 2026-03-05 Full-Gate Reruns on Promoted Defaults (Blocked in Discovery)

Objective:

- close step 64 by running full regression gate + soak on promoted
  `16 KiB`/`32` defaults.

Attempts:

- attempt 1:
  - run dir:
    `logs/wifi_regression_gate_ingresstailpromote_default16384_32_rerun_20260305_0753`
  - artifact present:
    `discovery_debug.log` (no stage report emitted due prolonged stall/abort)
  - observed:
    - repeated `scan_done status=0 count=0` (`16` samples)
    - repeated `NET_STATUS ... "failure_class":"discovery_empty"` (`49` samples)
- attempt 2 (clean reflash before run):
  - run dir:
    `logs/wifi_regression_gate_ingresstailpromote_default16384_32_rerun2_20260305_0756`
  - artifact present:
    `discovery_debug.log` (no stage report emitted due prolonged stall/abort)
  - observed:
    - repeated `scan_done status=0 count=0` (`9` samples)
    - repeated `NET_STATUS ... "failure_class":"discovery_empty"` (`25` samples)

Interpretation:

- this is currently a reproducible discovery-stage instability under the live
  AP-dense environment, not an ingress upload regression signal.

Action:

- keep `16 KiB`/`32` promotion based on completed outlier A/B and acceptance
  validation evidence.
- move immediate focus to discovery-empty recurrence root-cause so step 64
  (full gate closure) can complete.

## 11.46 2026-03-05 Pre-Connect Zero-Discovery Guard (Step 65 Progress)

Change:

- firmware now short-circuits pre-connect when a full scan round sees zero APs
  (active + directed + passive + probe all zero):
  - file:
    `src/firmware/storage/upload/wifi/connect/prepare/prepare_scan.rs`
  - new transition trigger:
    `scan_zero_discovery_driver_restart`
  - behavior:
    - classify immediately as `failure_class=discovery_empty`, `failure_code=201`
    - perform hard recovery (`disconnect + stop + restart backoff`)
    - skip the previous association-first `no_ap_found` path.

Live validation:

- run:
  `logs/wifi_discovery_debug_step65_preconnect_guard_20260305_0808.log`
- prior failing reference:
  `logs/wifi_regression_gate_ingresstailpromote_default16384_32_rerun2_20260305_0756/discovery_debug.log`

Observed pre/post deltas:

- `connect_begin`: `1` -> `0`
- `event sta_disconnected reason=201`: `1` -> `0`
- `scan_zero_discovery_driver_restart`: `0` -> `30`

Interpretation:

- recovery classing is now cleaner and deterministic for all-zero scan rounds.
- follow-up hardening added parity terminal cap (`scan_zero_discovery_guard_terminal`)
  on the pre-connect guard path to prevent unbounded restart churn.
- underlying RF/scan-zero condition still reproduces; step 65 remains open until
  a full regression gate passes.

## 11.47 2026-03-05 Discovery Split Test (Minimal Host Pressure vs Control)

Objective:

- verify whether remaining discovery-empty failures are primarily host
  orchestration pressure or firmware/radio-side scan starvation.

Profiles (`rounds=3`, permissive pass thresholds):

- minimal pressure:
  - `recover_before_round=false`
  - `disable_listener_during_probe_rounds=false`
  - log:
    `logs/wifi_discovery_debug_step65_split_minpressure3r_20260305_0822.log`
- control:
  - `recover_before_round=true`
  - `disable_listener_during_probe_rounds=true`
  - log:
    `logs/wifi_discovery_debug_step65_split_control3r_20260305_0832.log`

Result:

- both profiles failed discovery equivalently:
  - `ready_rounds=0/3`
  - `zero_discovery_rounds=3/3`
  - `total_scan_nonzero_events=0`
  - repeated `post-hard-recover-connect-stall` in both runs.
- deltas:
  - minimal pressure: `total_scan_zero_events=77`, `total_no_ap_found_events=36`
  - control: `total_scan_zero_events=89`, `total_no_ap_found_events=16`

Interpretation:

- host pressure contributes noise but is not the primary root cause.
- dominant remaining issue is firmware/radio-side scan starvation under live
  AP conditions (all-round `scan_nonzero=0`), plus recurring post-hard-recover
  connect stalls.

## 11.48 2026-03-05 Watchdog Causality Check (`recover_before_round=false`)

Objective:

- verify whether post-hard-recover watchdog churn is itself the primary failure
  source, or a secondary symptom once discovery is already in all-zero scan
  state.

Runs:

- recover-before-round reference (`recover_before_round=true`):
  - log:
    `logs/wifi_discovery_debug_step65_watchdogdiag_1r_20260305_0852.log`
  - watchdog evidence:
    - `post-hard-recover-watchdog start reason=host_recover`
    - `scan_rounds=4`, `zero_scan_rounds=4`, `connect_begins=0`
    - terminal clear:
      `clear reason=scan_zero_discovery_guard_terminal ... elapsed_ms=176526`
- no-recover probe (`recover_before_round=false`):
  - log:
    `logs/wifi_discovery_debug_step65_watchdogdiag_norecover_1r_20260305_085746.log`
  - observed:
    - no `post-hard-recover-watchdog` start/clear lines emitted
    - direct transition:
      `Starting -> Scanning -> Failed (scan_zero_discovery_guard_terminal)`
    - round summary:
      `scan_zero=7`, `scan_nonzero=0`, `failure_class=discovery_empty`

Interpretation:

- post-hard-recover watchdog churn is a host-recover-contingent secondary path,
  not the primary trigger of discovery failure.
- primary blocker remains scan starvation itself (`scan_nonzero=0` even without
  watchdog entry), so next mitigation should target scan/recovery internals
  rather than watchdog timing.

## 11.49 2026-03-05 Listener-Pressure Is Not the Primary Driver (No-Recover Pair)

Objective:

- isolate listener pressure with `recover_before_round=false` fixed, comparing
  listener-on vs listener-off probe rounds.

Runs:

- listener on (`disable_listener_during_probe_rounds=false`):
  - log:
    `logs/wifi_discovery_debug_step65_watchdogdiag_norecover_1r_20260305_085746.log`
  - summary:
    `scan_zero=7`, `scan_nonzero=0`, `failure_class=discovery_empty`
- listener off (`disable_listener_during_probe_rounds=true`):
  - log:
    `logs/wifi_discovery_debug_step65_watchdogdiag_norecover_listeneroff_1r_20260305_090156.log`
  - summary:
    `scan_zero=38`, `scan_nonzero=0`, `failure_class=discovery_empty`
  - watchdog marker:
    `start reason=scan_zero_discovery_driver_restart` with repeated
    `scan_zero_discovery_driver_restart` transitions and no `connect_begins`.

Interpretation:

- disabling listener pressure did not restore any non-zero scan results.
- discovery remains starvation-dominated (`scan_nonzero=0` in both no-recover
  runs), reinforcing scan/recovery internals as the next root-cause target.
