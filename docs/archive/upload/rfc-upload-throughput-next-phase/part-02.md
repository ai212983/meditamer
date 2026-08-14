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

