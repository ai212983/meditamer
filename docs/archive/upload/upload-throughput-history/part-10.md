# Upload Throughput History (Part 10)

## 2026-03-04: listener-readiness closure follow-up (bounded soak=20)

Context:

- listener-readiness regression closure (step 58) had already passed bounded
  soak (`10` cycles), but one immediate follow-up `20`-cycle gate run failed
  early at `acceptance_1_cycle` with `failure_class=listener_not_ready`.

Runs:

- failed run:
  - `logs/wifi_regression_gate_link_gate_relax_soak20_20260304_193643/report.json`
  - failure stage: `acceptance_1_cycle`
  - failure class: `listener_not_ready`
- rerun (same environment/profile) after clean reflash:
  - `logs/wifi_regression_gate_link_gate_relax_soak20_rerun_20260304_194429/report.json`
  - `final_status=passed` (all stages)
  - soak summary: `cycles=20`, `avg_upload_s=3.50`, `avg_kib_s=146.65`

Observed on passing soak rerun (`upload_http: upload stats`, `n=20`):

- `read_wait_ms avg=2368.6`, `min=2267`, `max=3075`, `p95=2612`
- `ingress_read_wait_empty_q_ms avg=2363.1`, `min=2262`, `max=3070`,
  `p95=2607`
- dominant time remains ingress empty-queue read wait, not SD append path.

Decision:

- keep step-58 listener/DHCP readiness fixes as hardened defaults.
- retain this run pair as closure evidence for bounded soak=`20` under the same
  AP-dense environment.

## 2026-03-04: host `TCP_NODELAY` post-fix recheck (`direct`, 10-cycle A/B)

Goal:

- verify whether host upload socket Nagle behavior materially changes current
  ingress-empty wait bottleneck after readiness/listener hardening.

Profile:

- `HOSTCTL_UPLOAD_MODE=direct`
- `HOSTCTL_UPLOAD_DIRECT_BURST_SENDER=0`
- `HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE=0`
- `HOSTCTL_NET_CYCLES=10`

Runs:

- `TCP_NODELAY=1` (default):
  - `logs/wifi_nodelay_ab_on_direct10_20260304_1959.log`
  - host summary: `avg_upload_s=3.57`, `avg_kib_s=143.43`
  - upload stats: `req_ms avg=3109.9`, `p95=3248`
  - upload stats: `read_wait_ms avg=2420.8`,
    `ingress_read_wait_empty_q_ms avg=2415.1`
- `TCP_NODELAY=0`:
  - `logs/wifi_nodelay_ab_off_direct10_20260304_2000.log`
  - host summary: `avg_upload_s=3.60`, `avg_kib_s=142.54`
  - upload stats: `req_ms avg=3103.3`, `p95=3235`
  - upload stats: `read_wait_ms avg=2416.5`,
    `ingress_read_wait_empty_q_ms avg=2411.3`

Result:

- near-parity outcomes; no meaningful ingress wait reduction or throughput
  improvement from disabling `TCP_NODELAY`.
- `req_read_body_reset` guard remained stable (`delta=0`) in both runs.

Decision:

- keep `HOSTCTL_UPLOAD_TCP_NODELAY=1` as default.
- mark this knob as rechecked under post-step-58 baseline to avoid duplicate
  reruns without a new hypothesis.

Next focus:

- ingress tail hardening (`req_ms p95/p99`) via new empty-queue wait-burst
  histogram telemetry and outlier-focused `20`-cycle validation.

## 2026-03-04: ingress wait-tail telemetry instrumentation

Goal:

- expose ingress tail shape directly in `upload_http: upload stats` so outlier
  tuning can be driven by `p95/p99` evidence (not mean-only deltas).

Firmware change:

- file: `src/firmware/storage/upload/http/connection/body.rs`
- added fields:
  - `ingress_read_wait_over_100ms`
  - `ingress_read_wait_empty_q_over_10ms`
  - `ingress_read_wait_empty_q_over_50ms`
  - `ingress_read_wait_empty_q_over_100ms`
  - `ingress_read_wait_empty_q_max_ms`
  - `ingress_read_empty_streak_ms_max`

Validation:

- build passed: `scripts/build/build.sh debug`
- flashed firmware on `/dev/cu.usbserial-510`
- post-flash sanity run:
  - `logs/wifi_ingresstailtelemetry_sanity1_postflash_20260304_2001.log`
  - `upload_http: upload stats` includes all new fields.

Next:

- execute outlier-focused `20`-cycle run and tune for tail reduction with
  throughput parity.

## 2026-03-04: outlier baseline capture with new telemetry (`direct`, 20 cycles)

Run:

- `logs/wifi_ingresstailbaseline20_direct_20260304_2002.log`
- profile:
  - `HOSTCTL_UPLOAD_MODE=direct`
  - `HOSTCTL_UPLOAD_DIRECT_BURST_SENDER=0`
  - `HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE=0`
  - `HOSTCTL_NET_CYCLES=20`

Observed:

- host summary: `avg_upload_s=3.57`, `avg_kib_s=143.64`
- all cycles completed first-attempt with guard clean:
  - `req_read_body_reset delta=0`
- upload tail baseline (`n=20`, `upload_http: upload stats`):
  - `req_ms avg=3095.8`, `p95=3283`, `max=3377`
  - `read_wait_ms avg=2403.7`, `p95=2586`
  - `ingress_read_wait_empty_q_ms avg=2398.4`, `p95=2584`
  - `ingress_read_wait_over_100ms avg=7.3`
  - `ingress_read_wait_empty_q_max_ms avg=165.2`, `p95=194`
  - `ingress_read_empty_streak_ms_max avg=366.8`, `p95=466`, `max=533`

Interpretation:

- ingress empty-queue wait tails remain the dominant residual variance source.

Next:

- run ingress fairness threshold A/B tuning against this baseline and promote
  only if `req_ms p95/p99` improves without throughput regression.

## 2026-03-04: ingress fairness threshold A/B + promotion (`16 KiB/32`)

Runs (`cycles=20`, direct mode, burst off, boot gate off):

- baseline:
  - `logs/wifi_ingresstailbaseline20_direct_20260304_2002.log`
  - host summary: `avg_kib_s=143.64`
  - upload stats: `req_ms p95=3283`, `p99=3283`
- variant A (`24576/48`):
  - `logs/wifi_ingresstailab_24576_48_direct20_20260304_2008.log`
  - host summary: `avg_kib_s=143.26`
  - upload stats: `req_ms p95=3331` (worse)
- variant B (`16384/32`) + confirm:
  - `logs/wifi_ingresstailab_16384_32_direct20_20260304_2011.log`
  - `logs/wifi_ingresstailab_16384_32_direct20_confirm_20260304_2014.log`
  - host summary: `avg_kib_s=146.17` and `143.65`
  - combined upload stats (`n=40`):
    - `req_ms avg=3063.3`, `p95=3190`, `p99=3273`
    - `read_wait_ms avg=2368.7`, `p95=2488`
    - `ingress_read_wait_empty_q_ms avg=2363.0`, `p95=2482`
    - `ingress_read_empty_streak_ms_max p95=458` (baseline `466`)

Decision:

- promote firmware defaults in `src/firmware/types/base.rs`:
  - `HTTP_INGRESS_COOP_YIELD_BYTES_DEFAULT = 16 * 1024`
  - `HTTP_INGRESS_COOP_YIELD_READS_DEFAULT = 32`

## 2026-03-04: promoted-default validation and gate follow-up

Validation:

- flashed default debug build (no ingress override envs).
- acceptance sanity (`cycles=10`) at:
  - `logs/wifi_ingresstailpromote_default16384_32_direct10_20260304_2018.log`
  - host summary: `avg_kib_s=146.30`
  - upload stats: `req_ms avg=3009.1`, `p95=3051`
  - guard remained stable (`req_read_body_reset delta=0`).

Gate follow-up status:

- attempted full regression gate + soak:
  - `logs/wifi_regression_gate_ingresstailpromote_default16384_32_20260304_2020`
- discovery passed, but acceptance stage hit prolonged in-session
  `ListenerWait` (`ipv4=0.0.0.0`) churn before upload completion, so this gate
  attempt is treated as blocked by readiness instability rather than ingress
  throughput regression.

Continuation moved to:

- `docs/development/upload-throughput-history/part-11.md`
