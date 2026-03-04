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
