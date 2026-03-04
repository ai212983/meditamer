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
