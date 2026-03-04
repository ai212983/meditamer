# RFC: Upload Throughput Next Phase (Pipeline + Latency Decomposition)

- Status: In Progress (Phases A-B complete; Phase C deferred; chunk-size default `65_536` stable under current bounded soak gate; SD SPI variance A/B complete with `36 MHz` retained; CMD25 + FAT append + queue-boundary diagnostics completed; split-residual root-cause isolated to bridge publish-to-receive leg; mitigation via non-blocking inflight drain implemented and under extended validation; keep-alive fix validated with near-parity throughput; host cross-cycle client reuse retained as opt-in only)
- Owner: Firmware/Host Tooling
- Date: 2026-03-01
- Last Updated: 2026-03-04
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
9. [x] Run next A/B: make upload chunk size build-tunable, then compare `SD_UPLOAD_CHUNK_MAX=49_152` vs `65_536` under the same regression gate (2026-03-03 chunk-size A/B run).
10. [x] Run bounded soak at `SD_UPLOAD_CHUNK_MAX=65_536` before default switch (2026-03-03: failed due runtime panic in acceptance soak).
11. [x] Add stack-headroom diagnostics around upload begin/route + SD begin and capture panic-focused evidence (`feat(telemetry): add stack headroom probes for upload panic triage`).
12. [x] Reduce default touch trace pressure to recover stack headroom and rerun `65_536` bounded soak (`fix(touch): reduce trace channel buffers to reclaim stack headroom` + pass at `logs/wifi_regression_gate_65536_postfix_20260303_141406`).
13. [x] Waive extended soak (24h profile) and proceed with default switch at `SD_UPLOAD_CHUNK_MAX=65_536` (owner risk acceptance, 2026-03-03).
14. [x] Harden upload read-body reset recovery (`54e952f`) with immediate socket abort + `req_read_body_reset` telemetry counter.
15. [x] Re-run default `65_536` regression gate (with soak=10) three times and confirm all stages pass with no panic/reboot markers (`logs/wifi_regression_gate_default65536_connresetfix_r1_20260303_144611`, `logs/wifi_regression_gate_default65536_connresetfix_r2_20260303_144943`, `logs/wifi_regression_gate_default65536_connresetfix_r3_20260303_145315`).
16. [x] Add host acceptance guardrail step `assert_upload_metrics` to fail if `METRICS UPLOAD req_read_body_reset` increases beyond configured delta (`HOSTCTL_NET_REQ_READ_BODY_RESET_MAX_DELTA`, default `0`).
17. [x] Mark `SD_UPLOAD_CHUNK_MAX_DEFAULT=65_536` as stable for bounded soak gate and move to variance-reduction A/B planning.
18. [x] Run SD SPI variance A/B (`MEDITAMER_SD_SPI_DATA_MHZ=36` vs `40`) with full regression gate + soak and compare request-timing spread (`logs/wifi_regression_gate_sdspi36b_20260303_151750`, `logs/wifi_regression_gate_sdspi40_20260303_152151`).
19. [x] Fix host panic classification false-positive on upload metrics lines containing `_abort` (`2b2a3b3`).
20. [x] Add CMD25 burst/ready-wait diagnostics to upload write metrics (`3bb91e0`).
21. [x] Run 3x `36 MHz` bounded soak regression gates with burst diagnostics and correlate request timing with CMD25 wait metrics (`logs/wifi_regression_gate_sdspi36_burstdiag_r1_20260303_161323`, `logs/wifi_regression_gate_sdspi36_burstdiag_r2_20260303_161645`, `logs/wifi_regression_gate_sdspi36_burstdiag_r3b_20260303_162537`).
22. [x] Add per-chunk FAT append timing diagnostics (`ensure_capacity_ms`, `write_data_ms`, chunk boundary totals) to upload write metrics (`64f6da6`).
23. [x] Run 3x `36 MHz` bounded soak regression gates with append diagnostics and correlate high `chunk_max_ms` against append-path timing fields (`logs/wifi_regression_gate_sdspi36_appenddiag_r1b_20260303_163755`, `logs/wifi_regression_gate_sdspi36_appenddiag_r2_20260303_164229`, `logs/wifi_regression_gate_sdspi36_appenddiag_r3_20260303_164631`).
24. [x] Add queue-boundary chunk timing diagnostics (`enqueued_at_ms`, chunk queue-wait/handler timing in response, `sd_task_*` decomposition in HTTP upload stats) (`e85f2a7`).
25. [x] Run 3x `36 MHz` bounded soak regression gates with queue-boundary diagnostics and correlate high `chunk_max_ms` against queue/handler/residual fields (`logs/wifi_regression_gate_sdspi36_queuebridge_r1_20260303_170129`, `logs/wifi_regression_gate_sdspi36_queuebridge_r2b_20260303_170808`, `logs/wifi_regression_gate_sdspi36_queuebridge_r3_20260303_171241`).
26. [x] Add post-handler residual split instrumentation (SD-task publish-edge + bridge receive-edge timing) and expose split fields in `upload_http: upload stats`.
27. [x] Run bounded 3x soak correlation with split-residual instrumentation and identify dominant residual leg (`logs/wifi_acceptance_splitresidual_soak_r1_20260303_175924.log`, `logs/wifi_acceptance_splitresidual_soak_r2_20260303_180053.log`, `logs/wifi_acceptance_splitresidual_soak_r3_20260303_180239.log`).
28. [x] Implement bridge non-blocking inflight-drain mitigation and validate on bounded soak (`logs/wifi_acceptance_splitresidual_trydrain_soak_r1_20260303_180926.log`, `logs/wifi_acceptance_splitresidual_trydrain_soak_r2_20260303_181048.log`).
29. [x] Run host transport A/B to isolate ingress behavior (`HOSTCTL_UPLOAD_MODE=direct` vs `chunked`) and compare bounded 10-cycle results plus `METRICS UPLOAD_PHASE` deltas (`logs/wifi_acceptance_ingress_ab_direct_20260303_191416.log`, `logs/wifi_acceptance_ingress_ab_chunked_20260303_191605.log`).
30. [x] Run direct-mode HTTP RX buffer A/B (`65_536` vs `131_072`) and compare ingress/read-wait metrics plus `METRICS UPLOAD_PHASE` deltas (`logs/wifi_acceptance_ingress_rxbuf65536_direct10_20260303_192929.log`, `logs/wifi_acceptance_ingress_rxbuf131072_direct10_20260303_193224.log`).
31. [x] Add host direct-upload send diagnostics + retry classing with persisted sidecar logs (`HOSTCTL_UPLOAD_SEND_DIAG*`, `host_upload_send_diag`, `host_upload_retry_diag`).
32. [x] Run host transport retry-class probes (`HOSTCTL_UPLOAD_DISABLE_POOL`, `HOSTCTL_UPLOAD_FORCE_CONN_CLOSE`, `HOSTCTL_UPLOAD_FRESH_CLIENT_PER_UPLOAD`) and compare ingress deltas (`logs/wifi_acceptance_poolab_off_direct5_20260303_194538.log`, `logs/wifi_acceptance_poolab_on_direct5_20260303_194625.log`, `logs/wifi_acceptance_conncloseab_off_direct3_20260303_195201.log`, `logs/wifi_acceptance_conncloseab_on_direct3_20260303_195228.log`, `logs/wifi_acceptance_freshclientab_off_direct3_20260303_195342.log`, `logs/wifi_acceptance_freshclientab_on_direct3_20260303_195414.log`).
33. [x] Add host socket `TCP_NODELAY` A/B knob and compare direct-path ingress timing (`logs/wifi_acceptance_nodelayab_on_direct5_20260303_195857.log`, `logs/wifi_acceptance_nodelayab_off_direct5_20260303_200015.log`).
34. [x] Add firmware ingress wait split metrics (`ingress_read_wait_empty_q_ms`/`ingress_read_wait_nonempty_q_ms`) and capture bounded validation run (`logs/wifi_acceptance_ingress_waitsplit_direct3_20260303_200511.log`).
35. [x] Add Wi-Fi RSSI telemetry context (`METRICS WIFI_LINK` + per-upload RSSI fields), validate discovery recovery, and run direct 10-cycle correlation sample (`logs/wifi_discovery_rssi_recover_20260304_083129.log`, `logs/wifi_acceptance_ingress_rssi_direct10_20260304_083419.log`).
36. [x] Add deep host retry cause-chain diagnostics + direct pre-PUT pacing knob, then run bounded direct A/B (`HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=0` vs `120`) to verify first-attempt failure class behavior (`logs/wifi_acceptance_preputdelay_off_direct5_20260304_084754.log`, `logs/wifi_acceptance_preputdelay_on120_direct5_20260304_084843.log`).
37. [x] Add firmware `NET_ACCEPT` microsecond accept-arm gap telemetry and capture bounded direct evidence (`logs/wifi_acceptance_acceptarmgapus_direct3b_20260304_090920.log`).
38. [x] Validate firmware keep-alive/multi-request socket handling fix (remove forced `Connection: close`) with bounded direct acceptance (`HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=0`) after serial-port recovery (2026-03-04; matched direct A/B pairs now recorded in sections 11.24-11.25).
39. [x] Add optional host acceptance cross-cycle upload-client reuse (`HOSTCTL_NET_REUSE_UPLOAD_CLIENT`) and run bounded direct comparisons against forced-close control (2026-03-04; no consistent throughput gain).
40. [x] Focus next optimization on firmware ingress empty-queue `read_wait_ms` dominance (implemented cooperative read-loop fairness yield in firmware and captured bounded validation, 2026-03-04).
41. [x] Re-run extended direct acceptance (`cycles=10`) with ingress fairness yield enabled and tune yield thresholds (`bytes`/`reads`) for throughput vs variance (2026-03-04).
42. [x] Run bounded soak with promoted ingress fairness thresholds to confirm failure-class stability and long-run variance behavior (`logs/wifi_acceptance_ingressfairness_soak10_20260304_113208.log`).

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

## 11.3 2026-03-03 Chunk-Size A/B (`49_152` vs `65_536`)

- Implementation:
  - added build-time chunk-size override in `src/firmware/types/base.rs`:
    - preferred: `MEDITAMER_SD_UPLOAD_CHUNK_MAX`
    - fallback: `SD_UPLOAD_CHUNK_MAX`
    - accepted range (PSRAM upload build): `4096..65536`
  - widened upload chunk command length field to `u32` to safely support `65536`
    without truncation.

- Baseline (`SD_UPLOAD_CHUNK_MAX=49_152`) regression gate:
  - run_id: `20260303_123948`
  - output: `logs/wifi_regression_gate_chunk_ab_49152_final_20260303_123948`
  - result: passed
  - acceptance summary:
    - 1-cycle: `upload_ms=4828`, `throughput_kib_s=106.05`
    - 3-cycle avg: `avg_upload_s=5.08`, `avg_kib_s=101.07`

- Variant (`MEDITAMER_SD_UPLOAD_CHUNK_MAX=65536`) regression gate:
  - run_id: `20260303_124328`
  - output: `logs/wifi_regression_gate_chunk_ab_65536_20260303_124328`
  - result: passed
  - acceptance summary:
    - 1-cycle: `upload_ms=4708`, `throughput_kib_s=108.75`
    - 3-cycle avg: `avg_upload_s=4.36`, `avg_kib_s=117.86`

- A/B delta (`65536` vs `49_152`):
  - 1-cycle upload time: `4828 -> 4708 ms` (`-2.5%`)
  - 3-cycle average upload time: `5.08 -> 4.36 s` (`-14.2%`)
  - 3-cycle average throughput: `101.07 -> 117.86 KiB/s` (`+16.6%`)
  - discovery: passed in both runs (`discovery_debug` stage passed; no panic/reboot markers)

- Decomposition signal:
  - `49_152` variant: `chunks=11`, `max_chunk=49152`, `cmd25_attempt_bursts=21`
  - `65_536` variant: `chunks=8`, `max_chunk=65536`, `cmd25_attempt_bursts=16`
  - both: `cmd25_fallback_bursts=0`

- Decision:
  - keep build-time override support in place.
  - do not switch default chunk size to `65536` yet.

## 11.4 2026-03-03 Bounded Soak Follow-Up (`65_536`)

- Run:
  - output: `logs/wifi_regression_gate_chunk_ab_65536_soak10_clean_20260303_130052`
  - command shape: `HOSTCTL_NET_SOAK_CYCLES=10 scripts/tests/hw/test_wifi_regression_gate.sh`

- Result:
  - `discovery_debug`: passed
  - `acceptance_1_cycle`: passed
  - `acceptance_3_cycle`: passed
  - `acceptance_soak`: failed (`final_status=failed`)
  - panic class: `runtime_panic_other`
  - panic excerpt: `logs/wifi_regression_gate_chunk_ab_65536_soak10_clean_20260303_130052/panic_excerpt.log`

- Failure signature:
  - panic occurred during repeated soak uploads at `request method=PUT path=/upload`
    after `sd_upload: begin ...`
  - runtime message: `Detected a write to the stack guard value on ProCpu`

- Decision:
  - keep default `SD_UPLOAD_CHUNK_MAX=49_152` for now.
  - treat `65_536` as experimental override until panic mitigation evidence is repeated in extended soak.

## 11.5 2026-03-03 Panic-Focused Mitigation + `65_536` Re-Validation

- Mitigation commits:
  - `dd9eaf7` (`feat(telemetry): add stack headroom probes for upload panic triage`)
  - `912fb02` (`fix(touch): reduce trace channel buffers to reclaim stack headroom`)

- Regression gate rerun (`SD_UPLOAD_CHUNK_MAX=65_536`, soak=10):
  - output: `logs/wifi_regression_gate_65536_postfix_20260303_141406`
  - result: passed (`discovery_debug`, acceptance 1-cycle, acceptance 3-cycle, acceptance soak)
  - panic markers: none (`panic_detected=false`)

- Stack evidence from the pass run:
  - `stack_diag: tag=sd_upload_begin_entry ... headroom=11160 ... total=43492`
  - `stack_diag: tag=http_upload_route_entry ... headroom=36024 ... total=43492`
  - observed minimum headroom in this run: `11160` bytes.

- Additional observation:
  - a separate instrumentation run (`logs/wifi_regression_gate_stackdiag_postfix_20260303_140801`) failed soak on a transport-level body read reset (`ConnectionReset`) without panic/reboot signatures.

- Next decision gate:
  - superseded by Section 11.6 risk-accept decision.

## 11.6 2026-03-03 Owner Decision: Skip 24h Soak and Proceed

- Decision input:
  - explicit owner instruction to skip the 24h soak and proceed.

- Action taken:
  - switch firmware default `SD_UPLOAD_CHUNK_MAX_DEFAULT` to `65_536`.
  - align host fallback `/upload_chunk` default (`HOSTCTL_UPLOAD_CHUNK_SIZE`) to `65536`.

- Operational note:
  - keep `MEDITAMER_SD_UPLOAD_CHUNK_MAX` override available for quick rollback to `49_152` if field behavior regresses.

## 11.7 2026-03-03 Transport-Reset Hardening + 3x Regression Gate

- Hardening commit:
  - `54e952f` (`fix(upload): harden read-body reset recovery and add reset metrics`)
  - runtime behavior changes:
    - immediate socket abort on `read body` / `incomplete body` request paths.
    - bounded read-body abort wait (`1.5s`) before recovery return.
    - new upload metric counter: `req_read_body_reset`.

- Re-validation runs (default feature set, `SD_UPLOAD_CHUNK_MAX_DEFAULT=65_536`, soak=10):
  - `logs/wifi_regression_gate_default65536_connresetfix_r1_20260303_144611`
  - `logs/wifi_regression_gate_default65536_connresetfix_r2_20260303_144943`
  - `logs/wifi_regression_gate_default65536_connresetfix_r3_20260303_145315`

- Result:
  - all three runs: `final_status=passed`
  - all stages passed in each run: `discovery_debug`, `acceptance_1_cycle`,
    `acceptance_3_cycle`, `acceptance_soak`
  - panic/reboot markers: none (`panic_detected=false`, `unexpected_reboot_detected=false`)
  - no `ConnectionReset` / `request err=read body` / `body read err` signature matches
    in stage logs.

## 11.8 Phase Handoff: Throughput Variance Reduction

- Completion decision:
  - treat default `65_536` upload chunking as stable for the bounded soak gate under the current hardening set.

- Next A/B step (specific, codebase-aligned):
  - run a bounded variance A/B sweep of SD SPI data clock while keeping pipeline/default chunking unchanged:
    - A: default (`MEDITAMER_SD_SPI_DATA_MHZ=36`)
    - B: variant (`MEDITAMER_SD_SPI_DATA_MHZ=40`)
  - execute `scripts/tests/hw/test_wifi_regression_gate.sh` with soak enabled for both variants and compare:
    - stage pass/fail and panic/reboot markers
    - upload request timing spread (`req_ms` / `sd_ms` / `read_wait_ms` from `upload_http: upload stats`)
    - throughput drift across repeated runs.

## 11.9 2026-03-03 SD SPI Variance A/B (`36` vs `40` MHz)

- Run artifacts:
  - `36 MHz`: `logs/wifi_regression_gate_sdspi36b_20260303_151750`
  - `40 MHz`: `logs/wifi_regression_gate_sdspi40_20260303_152151`

- Gate result:
  - both runs passed: `discovery_debug`, `acceptance_1_cycle`,
    `acceptance_3_cycle`, `acceptance_soak`
  - discovery invariants remained intact in both runs.

- Decomposition comparison from `upload_http: upload stats`:
  - `36 MHz` soak (`n=10`):
    - `req_ms avg=3162.2`, range `3100..3248`
    - `sd_ms avg=2864.3`, range `2808..2962`
    - `read_wait_ms avg=2475.9`, range `2418..2573`
  - `40 MHz` soak (`n=10`):
    - `req_ms avg=3377.2`, range `3106..4816`
    - `sd_ms avg=3037.9`, range `2789..4419`
    - `read_wait_ms avg=2694.0`, range `2417..4127`

- Decision:
  - keep SD SPI data clock default at `36 MHz`.
  - do not promote `40 MHz`; it increases timing spread and exhibits
    significantly worse upper-tail latency in this bounded soak pass.

## 11.10 2026-03-03 CMD25 Burst Diagnostics + 3x Soak Correlation

- Instrumentation commit:
  - `3bb91e0` (`feat(storage): add cmd25 burst wait diagnostics for uploads`)
  - new per-upload `sd_upload: write_metrics` fields:
    - `cmd25_success_burst_ms_total`, `cmd25_success_burst_ms_avg`
    - `cmd25_ready_wait_count`, `cmd25_ready_wait_ms_total`,
      `cmd25_ready_wait_ms_avg`
    - `cmd25_ready_wait_polls_total`, `cmd25_ready_wait_polls_avg`
    - `cmd25_ready_wait_over_1ms`, `cmd25_ready_wait_over_4ms`,
      `cmd25_ready_wait_over_8ms`

- 3x `36 MHz` bounded soak runs used for correlation:
  - `logs/wifi_regression_gate_sdspi36_burstdiag_r1_20260303_161323`
  - `logs/wifi_regression_gate_sdspi36_burstdiag_r2_20260303_161645`
  - `logs/wifi_regression_gate_sdspi36_burstdiag_r3b_20260303_162537`
  - note: `logs/wifi_regression_gate_sdspi36_burstdiag_r3_20260303_162008`
    failed due host-side health send failures despite `NET_STATUS state=Ready`,
    so it is excluded from correlation set.

- Correlation result:
  - all three selected runs passed full gate (including soak).
  - no soak uploads exceeded `req_ms > 3400` in this instrumented set (`0/30`).
  - CMD25 wait metrics remained low per upload:
    - `cmd25_ready_wait_ms_total` averaged `3.2..4.2 ms`
    - `cmd25_ready_wait_over_8ms` was rare (`0..1` events per run total)
  - conclusion: observed latency spread is not primarily explained by CMD25
    ready-wait stalls.

## 11.11 2026-03-03 FAT Append Diagnostics + 3x Soak Correlation

- Instrumentation commit:
  - `64f6da6` (`feat(upload): add fat append chunk timing diagnostics`)
  - new `sd_upload: write_metrics` fields for SD-task chunk boundaries:
    - `chunk_total_ms_*`, `chunk_ensure_ready_ms_*`,
      `chunk_payload_lock_ms_*`
    - `chunk_append_ms_*`, `chunk_append_capacity_ms_*`,
      `chunk_append_write_data_ms_*`
    - `chunk_overhead_ms_*` and outlier counters (`*_over_200ms`, `*_over_400ms`)

- 3x `36 MHz` bounded soak runs:
  - `logs/wifi_regression_gate_sdspi36_appenddiag_r1b_20260303_163755`
  - `logs/wifi_regression_gate_sdspi36_appenddiag_r2_20260303_164229`
  - `logs/wifi_regression_gate_sdspi36_appenddiag_r3_20260303_164631`
  - all three passed full gate (discovery, `1-cycle`, `3-cycle`, soak=10).

- Correlation result:
  - `req_ms > 3400`: `0/30`.
  - `chunk_max_ms > 400`: `5/30` samples (`420`, `629`, `477`, `449`, `407`).
  - append-path timings stayed tight:
    - `chunk_append_ms_avg` per upload: `126.7..127.4 ms`
    - `chunk_append_capacity_ms_avg`: `38.0..38.5 ms`
    - `chunk_append_write_data_ms_avg`: `87.6..87.9 ms`
    - `chunk_append_ms_max` observed ceiling: `145 ms`
  - outlier example:
    - `chunk_max_ms=629` while same-upload `chunk_append_ms_max=134`.
  - conclusion: current upper-tail `chunk_max_ms` spread is not dominated by
    `fat::append_session_write` execution time.

## 11.12 2026-03-03 Queue-Boundary Diagnostics + 3x Soak Correlation

- Instrumentation commit:
  - `e85f2a7` (`feat(upload): add chunk queue-boundary residual diagnostics`)
  - `SdUploadRequest` now carries `enqueued_at_ms`; chunk responses include
    `chunk_queue_wait_ms` and `chunk_handler_ms`.
  - `upload_http: upload stats` now emits:
    - `sd_task_queue_wait_ms`
    - `sd_task_handler_ms`
    - `sd_task_residual_ms`
  - `sd_upload: write_metrics` now emits queue/residual fields:
    - `chunk_queue_wait_ms_*`
    - `chunk_non_append_ms_*`
    - `chunk_residual_ms_*`

- 3x `36 MHz` bounded soak runs:
  - selected set:
    - `logs/wifi_regression_gate_sdspi36_queuebridge_r1_20260303_170129`
    - `logs/wifi_regression_gate_sdspi36_queuebridge_r2b_20260303_170808`
    - `logs/wifi_regression_gate_sdspi36_queuebridge_r3_20260303_171241`
  - excluded run:
    - `logs/wifi_regression_gate_sdspi36_queuebridge_r2_20260303_170602`
      (`acceptance_1_cycle` failed with `net_wait_ready: listener timeout`).

- Correlation result (selected 30 uploads):
  - `req_ms avg=3060.8`, range `2933..3752`, `req_ms > 3400`: `1/30`.
  - `chunk_max_ms avg=364.7`, range `318..666`, `chunk_max_ms > 400`: `6/30`.
  - `chunk_append_ms_avg` remained stable at `126.2 ms`.
  - queue/handler decomposition:
    - `sd_task_queue_wait_ms avg=47.0 ms`
    - `sd_task_handler_ms avg=1017.6 ms`
    - `sd_task_residual_ms avg=1239.0 ms`
  - high-`chunk_max_ms` samples aligned with elevated residual, not handler growth:
    - example: `chunk_max_ms=666` with `sd_task_handler_ms=1027`,
      `sd_task_residual_ms=1845`.
  - conclusion: current upper tail is not dominated by queue wait or handler
    execution; post-handler residual wait remains the dominant unexplained term.

## 11.13 Next Step (Specific)

- Shift root-cause focus to post-handler residual path:
  - stamp chunk completion-to-response timing explicitly (publish edge in SD task
    and receive edge in SD bridge) to split `sd_task_residual_ms` into:
    - SD-task post-handler pre-publish delay
    - response transit/receive delay
  - correlate these new subcomponents against `chunk_max_ms > 400` uploads.
  - rerun bounded 3x `36 MHz` soak after this split-residual instrumentation.

## 11.14 2026-03-03 Post-Handler Residual Split Instrumentation

- Firmware instrumentation update:
  - SD task now stamps chunk handler completion and publish edge for chunk
    responses.
  - SD bridge now stamps receive edge and computes publish-to-receive delay.
  - `upload_http: upload stats` now includes split residual fields:
    - `sd_task_post_handler_ms`
    - `sd_task_publish_to_receive_ms`
    - `sd_task_residual_other_ms`
  - `sd_task_residual_ms` is retained for continuity and now decomposes into:
    `post_handler + publish_to_receive + residual_other`.

- Smoke verification (1-cycle acceptance):
  - run: `logs/wifi_acceptance_split_residual_smoke_20260303_173818.log`
  - observed upload stats sample:
    - `sd_task_residual_ms=1291`
    - `sd_task_post_handler_ms=1`
    - `sd_task_publish_to_receive_ms=1290`
    - `sd_task_residual_other_ms=0`
  - preliminary signal: current residual in this sample is almost entirely in
    publish-to-receive delay, not SD-task post-handler delay.

- Next measurement step:
  - run bounded 3x `36 MHz` regression gates with this split-residual
  instrumentation and correlate `chunk_max_ms > 400` uploads against the new
  split fields.

## 11.15 2026-03-03 Split-Residual Correlation (3x Bounded Soak)

Run set used for split-residual correlation:

- bounded soak runs (`HOSTCTL_NET_CYCLES=10`, `HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE=0`):
  - `logs/wifi_acceptance_splitresidual_soak_r1_20260303_175924.log`
  - `logs/wifi_acceptance_splitresidual_soak_r2_20260303_180053.log`
  - `logs/wifi_acceptance_splitresidual_soak_r3_20260303_180239.log`

Notes:

- full regression-gate attempts in this window were unstable for reasons not
  tied to upload chunk timing (acceptance pre-stage listener/boot-discovery
  gate failures), so split-residual correlation used the direct bounded soak
  acceptance runs above.
- one stats line in `r3` was serial-concatenated/truncated and excluded from
  aggregate parsing.

Correlation summary (`upload_http: upload stats`, valid `n=29`):

- `req_ms avg=3135.1`
- `chunk_max_ms avg=401.4`
- `chunk_max_ms > 400`: `9/29`
- decomposition means:
  - `sd_task_queue_wait_ms avg=43.7`
  - `sd_task_handler_ms avg=1035.4`
  - `sd_task_residual_ms avg=1259.9`
- residual split means:
  - `sd_task_post_handler_ms avg=0.4`
  - `sd_task_publish_to_receive_ms avg=1259.5`
  - `sd_task_residual_other_ms avg=0.0`
- residual split shares:
  - `post_handler`: `~0.0%`
  - `publish_to_receive`: `~100.0%`
  - `other`: `~0.0%`
- outliers (`chunk_max_ms > 400`) remained publish-to-receive dominated:
  - `sd_task_residual_ms avg=1429.2`
  - `sd_task_publish_to_receive_ms avg=1429.0`

Conclusion:

- post-handler residual is not SD-task post-handler execution time.
- the dominant leg is bridge-side publish-to-receive delay.

Next focused root-cause target:

- investigate why chunk result receive is deferred during pipelined body ingest
  (bridge receive cadence/drain timing), and evaluate whether this delay is:
  - expected overlap accounting (diagnostic artifact), or
  - avoidable receive-lag that materially inflates request tail latency.

## 11.16 2026-03-03 Bridge Drain Mitigation (Non-Blocking Inflight Poll)

Change:

- added non-blocking chunk-result drain in HTTP body ingest loop:
  - bridge now attempts `try_receive` for inflight SD chunk result between body
    reads and drains completed chunks early without waiting for queue-boundary
    flush.
  - this reduces deferred publish-to-receive accumulation while preserving
    pipeline overlap behavior.

Validation runs:

- `logs/wifi_acceptance_splitresidual_trydrain_soak_r1_20260303_180926.log`
- `logs/wifi_acceptance_splitresidual_trydrain_soak_r2_20260303_181048.log`

Comparison vs pre-fix split-residual soak set:

- pre-fix (`n=29`, r1-r3):
  - `req_ms avg=3135.1`
  - `chunk_max_ms avg=401.4` (`>400`: `9`)
  - `sd_task_residual_ms avg=1259.9`
  - `sd_task_publish_to_receive_ms avg=1259.5`
- post-fix (`n=20`, r1-r2):
  - `req_ms avg=3175.6`
  - `chunk_max_ms avg=172.4` (`>400`: `0`)
  - `sd_task_residual_ms avg=40.0`
  - `sd_task_publish_to_receive_ms avg=39.7`

Interpretation:

- bridge publish-to-receive deferral was the dominant residual contributor and
  is substantially mitigated by early non-blocking drain.
- request mean stayed in the same band in this small sample; extended gate
  validation is still required.

Next step:

- rerun full regression-gate campaign (1/3/soak profile) on this mitigation and
  confirm:
  - discovery stability
  - no panic/reboot regressions
  - upload request timing remains stable or better under bounded soak.

## 11.17 2026-03-03 Host Transport A/B for Ingress Isolation (`direct` vs `chunked`)

Change:

- added host upload mode selector in hostctl:
  - `HOSTCTL_UPLOAD_MODE=auto|direct|chunked`
  - `auto` keeps existing behavior (try `PUT /upload`, fallback to chunked flow).
  - `direct` forces `PUT /upload` only.
  - `chunked` forces `/upload_begin` + `/upload_chunk` + `/upload_commit`.

Runs:

- direct (`HOSTCTL_UPLOAD_MODE=direct`, cycles=10):
  - `logs/wifi_acceptance_ingress_ab_direct_20260303_191416.log`
  - summary: `avg_upload_s=6.27`, `avg_kib_s=82.64`
- chunked (`HOSTCTL_UPLOAD_MODE=chunked`, cycles=10):
  - `logs/wifi_acceptance_ingress_ab_chunked_20260303_191605.log`
  - summary: `avg_upload_s=17.88`, `avg_kib_s=28.93`

Metric comparison method:

- primary comparison used `METRICS UPLOAD_PHASE` deltas (first vs last sample
  in each run) to avoid serial-line sampling bias under high request volume.
- both runs transferred the same payload volume (`5.0 MiB` across 10 cycles).

`METRICS UPLOAD_PHASE` delta results (normalized):

- direct (`reqs_per_512KiB=1.0`):
  - `body_ms`: `2457.3 ms/512KiB`
  - `sd_ms`: `1563.8 ms/512KiB`
  - `req_ms`: `3156.3 ms/512KiB`
- chunked (`reqs_per_512KiB=8.0`):
  - `body_ms`: `1685.0 ms/512KiB`
  - `sd_ms`: `1056.6 ms/512KiB`
  - `req_ms`: `2923.6 ms/512KiB`

Interpretation:

- forcing chunked transport lowers per-byte server-side request/body timing.
- despite that, end-to-end throughput collapses (`82.64 -> 28.93 KiB/s`) due
  multi-request orchestration overhead on the host/device path.
- this rejects forced chunking as the optimization path for current default
  upload flow.

Specific next root-cause target:

- keep direct `PUT /upload` as the performance path.
- focus ingress optimization inside direct upload:
  - investigate sender pacing / TCP ingress cadence that manifests as high
    `read_wait_ms` with mostly empty pre-read queues.
  - retain `HOSTCTL_UPLOAD_MODE` as an A/B lever for future validation.

## 11.18 2026-03-03 Direct Upload RX Buffer A/B (`65_536` vs `131_072`)

Change:

- added compile-time HTTP RX socket buffer tuning for PSRAM upload builds:
  - preferred env: `MEDITAMER_HTTP_RX_BUF_TARGET`
  - fallback: `HTTP_RX_BUF_TARGET`
  - accepted range: `8192..262144` (default `65536`)

Runs (`HOSTCTL_UPLOAD_MODE=direct`, `HOSTCTL_NET_CYCLES=10`):

- baseline (`HTTP_RX_BUF_TARGET=65_536`, default build):
  - `logs/wifi_acceptance_ingress_rxbuf65536_direct10_20260303_192929.log`
  - runtime confirmation: `upload_http: http_rx buffer placement=Psram bytes=65536`
- variant (`MEDITAMER_HTTP_RX_BUF_TARGET=131072`):
  - `logs/wifi_acceptance_ingress_rxbuf131072_direct10_20260303_193224.log`
  - runtime confirmation: `upload_http: http_rx buffer placement=Psram bytes=131072`

`upload_http: upload stats` comparison (`n=10` each):

- baseline:
  - `read_wait_ms avg=2398.9`
  - `req_ms avg=3093.5`
  - `ingress_pre_read_q_total avg=36347.6` (`~413.0 bytes/read`)
  - `ingress_read_wait_over_50ms avg=7.8` (`8.9%` of reads)
- variant:
  - `read_wait_ms avg=2802.6`
  - `req_ms avg=3491.6`
  - `ingress_pre_read_q_total avg=58205.2` (`~937.3 bytes/read`)
  - `ingress_read_wait_over_50ms avg=16.8` (`27.1%` of reads)

`METRICS UPLOAD_PHASE` delta comparison (equal `5.0 MiB` transferred):

- baseline:
  - `body_ms=2398.9 ms/512KiB`
  - `sd_ms=1566.3 ms/512KiB`
  - `req_ms=3093.5 ms/512KiB`
- variant:
  - `body_ms=2802.6 ms/512KiB`
  - `sd_ms=1684.6 ms/512KiB`
  - `req_ms=3491.4 ms/512KiB`

Host summary throughput:

- baseline: `avg_kib_s=97.83`
- variant: `avg_kib_s=80.26`

Decision:

- keep HTTP RX buffer target default at `65_536`.
- reject `131_072`; it worsens request latency and throughput in this direct-path
  bounded run.

Next step:

- keep direct upload path and focus on ingress pacing/jitter not solved by RX
  buffer growth:
  - instrument host-side send cadence (request write phase timing and burst/idle
    pattern) and correlate against firmware `read_wait_ms` spikes.

## 11.19 2026-03-03 Host Send Diagnostics + Retry-Class Probes (Direct Path)

Changes:

- host direct-upload instrumentation in hostctl:
  - per-upload timing line: `host_upload_send_diag`
  - retry classification line: `host_upload_retry_diag` (`transport_reset`,
    `sd_busy`, `timeout`, `transient`)
  - sidecar persistence default: `<HOSTCTL_NET_LOG_PATH>.hostdiag`
- host retry hardening:
  - rebuild client on `transport_reset` retry path
  - require configurable consecutive health passes before retrying:
    `HOSTCTL_UPLOAD_NET_RECOVERY_CONSECUTIVE_HEALTH`

Primary correlation run (`HOSTCTL_UPLOAD_MODE=direct`, cycles=10):

- `logs/wifi_acceptance_senddiag2_direct10_20260303_194248.log`
- `logs/wifi_acceptance_senddiag2_direct10_20260303_194248.log.hostdiag`

Aggregate (`n=10`):

- firmware:
  - `read_wait_ms avg=2475.9`
  - `req_ms avg=3150.0`
- host:
  - `send_ms avg=3326.4`
  - `avg_attempts=2.00`
  - correlation: `corr(send_ms, read_wait_ms)=0.944`

Retry-class probe runs:

- pool A/B:
  - off: `logs/wifi_acceptance_poolab_off_direct5_20260303_194538.log`
  - on: `logs/wifi_acceptance_poolab_on_direct5_20260303_194625.log`
  - delta (`on` vs `off`): `read_wait_ms 2439.6 -> 2395.0`, `req_ms 3116.2 -> 3084.8`
- connection-close A/B:
  - off: `logs/wifi_acceptance_conncloseab_off_direct3_20260303_195201.log`
  - on: `logs/wifi_acceptance_conncloseab_on_direct3_20260303_195228.log`
  - delta (`on` vs `off`): `read_wait_ms 2541.3 -> 2419.0`, `req_ms 3204.3 -> 3080.0`
  - retries increased (`retry_count 1 -> 3`)
- fresh-client A/B:
  - off: `logs/wifi_acceptance_freshclientab_off_direct3_20260303_195342.log`
  - on: `logs/wifi_acceptance_freshclientab_on_direct3_20260303_195414.log`
  - near-neutral latency deltas; retries unchanged in this sample (`3` vs `3`)

Interpretation:

- send-side timing remains strongly coupled with firmware `read_wait_ms`.
- no host transport toggle consistently removes `transport_reset` first-attempt
  retries while preserving clear ingress wins.

## 11.20 2026-03-03 Direct Upload `TCP_NODELAY` A/B (`1` vs `0`)

Change:

- added `HOSTCTL_UPLOAD_TCP_NODELAY` (`1` default) in host upload client.

Runs (`HOSTCTL_UPLOAD_MODE=direct`, cycles=5):

- `TCP_NODELAY=1`:
  - `logs/wifi_acceptance_nodelayab_on_direct5_20260303_195857.log`
  - summary: `avg_upload_s=5.56`, `avg_kib_s=93.23`
- `TCP_NODELAY=0`:
  - `logs/wifi_acceptance_nodelayab_off_direct5_20260303_200015.log`
  - summary: `avg_upload_s=5.91`, `avg_kib_s=87.59`

`upload_http: upload stats` aggregates (`n=5`):

- `TCP_NODELAY=1`:
  - `read_wait_ms avg=2289.4`
  - `req_ms avg=2960.0`
  - `ingress_pre_read_q_total avg=31602.2`
- `TCP_NODELAY=0`:
  - `read_wait_ms avg=2397.6`
  - `req_ms avg=3060.4`
  - `ingress_pre_read_q_total avg=35916.2`

Decision:

- keep default `HOSTCTL_UPLOAD_TCP_NODELAY=1`.
- disabling `TCP_NODELAY` regressed both throughput and request timing in this
  bounded A/B.

## 11.21 2026-03-03 Ingress Wait Split Telemetry (Empty vs Non-Empty RX Queue)

Change:

- added firmware ingress wait decomposition:
  - `ingress_read_wait_empty_q_ms`
  - `ingress_read_wait_nonempty_q_ms`

Validation run:

- `logs/wifi_acceptance_ingress_waitsplit_direct3_20260303_200511.log`
- `logs/wifi_acceptance_ingress_waitsplit_direct3_20260303_200511.log.hostdiag`

Aggregate (`n=3`):

- `read_wait_ms avg=2355.7`
- `ingress_read_wait_empty_q_ms avg=2351.3`
- `ingress_read_wait_nonempty_q_ms avg=4.3`
- empty-queue share of read-wait: `~99.8%`

Interpretation:

- direct-path ingress wait is almost entirely no-data waiting (socket queue
  empty), not delayed reads against already-buffered data.

Specific next root-cause target:

- keep direct upload + `TCP_NODELAY=1` baseline.
- shift optimization focus to upstream ingress pacing (network/AP/radio path)
  rather than HTTP socket buffer sizing or host client pooling toggles.

## 11.22 2026-03-04 Wi-Fi RSSI Context for Ingress Correlation

Changes:

- added connected-watchdog RSSI sampling via `WifiController::rssi()`.
- added `METRICS WIFI_LINK` line:
  - `rssi_last_dbm`, `rssi_min_dbm`, `rssi_max_dbm`, `rssi_samples`,
    `rssi_low_samples`
- added per-upload RSSI context fields to `upload_http: upload stats`:
  - `wifi_rssi_last_dbm`, `wifi_rssi_min_dbm`, `wifi_rssi_max_dbm`,
    `wifi_rssi_samples`, `wifi_rssi_low_samples`

Validation sequence:

- post-flash acceptance attempt hit boot discovery gate timeout (expected guard):
  - `logs/wifi_acceptance_ingress_rssi_direct3_20260304_082811.log`
- recovery proof:
  - `logs/wifi_discovery_rssi_recover_20260304_083129.log`
  - summary: `ready_rounds=8`, `zero_discovery_rounds=0`,
    `total_scan_nonzero_events=1`
- bounded direct sample:
  - `logs/wifi_acceptance_ingress_rssi_direct10_20260304_083419.log`
  - `logs/wifi_acceptance_ingress_rssi_direct10_20260304_083419.log.hostdiag`

Direct 10-cycle aggregate (`n=10`):

- request timing:
  - `read_wait_ms avg=2532.8`
  - `req_ms avg=3227.5`
  - `ingress_read_wait_empty_q_ms avg=2528.2`
  - `ingress_read_wait_nonempty_q_ms avg=4.6`
- Wi-Fi RSSI context:
  - `wifi_rssi_last_dbm avg=-62.5` (range `-68..-59`)
  - `wifi_rssi_min_dbm avg=-71.0`
  - `wifi_rssi_low_samples avg=1.0`
- correlation checks:
  - `corr(rssi_last, read_wait_ms)=0.056` (weak in this sample band)
  - `corr(send_ms, read_wait_ms)=0.991` (strong)

Interpretation:

- ingress wait remains overwhelmingly empty-queue dominated.
- within observed RSSI band, link signal variation does not explain read-wait
  variance as strongly as host send pacing/transport behavior.

Specific next step:

- keep the RSSI context instrumentation.
- focus root-cause on direct-path transport cadence and first-attempt
  `transport_reset` behavior, with AP/radio factors treated as secondary unless
  wider RSSI variance appears.

## 11.23 2026-03-04 Host Retry Cause-Chain + Pre-PUT Pacing A/B

Changes:

- expanded `host_upload_retry_diag` with:
  - typed reqwest flags (`reqwest_*`)
  - typed IO flags (`io_*`)
  - compact full error chain (`err_chain=...`)
- added host knob:
  - `HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS` (`0` default), applied before each direct
    `PUT /upload` attempt.
- added host failure-class refinement:
  - `host_transport_connect_refused` (distinguishes connect-refused from generic
    send failure).

Runs (`HOSTCTL_UPLOAD_MODE=direct`, `HOSTCTL_UPLOAD_SEND_DIAG=1`, cycles=5):

- baseline (`HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=0`):
  - `logs/wifi_acceptance_preputdelay_off_direct5_20260304_084754.log`
  - `logs/wifi_acceptance_preputdelay_off_direct5_20260304_084754.log.hostdiag`
  - summary: `avg_upload_s=6.45`, `avg_kib_s=79.72`
- variant (`HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=120`):
  - `logs/wifi_acceptance_preputdelay_on120_direct5_20260304_084843.log`
  - `logs/wifi_acceptance_preputdelay_on120_direct5_20260304_084843.log.hostdiag`
  - summary: `avg_upload_s=5.11`, `avg_kib_s=103.96`

Host diagnostics delta:

- baseline:
  - `host_retry_count=5/5`
  - `avg_attempts=2.00`
  - repeated first-attempt chain:
    `client error (Connect) <- tcp connect error <- Connection refused (os error 61)`
- variant (`120 ms` pre-PUT delay):
  - `host_retry_count=0/5`
  - `avg_attempts=1.00`
  - no first-attempt retry lines observed.

Firmware upload-stats aggregate (`n=5`, last five upload requests per run):

- baseline:
  - `read_wait_ms avg=2546.8`
  - `req_ms avg=3224.6`
  - `ingress_read_wait_empty_q_ms avg=2541.4`
  - `ingress_read_wait_nonempty_q_ms avg=5.4`
- variant:
  - `read_wait_ms avg=2602.8`
  - `req_ms avg=3285.8`
  - `ingress_read_wait_empty_q_ms avg=2598.2`
  - `ingress_read_wait_nonempty_q_ms avg=4.6`

Interpretation:

- dominant first-attempt failure signature is now explicit: connect-refused on
  direct `PUT /upload` before body transfer.
- a short bounded host pre-PUT delay suppresses that failure class in this
  sample and improves end-to-end throughput by removing retry overhead.
- core ingress bottleneck remains empty-queue read wait; pacing does not reduce
  per-success request `read_wait_ms` materially.

Specific next root-cause target:

- instrument and isolate firmware-side listener availability around the
  `mkdir -> upload` transition (accept-loop readiness window), then validate
  whether a firmware-side fix can remove connect-refused without host delay.

## 11.24 2026-03-04 `NET_ACCEPT` Microsecond Gap Evidence + Keep-Alive Fix (Validated)

Completed changes:

- upgraded accept-arm telemetry to microsecond granularity:
  - `METRICS NET_ACCEPT arm_gap_n arm_gap_us arm_gap_us_max ...`
- implemented firmware keep-alive/multi-request socket handling:
  - response helper uses `HTTP/1.1` + `Connection: keep-alive`
  - socket cycle serves multiple requests per accepted socket
  - short keep-alive idle guard (`500 ms`) prevents idle monopolization.

Bounded direct validation (`HOSTCTL_UPLOAD_MODE=direct`, `HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=0`):

- keep-alive ON (cycles=3):
  - summary: `avg_upload_s=3.62`, `avg_kib_s=141.86`
- forced close (cycles=3):
  - summary: `avg_upload_s=3.40`, `avg_kib_s=150.67`
- keep-alive ON repeat (cycles=3):
  - summary: `avg_upload_s=3.53`, `avg_kib_s=145.39`
- matched warmed pair (cycles=3):
  - forced close: `avg_kib_s=149.52`
  - keep-alive ON: `avg_kib_s=147.06`
- matched warmed pair (cycles=6):
  - keep-alive ON: `avg_upload_s=3.45`, `avg_kib_s=148.58`
  - forced close: `avg_upload_s=3.45`, `avg_kib_s=148.30`

Interpretation:

- keep-alive fix is runtime-stable in bounded acceptance (no persistent
  connect-refused class observed in final paired runs).
- throughput impact is small and inconsistent across short runs; current signal
  indicates parity rather than a clear gain.

## 11.25 2026-03-04 Host Cross-Cycle Upload-Client Reuse (Bounded Result)

Host changes:

- added reusable direct-upload client APIs:
  - `make_direct_upload_client`
  - `upload_file_direct_fast_with_client`
- wired wifi-acceptance to optionally reuse one client across cycles via:
  - `HOSTCTL_NET_REUSE_UPLOAD_CLIENT=1`
  - default remains `0` (off) to avoid promoting a non-winning path.
- pooled client is dropped on upload failure or recovery path.

Bounded evidence:

- reuse-enabled 6-cycle run (strict reset guard, `max_delta=0`) hit one
  first-attempt send timeout in cycle 3:
  - `HOST_FAILURE class=host_transport_send_fail`
  - retry recovered upload, but guard failed on `req_read_body_reset delta=1`.
- reuse-enabled 6-cycle run (relaxed guard, `max_delta=2`) completed:
  - keep-alive ON: `avg_upload_s=3.64`, `avg_kib_s=142.61` (one slow send outlier)
  - forced close: `avg_upload_s=3.50`, `avg_kib_s=146.36`
- default mode (reuse off) sanity run (cycles=3) remained stable:
  - `avg_upload_s=3.49`, `avg_kib_s=146.90`

Decision:

- do not promote host cross-cycle client reuse as a throughput optimization.
- keep it as an opt-in diagnostic/experiment knob while primary optimization
  focus returns to firmware ingress empty-queue `read_wait_ms`.

## 11.26 2026-03-04 Firmware Ingress Empty-Queue Mitigation (Cooperative Fairness Yield)

Firmware change:

- added cooperative fairness yield in upload body read loop:
  - file: `src/firmware/storage/upload/http/connection/body.rs`
  - behavior: while draining immediately-ready socket reads, yield periodically
    (`HTTP_INGRESS_COOP_YIELD_BYTES` or `HTTP_INGRESS_COOP_YIELD_READS`;
    initial bounded run used `12 KiB` / `24`)
    so the net runner can execute and refill RX queue.
  - rationale: reduce starvation bursts in cooperative scheduling where
    back-to-back ready reads can delay network runner progress and amplify
    empty-queue read wait.

Validation runs (`HOSTCTL_UPLOAD_MODE=direct`, `HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS=0`, cycles=3):

- baseline (pre-change):
  - summary: `avg_upload_s=3.53`, `avg_kib_s=145.27`
  - last three upload stats `read_wait_ms`: `2368`, `2389`, `2493`
  - last three `ingress_read_wait_empty_q_ms`: `2362`, `2383`, `2483`
- post-change run A:
  - summary: `avg_upload_s=3.43`, `avg_kib_s=149.33`
  - `read_wait_ms`: `2304`, `2295`, `2293`
  - `ingress_read_wait_empty_q_ms`: `2302`, `2291`, `2285`
- post-change run B (confirmation):
  - summary: `avg_upload_s=3.42`, `avg_kib_s=149.57`
  - `read_wait_ms`: `2280`, `2397`, `2255`
  - `ingress_read_wait_empty_q_ms`: `2276`, `2391`, `2247`

Observed bounded delta:

- throughput (`avg_kib_s`): `145.27 -> 149.33/149.57` (`+2.8..+3.0%`)
- `read_wait_ms` average over compared samples:
  - pre: `2416.7`
  - post (6 samples): `2304.0` (`-4.7%`)

Interpretation:

- this firmware-side scheduler fairness tweak is a promising mitigation for the
  empty-queue ingress bottleneck in bounded runs.
- effect size is moderate and should be confirmed in longer-cycle/soak runs
  before declaring stable promotion.

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
