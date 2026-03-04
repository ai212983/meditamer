## 2026-03-04: burst control-path pooling experiment (regression) and rollback

Experiment:

- goal: keep burst sender only for `PUT /upload` while restoring normal pooled
  reqwest behavior for control routes (`/mkdir`, `/health`, `/stat`).
- run IDs:
  - baseline: `hybrid_base10_20260304_125730`
  - burst (pooled control): `hybrid_burst10_64k_20260304_125834`

Observed:

- baseline (10 cycles):
  - `avg_kib_s=147.73`, `stddev=3.85`
  - `read_wait_ms avg=2316.9`
  - `ingress_read_wait_empty_q_ms avg=2309.9`
- burst with pooled control (10 cycles):
  - `avg_kib_s=75.79`, `stddev=7.88`
  - `read_wait_ms avg=2279.2`
  - `ingress_read_wait_empty_q_ms avg=2271.1`
  - repeated first-attempt `Connection refused` on every upload cycle
    (`host_upload_retry_diag`, usually attempts `2..3`), inflating cycle upload
    time despite slightly lower read-wait metrics.

Root-cause inference:

- pooled control connection kept the server-side single-connection loop occupied
  long enough that the next raw burst `PUT` connect frequently hit refusal.

Action:

- rolled back pooled-control behavior for burst mode:
  - burst mode again forces close/no-pool semantics for reqwest control path
    (while keeping direct burst sender for `PUT /upload`).
- quick rollback check:
  - run id: `burst_revertcheck3_20260304_130203`
  - no retry loop (`attempts=1` each cycle), `avg_kib_s=100.21`.

## 2026-03-04: reqwest-based burst sender A/B (regression; not promoted)

Experiment:

- replaced raw direct `PUT /upload` burst sender with reqwest blocking body
  reader path while preserving retry/diagnostic behavior.
- knobs under test:
  - `HOSTCTL_UPLOAD_DIRECT_BURST_SENDER=0|1`
  - `HOSTCTL_UPLOAD_DIRECT_BURST_BYTES=65536`
- profile:
  - `HOSTCTL_UPLOAD_MODE=direct`
  - `HOSTCTL_NET_CYCLES=10`
  - `HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE=0`

Runs:

- OFF: `logs/wifi_acceptance_burstab_reqwest_off_20260304_130807.log`
- ON: `logs/wifi_acceptance_burstab_reqwest_on_20260304_130910.log`

Observed:

- OFF (10 cycles):
  - `avg_kib_s=142.56`, `stddev=5.30`
  - `read_wait_ms avg=2452.3`
  - `ingress_read_wait_empty_q_ms avg=2447.5`
- ON (10 cycles):
  - `avg_kib_s=83.78`, `stddev=10.05`
  - `read_wait_ms avg=2307.2`
  - `ingress_read_wait_empty_q_ms avg=2301.0`

Interpretation:

- reqwest burst mode reduced ingress empty-queue wait counters by about `6%`,
  but overall throughput regressed by about `41%` and variance increased.
- this indicates the sender change did not translate reduced wait counters into
  faster end-to-end upload completion on current listener/transport behavior.

Action:

- do not promote reqwest burst sender mode.
- keep throughput-focused acceptance on the non-burst path
  (`HOSTCTL_UPLOAD_DIRECT_BURST_SENDER=0`) while pursuing the next dominant
  optimization target.

## 2026-03-04: firmware ingress try-drain cadence tuning (improves direct path)

Change:

- in `src/firmware/storage/upload/http/connection/body.rs`, adjusted inflight
  chunk try-drain polling cadence in the ingress read loop:
  - immediate poll when `recv_queue==0`,
  - otherwise poll every `4` reads (`INGRESS_TRY_DRAIN_INTERVAL_READS=4`).
- goal: reduce per-read hot-path overhead without regressing completion
  responsiveness during empty-queue stalls.

Validation profile:

- direct upload mode with burst sender OFF:
  - `HOSTCTL_UPLOAD_MODE=direct`
  - `HOSTCTL_UPLOAD_DIRECT_BURST_SENDER=0`
  - `HOSTCTL_NET_CYCLES=10`
  - `HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE=0`

Baseline for comparison:

- `logs/wifi_acceptance_burstab_reqwest_off_20260304_130807.log`
  - `avg_kib_s=142.56`, `stddev=5.30`
  - `read_wait_ms avg=2452.3`
  - `ingress_read_wait_empty_q_ms avg=2447.5`
  - `ingress_read_calls avg=90.5`

Post-change runs:

- `logs/wifi_acceptance_ingressdrain_tune_direct10_20260304_132054.log`
  - `avg_kib_s=146.50`, `stddev=5.54`
  - `read_wait_ms avg=2337.9`
  - `ingress_read_wait_empty_q_ms avg=2332.5`
  - `ingress_read_calls avg=88.2`
- `logs/wifi_acceptance_ingressdrain_tune_direct10_confirm_20260304_132309.log`
  - `avg_kib_s=148.93`, `stddev=4.02`
  - `read_wait_ms avg=2327.2`
  - `ingress_read_wait_empty_q_ms avg=2320.6`
  - `ingress_read_calls avg=89.3`

Observed effect:

- throughput improved by `+2.8%` (run A) and `+4.5%` (run B confirm) versus
  baseline.
- ingress empty-queue wait counters dropped by about `~5%`.
- `req_read_body_reset` guard remained stable (`delta=0`) in both runs.

Decision:

- keep cadenced try-drain behavior as the current direct-path default and use
  it as the new baseline for subsequent ingress-focused tuning.

## 2026-03-04: try-drain cadence sweep (`2/4/8`) and default promotion

Change:

- made ingress try-drain cadence build-time configurable:
  - `MEDITAMER_HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS`
  - fallback `HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS`
- sweep goal: pick cadence with best throughput/variance while minimizing
  ingress empty-queue wait.

Sweep profile:

- direct upload, burst sender OFF:
  - `HOSTCTL_UPLOAD_MODE=direct`
  - `HOSTCTL_UPLOAD_DIRECT_BURST_SENDER=0`
  - `HOSTCTL_NET_CYCLES=10`
  - `HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE=0`

Runs:

- cadence `2`:
  - `logs/wifi_acceptance_ingressdraincadence2_direct10_20260304_132736.log`
  - host summary: `avg_kib_s=145.51`, throughput stddev `6.84`
  - firmware means:
    - `read_wait_ms=2378.6`
    - `ingress_read_wait_empty_q_ms=2373.2`
    - `ingress_read_calls=87.6`
- cadence `4`:
  - `logs/wifi_acceptance_ingressdraincadence4_direct10_20260304_133009.log`
  - host summary: `avg_kib_s=145.55`, throughput stddev `9.07`
  - firmware means:
    - `read_wait_ms=2428.8`
    - `ingress_read_wait_empty_q_ms=2423.7`
    - `ingress_read_calls=89.3`
- cadence `8`:
  - `logs/wifi_acceptance_ingressdraincadence8_direct10_20260304_133238.log`
  - host summary: `avg_kib_s=143.32`, throughput stddev `4.59`
  - firmware means:
    - `read_wait_ms=2455.9`
    - `ingress_read_wait_empty_q_ms=2450.8`
    - `ingress_read_calls=89.8`

Result:

- cadence `2` and `4` were similar on mean throughput, but cadence `2` had
  lower ingress waits and lower throughput variance than cadence `4`.
- cadence `8` reduced mean throughput and increased ingress wait counters.

Decision:

- promoted default cadence to `2`:
  - `HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS_DEFAULT = 2`
    (`src/firmware/types/base.rs`)
- flashed promoted default and ran sanity check:
  - `logs/wifi_acceptance_ingressdrain_default2_sanity3_20260304_133613.log`
  - `cycles=3`, `avg_kib_s=149.07`, no `req_read_body_reset` guard deltas.

## 2026-03-04: high-AP per-cycle outlier investigation + retry-tail hardening

Investigation goal:

- focus on per-cycle outliers under AP-dense conditions (not mean-only shifts).

Baseline capture (direct path, burst sender OFF):

- run: `logs/wifi_outlierbaseline20_hostout_20260304_135616.log`
- serial: `logs/wifi_outlierbaseline20_serial_20260304_135616.log`
- summary (`n=20`):
  - `avg_kib_s=148.41`, `stddev=4.57`
  - min cycle: `133.93 KiB/s` (`cycle 7`)
  - `avg_upload_ms=3453.4`, max `3823 ms` (`cycle 7`)
- outlier signature:
  - no retry loops (`attempts=1` in host diagnostics)
  - cycle-7 spike aligned across host + firmware:
    - host `send_ms=3470`, `body_gap_ms_total=1713`
    - firmware `read_wait_ms=2605`, `ingress_read_wait_empty_q_ms=2602`

Burst stress A/B for retry-tail behavior:

- pre-hardening (burst enabled):
  - `logs/wifi_outlierab_burst32k_hostout_20260304_135903.log`
  - repeated `host_upload_retry_diag` transport-reset on first attempt
  - summary (`n=7`): `avg_kib_s=82.39`, `avg_upload_ms=6273.4`
- key finding:
  - `HOSTCTL_UPLOAD_DIRECT_BURST_BYTES=32768` did not reduce body read call
    count (`body_read_calls` remained `64`), so reqwest body pull cadence still
    behaved as `8 KiB` reads in this path.

Hardening change:

- file: `tools/hostctl/src/workflows_storage/upload/client.rs`
- for transport-reset retries:
  - faster recovery poll (`0.2s`)
  - single-success health gate for retry path
  - shorter retry backoff ramp (`75ms` step, capped `600ms`)
  - conservative fallback to legacy backoff when recovery probe does not pass

Post-hardening burst stress:

- run: `logs/wifi_outlierpost_retryhardening_hostout_20260304_140330.log`
- summary (`n=7`):
  - `avg_kib_s=113.28` (vs `82.39`, `+37%`)
  - `avg_upload_ms=4564.1` (vs `6273.4`, `-27%`)
  - max upload cycle `5539 ms` (vs `7038 ms`)

Direct-path safety check after hardening:

- run: `logs/wifi_outlierpost_nonburst10_hostout_20260304_140537.log`
- summary (`n=10`): `avg_kib_s=144.60`, one transient low outlier
  (`cycle 3: 118.27 KiB/s`), no retry markers.

Conclusion:

- retry-tail hardening reduced transport-reset outlier cost substantially.
- dominant remaining outlier class on direct non-burst path is still
  host-send-gap / firmware-empty-queue wait spikes without retries.
- next optimization target: non-retry ingress starvation (request-body scheduling
  cadence), not additional retry policy changes.

## 2026-03-04: direct stream burst sender (64 KiB writes) + pacing guard

Change:

- implemented true direct-stream `PUT /upload` sender for
  `HOSTCTL_UPLOAD_DIRECT_BURST_SENDER=1` in host uploader:
  - contiguous `TcpStream` body writes (`HOSTCTL_UPLOAD_DIRECT_BURST_BYTES`)
  - reqwest body pull path bypassed for this mode
- host diagnostics now show expected cadence in burst mode:
  - `body_read_calls=8`, `body_bytes_per_read=65536` for 512 KiB payload.

Observed A/B:

- burst stream with no pacing guard:
  - run: `logs/wifi_streamab_on_hostout_20260304_141346.log`
  - repeated first-attempt `Connection refused` retries (`attempts=2` in most
    cycles), `avg_kib_s=117.20`.
- burst stream with `HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=120`:
  - run: `logs/wifi_streamab_on_preput120_hostout_20260304_141538.log`
  - retry outliers disappeared in sampled cycles (`attempts=1`) but one later
    run hit unrelated host `/stat` timeout before summary.
- direct non-burst control remained faster:
  - `logs/wifi_streamab_off_hostout_20260304_141242.log`
  - `avg_kib_s=150.61`.

Hardening applied:

- set burst-mode default pre-put pacing guard in host uploader:
  - `HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS` default now `120` when
    `HOSTCTL_UPLOAD_DIRECT_BURST_SENDER=1` (still overrideable).
- post-guard burst sanity:
  - `logs/wifi_streamab_on_defaultguard_sanity3_hostout_20260304_142126.log`
  - stable `attempts=1` across 3 cycles, `avg_kib_s=129.56`.

Decision:

- keep direct stream burst sender as experimental (non-default).
- keep throughput acceptance default on non-burst path.
- retain burst default pacing guard to suppress retry-outlier explosions when
  burst mode is explicitly enabled for diagnostics.
