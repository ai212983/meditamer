# RFC: Upload Throughput Next Phase (Pipeline + Latency Decomposition)

- Status: In Progress (Phases A-B complete; Phase C deferred; next A/B pending)
- Owner: Firmware/Host Tooling
- Date: 2026-03-01
- Last Updated: 2026-03-03
- Scope: firmware upload path + host acceptance harness (no `tools/scene_*`)

## 1. Summary

This RFC proposes the next optimization phase for upload throughput after recent SD and HTTP improvements.

Primary proposal:

1. Add fine-grained upload latency decomposition telemetry.
2. Implement chunk pipeline (overlap HTTP body ingestion with SD write processing).
3. Keep discovery reliability invariants and gate throughput tests behind non-zero discovery proof.

## 2. Problem Statement

We improved throughput significantly over early baselines, but current telemetry still shows meaningful bottlenecks:

- SD wait remains a dominant fixed cost per 512 KiB upload (`sd_ms` commonly ~`1220..1300`).
- Network/body ingestion time is still variable (`body_ms` spikes observed).
- Current firmware path is effectively stop-and-wait by chunk:
  - read body data into chunk buffer
  - send to SD task
  - wait for SD result
  - continue reading

This serialization leaves overlap opportunity unused.

## 3. Current Evidence

From bounded 1-cycle sweep runs (524288-byte payload):

- `24 MHz`: `throughput_kib_s=178.58`, `upload_ms=2867`
- `30 MHz`: `throughput_kib_s=173.21`, `upload_ms=2956`
- `36 MHz`: `throughput_kib_s=194.97`, `upload_ms=2626`
- `40 MHz`: `throughput_kib_s=182.08`, `upload_ms=2812`

Device write metrics remained stable:

- `cmd24_sectors=40`
- `cmd25_attempt_sectors=1024`
- `cmd25_success_sectors=1024`
- `cmd25_fallback_bursts=0`

Interpretation:

- Payload path is healthy CMD25 multi-block.
- Metadata overhead is still present but stable.
- Largest remaining opportunity is reducing serialization and jitter, not retrying SD write-mode changes.

## 4. Goals

1. Increase sustained upload throughput with no discovery reliability regressions.
2. Preserve zero-fallback CMD25 behavior.
3. Reduce per-upload latency variance across 1-cycle, 3-cycle, and bounded soak runs.
4. Make next bottleneck obvious via telemetry without log forensics.

## 5. Non-Goals

1. Rewriting the full SD FAT layer.
2. Changing Wi-Fi association strategy in this RFC.
3. Any work under `tools/scene_*`.

## 6. Proposal

## 6.1 Phase A: Latency Decomposition Telemetry

- Status: Completed (2026-03-03)

Add explicit metrics/log fields for each upload request:

- socket read wait time
- payload copy time
- sd-bridge queue/send wait
- sd-task processing wait
- commit/rename phase time
- per-chunk p50/p95/max (bounded counters)

Expected output should support a one-line decomposition per upload and summary counters in existing `METRICS UPLOAD_*` serial reporting.

## 6.2 Phase B: Chunk Pipeline (Double Buffer)

- Status: Completed (implemented and enabled by default after 2026-03-03 A/B pass)

Implement a two-buffer producer/consumer flow:

- Buffer A is being written to SD task while buffer B is filled from socket.
- Ensure strict ordering semantics for chunk sequence.
- Preserve existing error handling and abort logic on first failed stage.

Constraints:

- Reuse PSRAM-backed buffers when available.
- Keep internal RAM pressure bounded and observable (`internal_free`, low-water).
- Maintain compatibility with existing `/upload` and `/upload_chunk` behavior.

## 6.3 Phase C: Metadata Cost Tightening

- Status: Deferred (post-pipeline evidence shows commit/metadata is not dominant)

After pipeline lands, profile remaining fixed cost.
If commit-phase metadata is still dominant:

- optimize append-session flush/rename ordering where safe
- avoid redundant metadata updates during active chunk stream
- keep crash-consistency guarantees unchanged

## 7. Reliability Guardrails

The following are mandatory for this RFC rollout:

1. Discovery proof before throughput profiling:
   - bounded `wifi-discovery-debug` round after boot
   - non-zero scan evidence and target SSID observed
2. Throughput runs must not proceed if discovery/connectivity is failing.
3. Upload listener disable-mode for discovery probes remains available for diagnosis.
4. Any throughput optimization that reintroduces zero-discovery regressions is rejected.

## 8. Validation Plan

Run sequence:

1. `wifi-discovery-debug` bounded round after boot.
2. `wifi-acceptance` 1-cycle.
3. `wifi-acceptance` 3-cycle.
4. bounded soak.

Capture and compare:

- `throughput_kib_s`, `upload_ms`
- `body_ms`, `sd_ms`, `req_ms`
- `cmd25_fallback_bursts` (must remain 0)
- discovery counters (`scan_zero`, `scan_nonzero`, `ssid_seen`)

Acceptance criteria:

1. No zero-discovery regression in validation sequence.
2. CMD25 fallback remains zero for normal acceptance payloads.
3. Throughput improvement is measurable in both:
   - 3-cycle average
   - bounded soak average
4. No new memory-pressure failure mode (`NoMem`) in scan/start paths.

## 9. Rollout and Rollback

Rollout:

1. Land telemetry first.
2. Land pipeline behind a compile-time or runtime toggle for A/B.
3. Enable by default only after passing full validation sequence.

Rollback:

1. Disable pipeline toggle.
2. Keep telemetry in place.
3. Revert only optimization layer; preserve diagnostics.

## 10. Risks

1. Increased buffer concurrency may increase internal RAM pressure.
2. Incorrect chunk ordering can corrupt uploads.
3. Pipeline error handling may produce stale SD results if not drained carefully.
4. Host acceptance harness timeouts can mask firmware improvements if not bounded consistently.

## 11. Execution Checklist for Next Session

1. [x] Confirm baseline commit and run discovery proof (2026-03-01 sequence recorded in throughput history).
2. [x] Add Phase A telemetry (implemented in firmware metrics and upload decomposition reporting).
3. [x] Run 1/3/soak with no behavior changes; record decomposition baseline (recorded in throughput history).
4. [x] Implement Phase B pipeline with guard toggle (`asset-upload-http-pipeline`).
5. [x] Re-run 1/3/soak with pipeline enabled, compare deltas, and check discovery stability (2026-03-03 A/B run).
6. [x] Decide on Phase C only if fixed commit/metadata cost remains dominant after Phase B A/B (2026-03-03: deferred; commit cost not dominant in A/B logs).
7. [x] Update throughput history and regression guardrail docs with final A/B measurements.
8. [x] Re-run regression gate with pipeline enabled in default feature set (`logs/wifi_regression_gate_default_confirm_20260303_121014`).
9. [ ] Run next A/B: make upload chunk size build-tunable, then compare `SD_UPLOAD_CHUNK_MAX=49_152` vs `65_536` under the same regression gate.

## 11.1 2026-03-03 A/B Execution Result

- Baseline (`pipeline=off`) regression gate:
  - run_id: `20260303_115711`
  - output: `logs/wifi_regression_gate_ab_off_20260303_115711`
  - result: passed (`discovery_debug`, acceptance 1-cycle, acceptance 3-cycle)
  - acceptance summary:
    - 1-cycle: `upload_ms=4412`, `throughput_kib_s=116.05`
    - 3-cycle avg: `avg_upload_s=4.79`, `avg_kib_s=107.48`

- Variant (`pipeline=on`, `asset-upload-http-pipeline`) regression gate:
  - run_id: `20260303_120041`
  - output: `logs/wifi_regression_gate_ab_on_20260303_120041`
  - result: passed (`discovery_debug`, acceptance 1-cycle, acceptance 3-cycle)
  - acceptance summary:
    - 1-cycle: `upload_ms=4103`, `throughput_kib_s=124.79`
    - 3-cycle avg: `avg_upload_s=4.14`, `avg_kib_s=123.86`

- A/B delta (pipeline on vs off):
  - 1-cycle upload time: `4412 -> 4103 ms` (`-7.0%`)
  - 3-cycle average upload time: `4.79 -> 4.14 s` (`-13.6%`)
  - 3-cycle average throughput: `107.48 -> 123.86 KiB/s` (`+15.2%`)
  - discovery stability: no zero-discovery regression observed (`zero_discovery_rounds=0` in both runs)

- Decision:
  - enable `asset-upload-http-pipeline` in default feature set
  - keep rollback path available via `CARGO_NO_DEFAULT_FEATURES=1` with
    explicit feature list excluding `asset-upload-http-pipeline`

## 11.2 2026-03-03 Default-Feature Confirmation (Post-Enable)

- Default build regression gate:
  - run_id: `20260303_121014`
  - output: `logs/wifi_regression_gate_default_confirm_20260303_121014`
  - result: passed (`discovery_debug`, acceptance 1-cycle, acceptance 3-cycle)
  - acceptance summary:
    - 1-cycle: `upload_ms=4169`, `throughput_kib_s=122.81`
    - 3-cycle avg: `avg_upload_s=4.54`, `avg_kib_s=113.11`

- Phase C decision basis:
  - `commit_ms` in sampled upload stats remains ~`233..235 ms` while request totals
    remain multi-second (`req_ms` ~`3190..3355`), so metadata/commit is not the
    dominant cost in this phase.

- Specific next step (codebase-aligned):
  - Add build-time tuning for `SD_UPLOAD_CHUNK_MAX` (currently fixed at `49_152` in
    `src/firmware/types/base.rs`) and run an A/B sweep at `49_152` vs `65_536` using
    `scripts/tests/hw/test_wifi_regression_gate.sh`.
