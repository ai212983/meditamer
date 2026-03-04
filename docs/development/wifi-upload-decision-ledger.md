# Wi-Fi/Upload Decision Ledger (Hardened + Fixed)

As of: 2026-03-04

## Purpose

Keep a compact, decision-only ledger for Wi-Fi/upload tuning so we do not rerun already-decided experiments.

Use this file as the first preflight check before any new A/B or tuning run.

## How to Use

1. Check whether your knob/value pair already appears in the table below.
2. If status is `promoted`, `kept default`, `rejected`, or `non-default`, do not rerun by default.
3. Rerun only with explicit new rationale (new code path, new baseline, or user-requested reconfirmation).
4. If a knob/value is not listed here, run:

```bash
rg -n "<knob>|<value>" \
  docs/development/rfc-upload-throughput-next-phase \
  docs/development/upload-throughput-history
```

## Current Decisions

| Area | Decision | Status | Date | Evidence |
| --- | --- | --- | --- | --- |
| Upload chunk size (`SD_UPLOAD_CHUNK_MAX_DEFAULT`) | `65_536` is the active default and stable for bounded soak under current mitigations. | promoted | 2026-03-03 | [History part-03](./upload-throughput-history/part-03.md), [RFC part-01 step 17](./rfc-upload-throughput-next-phase/part-01.md) |
| SD SPI data clock (`MEDITAMER_SD_SPI_DATA_MHZ`) | Keep `36 MHz`; do not use `40 MHz` for default path due wider tail variance. | kept default / rejected variant | 2026-03-03 | [History part-03](./upload-throughput-history/part-03.md), [RFC part-01 step 18](./rfc-upload-throughput-next-phase/part-01.md) |
| Direct HTTP RX buffer (`HTTP_RX_BUF_TARGET`) | Keep `65_536`; reject `131_072`. | kept default / rejected variant | 2026-03-03 | [History part-05](./upload-throughput-history/part-05.md), [RFC part-01 step 30](./rfc-upload-throughput-next-phase/part-01.md), [RFC part-04 section 11.18](./rfc-upload-throughput-next-phase/part-04.md) |
| Host socket Nagle (`HOSTCTL_UPLOAD_TCP_NODELAY`) | Keep `TCP_NODELAY=1`; `0` regressed throughput/timing. | kept default / rejected variant | 2026-03-03 | [History part-05](./upload-throughput-history/part-05.md), [RFC part-04 section 11.20](./rfc-upload-throughput-next-phase/part-04.md) |
| Cross-cycle upload client reuse (`HOSTCTL_NET_REUSE_UPLOAD_CLIENT`) | Keep as opt-in diagnostics only; not a default throughput optimization. | non-default | 2026-03-04 | [History part-06](./upload-throughput-history/part-06.md), [RFC part-05](./rfc-upload-throughput-next-phase/part-05.md) |
| Reqwest burst sender mode (`HOSTCTL_UPLOAD_DIRECT_BURST_SENDER` reqwest path) | Do not promote; throughput/variance regressed despite lower ingress waits. | rejected | 2026-03-04 | [History part-08](./upload-throughput-history/part-08.md), [RFC part-07 section 11.32](./rfc-upload-throughput-next-phase/part-07.md) |
| Direct stream burst sender (`PUT /upload` burst path) | Keep experimental only; throughput default path remains non-burst. | non-default | 2026-03-04 | [History part-08](./upload-throughput-history/part-08.md), [RFC part-01 steps 49 and 51](./rfc-upload-throughput-next-phase/part-01.md) |
| Burst-mode pacing guard (`HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS`) | When burst mode is enabled, default guard remains `120 ms` to suppress retry explosions. | hardened default (burst mode only) | 2026-03-04 | [History part-08](./upload-throughput-history/part-08.md), [RFC part-01 step 49](./rfc-upload-throughput-next-phase/part-01.md) |
| Upload body-read idle timeout (`HTTP_UPLOAD_BODY_READ_TIMEOUT_MS`) | Keep `6000 ms` guard to prevent `Ready`-but-unreachable stalled body-read state. | hardened default | 2026-03-04 | [RFC part-01 step 44](./rfc-upload-throughput-next-phase/part-01.md) |
| Ingress try-drain cadence (`HTTP_INGRESS_TRY_DRAIN_INTERVAL_READS_DEFAULT`) | Keep default cadence at `2`. | promoted | 2026-03-04 | [History part-08](./upload-throughput-history/part-08.md), [RFC part-01 step 47](./rfc-upload-throughput-next-phase/part-01.md) |
| Ingress fairness defaults (`HTTP_INGRESS_COOP_YIELD_*_DEFAULT`) | Current promoted defaults are `32 KiB` and `64` reads (supersedes older `8192/24`). | promoted (latest) | 2026-03-04 | [History part-09](./upload-throughput-history/part-09.md), [RFC part-01 step 52](./rfc-upload-throughput-next-phase/part-01.md), [History part-07](./upload-throughput-history/part-07.md) |
| Adaptive ingress fairness (`HTTP_INGRESS_ADAPTIVE_FAIRNESS`) | Keep non-default diagnostics only (`0` default, `1` enable): matched post-fix A/B showed slightly worse `req_ms`/wait tails and higher variance despite active adaptation telemetry. | non-default / not promoted | 2026-03-04 | [RFC part-07 section 11.37](./rfc-upload-throughput-next-phase/part-07.md), [RFC part-01 step 56](./rfc-upload-throughput-next-phase/part-01.md), [History part-09](./upload-throughput-history/part-09.md) |
| `Connection: close` handling | Firmware honors close hint and closes immediately after response to reduce accept re-arm delay. | hardened behavior | 2026-03-04 | [RFC part-01 step 50](./rfc-upload-throughput-next-phase/part-01.md) |
| Startup/AP readiness in acceptance workflow | Keep boot-gate fallback (`ready + ssid_seen`) and cycle-1 health hysteresis (`3` successful checks) enabled. | hardened behavior | 2026-03-04 | [History part-09](./upload-throughput-history/part-09.md), [RFC part-01 step 53](./rfc-upload-throughput-next-phase/part-01.md) |
| Auth-reject loop recovery | Keep hinted-candidate preservation for auth-reject class to prevent immediate snap-back loops. | fixed | 2026-03-04 | [History part-09](./upload-throughput-history/part-09.md), [RFC part-01 step 54](./rfc-upload-throughput-next-phase/part-01.md) |
| AP-dense discovery fallback semantics | Use target-candidate visibility (not generic non-zero AP scan count) to drive zero-discovery probe fallback and discovery-exhaustion reset behavior. | hardened behavior | 2026-03-04 | [History part-09](./upload-throughput-history/part-09.md), [RFC part-07 section 11.36](./rfc-upload-throughput-next-phase/part-07.md), [RFC part-01 step 57](./rfc-upload-throughput-next-phase/part-01.md) |

## Update Rules

1. Add one row per durable decision (promoted, rejected, non-default, fixed).
2. Link each row to at least one RFC step or history section.
3. When a decision is superseded, keep the old row and mark the new row as latest.
4. Keep this ledger concise; details stay in RFC/history shards.
