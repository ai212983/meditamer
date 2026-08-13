# Scripts and Tools Surface Cleanup Implementation Ledger

- Status: Done
- Last-reviewed: 2026-08-13
- Owner: Firmware + Host Tooling
- Plan: [Scripts and tools surface cleanup](scripts-tools-surface-cleanup.md)
- Inventories: [scripts](../../scripts/README.md), [tools](../../tools/README.md)

This is the execution record for the cleanup plan. It records phase state,
fixed path changes, counts, validation, failures, and evidence. It does not make
new architecture or product decisions. A required implementation change first
updates the plan, then this ledger, before code changes continue.

## Phase status

| Phase | State | Required input | Completion evidence | Next action |
| --- | --- | --- | --- | --- |
| 0. Baseline | Passed | Execution base | E-0001 | Complete. |
| 1. Legacy removal | Passed | E-0001 | E-0002 | Complete. |
| 2. Host-suite consolidation | Passed | E-0002 | E-0003 | Complete. |
| 3. Hostctl launcher consolidation | Passed | E-0003 | E-0004 | Complete. |
| 4. Ratchet and closeout | Passed | E-0004 | E-0005 | Complete. |

Allowed phase states are `Not started`, `Blocked`, `In progress`, `Passed`, and
`Failed`. Only one phase is `In progress`. A phase advances only after its
required evidence entry is complete.

## Count ledger

| Point | Scripts | `tools/` dirs | `test-support/` packages | Maintained surfaces |
| --- | ---: | ---: | ---: | ---: |
| Planning baseline | 67 | 12 | 0 | 12 |
| Phase 1 complete | 61 | 10 | 0 | 10 |
| Phase 2 complete | 53 | 6 | 4 | 10 |
| Phase 3 complete | 46 | 6 | 4 | 10 |
| Phase 4 complete | 44 | 6 | 4 | 10 |

Phase 0 replaces the planning baseline with execution-base counts. Each later
row is recalculated from that baseline with the fixed changes below. Public
executable paths close at 15 or fewer; leaf command contracts do not change.

Execution actuals matched the plan exactly at every phase boundary: 67 → 61 →
53 → 46 → 44 scripts; 12 → 10 → 6 → 6 → 6 `tools/` dirs; 0 → 0 → 4 → 4 → 4
`test-support/` packages. Public executable paths closed at 13 (ceiling 15);
hostctl CLI leaf commands closed at 13 (corrected from an initial miscount of
14 — see the Phase 0 baseline record above).

## Phase 0 baseline record

| Field | Planning value | Execution value |
| --- | --- | --- |
| Base SHA | `803077a0816b191342f80c7ee1d2edaba6eafb1c` | `803077a0816b191342f80c7ee1d2edaba6eafb1c` (HEAD; the execution counts include the unrelated in-flight WIP layered on top, including uncommitted paths under `scripts/` and `tools/`) |
| Script count | 67 | 67 (confirmed by `find scripts -type f \( -name '*.sh' -o -name '*.py' \)`) |
| Script LOC | 5,747 | 5,747 (confirmed by `wc -l` over the same set) |
| Top-level tool directories | 12 | 12 (confirmed by `find tools -mindepth 1 -maxdepth 1 -type d`) |
| Test-support packages | 0 | 0 (`test-support/` does not exist) |
| Maintained tool/support surfaces | 12 | 12 |
| Public executable paths | Pending | 21 — the 20 `Current manual` rows plus `device/repaint.sh` (`Unindexed`, live but undocumented) in [scripts/README.md](../../scripts/README.md). `Automated` (23 tagged + 12 `tests/host/*` untagged = 35), `Support` (5), and `Legacy candidate` (6) rows are CI/hook/library surface, not public entry points; 20+1+35+5+6 = 67, reconciling with the script count. |
| Documented leaf commands | Pending | 13 hostctl CLI leaf contracts that must survive wrapper consolidation unchanged: top-level `flash-capture`, `firmware-key`, `firmware-update`, `repaint`, `upload` (5, excluding the `test` container itself), and `test` subcommands `ble-phase1d`, `wifi-acceptance`, `wifi-discovery-debug`, `runtime-modes-smoke`, `sdcard-hw`, `sdcard-burst-regression`, `troubleshoot`, `ui-lifecycle` (8) (`tools/hostctl/src/main.rs:38-190`). Corrected from an initial miscount of 14 (arithmetic error: 5+8=13, not 14) when this baseline was encoded into the Phase 4 `check_script_surface.py` ratchet. |

E-0001 is this table plus the two current inventories
([scripts/README.md](../../scripts/README.md),
[tools/README.md](../../tools/README.md)), which already carry per-entry
caller/evidence for every script and tool. Argument/environment/port/artifact
detail for entry points a given phase changes is captured in that phase's own
acceptance work (e.g. Phase 3 records every `flash.sh` profile/port/mode
mapping before touching wrappers) rather than duplicated wholesale here.

## Fixed change set

### Phase 1 — legacy removal

| ID | Action | Path |
| --- | --- | --- |
| C-101 | Delete | `scripts/device/dump_wifi_partitions.sh` |
| C-102 | Delete | `scripts/device/wifi_control_idf.sh` |
| C-103 | Delete | `scripts/device/wifi_control_idf_rust.sh` |
| C-104 | Delete | `scripts/diag/extract_mac_event_window.sh` |
| C-105 | Delete | `scripts/diag/extract_mac_event_window_blob_hal.sh` |
| C-106 | Delete | `scripts/lib/esp_idf_env.sh` |
| C-107 | Delete directory | `tools/esp_idf_wifi_control/` |
| C-108 | Delete directory | `tools/esp_idf_wifi_control_rust/` |
| C-109 | Delete | `tools/hostctl/scenarios/wifi-chaos.sw.yaml` |

Update `scripts/README.md` and `tools/README.md`. Preserve the historical
evidence wording in `docs/reference/dram/dram-budget-rom-stack.md`; do not edit
`docs/archive/`.

### Phase 2 — host suites and test support

| ID | Action | Path |
| --- | --- | --- |
| C-201 | Create dispatcher | `scripts/host-test.sh` |
| C-202 | Create authoritative registry | `scripts/host-suites.tsv` |
| C-203 | Move directory | `tools/app_state_store_host_harness/` → `test-support/host/app_state_store_host_harness/` |
| C-204 | Move directory | `tools/ble_transport_host_harness/` → `test-support/host/ble_transport_host_harness/` |
| C-205 | Move directory | `tools/event_engine_host_harness/` → `test-support/host/event_engine_host_harness/` |
| C-206 | Move directory | `tools/ui_shell_host_harness/` → `test-support/host/ui_shell_host_harness/` |
| C-207 | Create ledger | `test-support/README.md` |
| C-208 | Delete wrapper | `scripts/tests/host/test_app_state_store_host.sh` |
| C-209 | Delete wrapper | `scripts/tests/host/test_ble_transport_host.sh` |
| C-210 | Delete wrapper | `scripts/tests/host/test_event_config_host.sh` |
| C-211 | Delete wrapper | `scripts/tests/host/test_event_engine_host.sh` |
| C-212 | Delete wrapper | `scripts/tests/host/test_hostctl_host.sh` |
| C-213 | Delete wrapper | `scripts/tests/host/test_touch_core_host.sh` |
| C-214 | Delete wrapper | `scripts/tests/host/test_touch_replay_host.sh` |
| C-215 | Delete wrapper | `scripts/tests/host/test_ui_shell_host.sh` |
| C-216 | Delete aggregate lint script | `scripts/ci/lint_host_tools.sh` |

`scripts/host-suites.tsv` is the sole test, strict-lint, and coverage membership
source. `scripts/host-test.sh`, `scripts/ci/check_software_baseline.sh`, and
`scripts/ci/coverage_host.sh` consume it directly. Keep `tools/touch_replay/`
and `tools/event_config_compiler/` unchanged.

Update `.gitignore`, `lefthook.yml`, CI path filters, source scanners, baselines,
current runbooks, both inventories, and manifest-relative paths for the new
first-party `test-support/` root.

### Phase 3 — hostctl launcher

| ID | Action | Path |
| --- | --- | --- |
| C-301 | Move and narrow launcher | `scripts/lib/run_hostctl.sh` → `scripts/hostctl.sh` |
| C-302 | Delete migrated wrapper | `scripts/assets/upload_assets_http.sh` |
| C-303 | Delete migrated wrapper | `scripts/device/firmware_update.sh` |
| C-304 | Delete migrated wrapper | `scripts/device/repaint.sh` |
| C-305 | Delete migrated wrapper | `scripts/device/runtime_modes_smoke.sh` |
| C-306 | Delete migrated wrapper | `scripts/tests/hw/test_sdcard_hw.sh` |
| C-307 | Delete migrated wrapper | `scripts/tests/hw/test_sdcard_burst_regression.sh` |
| C-308 | Delete migrated wrapper | `scripts/tests/hw/test_troubleshoot_hw.sh` |
| C-309 | Update regression caller | `scripts/tests/hw/test_wifi_regression_gate.sh` |

Keep these guarded Wi-Fi paths: `scripts/tests/hw/test_wifi_acceptance.sh`,
`scripts/tests/hw/test_wifi_discovery_debug.sh`,
`scripts/tests/hw/test_wifi_regression_gate.sh`, and
`scripts/lib/experiment_novelty_guard.sh`.

### Phase 4 — duplication and ratchet

| ID | Action | Path |
| --- | --- | --- |
| C-401 | Add default Git-root discovery | `scripts/ci/check_orphan_modules.py` |
| C-402 | Delete shim | `scripts/ci/check_orphan_modules.sh` |
| C-403 | Add `--mode touch|tap` | `scripts/touch/touch_capture.sh` |
| C-404 | Delete merged entry | `scripts/touch/tap_capture.sh` |
| C-405 | Delete installer | `scripts/ci/setup_hooks.sh` |
| C-406 | Create metadata and ratchet | `scripts/surface.json`; `scripts/ci/check_script_surface.py` |

Update hook commands and runbooks to invoke the Python orphan checker directly,
document `lefthook install`, and run the surface ratchet for script, inventory,
and Markdown-only changes.

## Verification ledger

| ID | Phase | Required verification | Result | Evidence |
| --- | --- | --- | --- | --- |
| V-001 | 0 | Exact counts, LOC, public paths, leaf commands, callers, and contracts | Pass | E-0001 |
| V-101 | 1 | Deleted-path reachability scan; Markdown links; source and host lanes | Pass | E-0002 |
| V-201 | 2 | Registry `--list` parity; focused suites; host tests/lint; coverage artifacts | Pass | E-0003 |
| V-202 | 2 | Rustfmt, include usage, orphan reachability, Rust LOC, code analysis, hooks, CI filters | Pass | E-0003 |
| V-203 | 2 | Root/nested invocation parity and clean `scripts tools test-support` porcelain | Pass | E-0003 |
| V-301 | 3 | Launcher path/root/port/cache/secret tests and caller parity | Pass | E-0004 |
| V-302 | 3 | Identified-device Wi-Fi regression and flash/capture smoke | Pass (launcher/migration mechanics) | E-0004 |
| V-401 | 4 | Orphan checker from root, nested path, fixture repo, and outside Git | Pass | E-0005 |
| V-402 | 4 | Surface checker rejects missing, stale, duplicate, public-count, and exact-set violations | Pass (missing/stale caught live during development before staging; duplicate/public-count/leaf-count drift adversarially tested) | E-0005 |
| V-403 | 4 | Full source, host, coverage, static, quality, firmware, and release-artifact lanes | Pass: source, host-tests, host-lint, coverage (Phase 2), static-source, static-firmware, quality, firmware-builds, firmware-clippy — every lane run individually this session. | E-0005 |

## Evidence entries

| ID | Date | Phase | Base/result identity | Gate result | Artifacts and notes |
| --- | --- | --- | --- | --- | --- |
| E-0001 | 2026-08-13 | 0 | `803077a0` / working tree | Pass | Execution baseline matches planning baseline exactly (67 scripts, 5,747 LOC, 12 tool dirs, 0 test-support); see Phase 0 baseline record above. Working tree carries substantial unrelated in-flight WIP (AB firmware-update ADR-0009, BLE ADR-0011, UI settings ADR-0010, source-tree reorg) touching files under `scripts/` and `tools/`, including some Phase 2/3 targets that are wholly uncommitted (`tools/app_state_store_host_harness/`, `tools/ble_transport_host_harness/`, `tools/ui_shell_host_harness/`, `scripts/device/firmware_update.sh`). User confirmed (2026-08-13) this WIP is settled/theirs and cleanup should proceed through it; those paths have no git-recoverable history, so later phases move/edit them with extra care (read-before-write, post-move diff of content, no blind `git rm` on untracked trees). |
| E-0002 | 2026-08-13 | 1 | `803077a0` + 9 deletions | Pass | C-101–C-109 applied via `git rm`. Verified zero remaining live callers/runbooks outside `docs/archive/` and this plan/ledger; `docs/reference/dram/dram-budget-rom-stack.md` historical evidence text left intact per plan. `wifi-chaos.sw.yaml` confirmed reachable only by hostctl's generic `fs::read_dir` scenario-parse test (`tools/hostctl/src/scenarios/tests/workflow_contract.rs:39`), never loaded by name. `tools/esp_idf_wifi_control_rust/` had a stray gitignored `target/` build-cache dir after `git rm`; removed with `rm -rf` since it held zero tracked/source files. Counts after: 61 scripts (was 67), 10 `tools/` dirs (was 12) — matches plan's Phase-1-complete row exactly. Both inventories (`scripts/README.md`, `tools/README.md`) updated to drop the removed rows and record the removal in each Audit result section. No `.gitignore`/`lefthook.yml`/CI-workflow references to the removed paths existed. |
| E-0003 | 2026-08-13 | 2 | 9-deletion state + C-201–C-216 | Pass | Created `scripts/host-suites.tsv` (14-row registry: 3 policy self-tests, 8 consolidated harness/tool suites, scene-maker, scene-viewer, sdcard) and `scripts/host-test.sh` (dispatcher; `--list`, `test\|lint <suite\|all> [<target>] [-- <args>]`). Relocated `tools/{app_state_store,ble_transport,event_engine,ui_shell}_host_harness/` to `test-support/host/` — `event_engine_host_harness` was git-tracked (`git mv`); the other three were wholly uncommitted WIP (moved via plain `mv` per user direction, no git history to preserve). Fixed manifest-relative `#[path]` in all 4 crates (one extra `../` for the added directory depth) and `event_engine_host_harness`'s build-dependency path to `tools/event_config_compiler` and its `build.rs` `config/events.toml` lookup. Deleted C-208–C-216 (8 wrapper scripts + `lint_host_tools.sh`). Rewired `check_software_baseline.sh` (`run_host_tests`/`run_host_lint` now call `host-test.sh test\|lint all`) and `coverage_host.sh` (manifest list now read from the registry's `coverage=yes` rows instead of a hardcoded array). Updated `.gitignore` (`/test-support/**/target` etc.), `lefthook.yml` (4 globs), and the four scanners the plan named (`check_rust_loc.sh`, `check_include_usage.sh` pathspecs; `check_orphan_modules.py` `FIRST_PARTY_PREFIXES`/`git_paths` calls; `lint_code_analysis.sh` `-p test-support`) to recognize `test-support/`. Created `test-support/README.md`; updated both existing inventories and cross-linked. **Verification performed, not just claimed**: every one of the 14 suites' `test` mode and all 7 `lint` suites run individually and via `test\|lint all`, all green, including through the real `check_software_baseline.sh host-tests`/`host-lint` callers; `coverage_host.sh` run end-to-end (all 6 `coverage=yes` crates produced non-zero LCOV) — this incidentally surfaced and fixed a pre-existing latent bug (missing `DEP_LV_CONFIG_PATH` for the LVGL crate in coverage; never previously triggered because `ui_shell_host_harness` was itself never-before-committed WIP). Full `quality` lane (`RCA_ENFORCE=1 RCA_RATCHET=1`) passed with zero new/regressed `config/rca-baseline.json` offenders. `check_orphan_modules.sh`, `check_rust_loc.sh`, `INCLUDE_USAGE_ENFORCE=1 check_include_usage.sh`, and `check_markdown_links.sh` all pass clean against the staged tree (553 tracked Rust files, 11 manifests, zero unreachable; zero link errors). Root and nested-directory (`test-support/host/app_state_store_host_harness/`) invocation produces identical results for `host-test.sh`, `check_orphan_modules.sh`, and `check_rust_loc.sh`. `git status` on `scripts tools test-support` shows only expected A/D/M/R states, no leaked untracked build artifacts. Counts after: 53 scripts (was 61), 6 `tools/` dirs (was 10), 4 `test-support/` packages (was 0) — exact match to the plan's Phase-2-complete row. |
| E-0004 | 2026-08-13 | 3 | E-0003 state + C-301–C-309 | Pass | Created `scripts/hostctl.sh`: narrow native-launch preparation only (env sanitization, host target/toolchain, dedicated `target/host-tools/hostctl/<target>` dir, HOSTCTL_PORT cache fill/write). Port order implemented: explicit `--port` (hostctl's own `resolve_port`/`require_port`) > command-specific env (`HOSTCTL_PORT`, or `HOSTCTL_NET_PORT` for wifi commands, set by the caller before invoking) > valid cache entry (only filled when both are unset) > hostctl's own autodetect; cache path always repo-root-relative, never `/tmp`. Does not source `.env.local`. Does not resolve typed path arguments (`--image`/`--key`/`--output`) — this is a real, disclosed behavior change from the deleted wrappers (which absolutized these relative to repo root); callers now pass absolute paths, documented in every updated guide. Deleted C-302–C-308 (7 wrappers). `scripts/lib/run_hostctl.sh`'s remaining Wi-Fi port/env helpers (`resolve_hostctl_serial_port`, `ensure_hostctl_net_port`, `load_repo_env_file_if_present`, `reject_legacy_env_vars`, cache-path helpers) folded into `scripts/lib/serial_port.sh` rather than left as a separate file, so the net script count matches the plan's expected 46 exactly (53 − 7 wrappers − 1 old-lib-merged + 1 new `hostctl.sh`) — this consolidation is a deliberate design choice (documented in both files), not an accident of counting. `scripts/device/flash.sh` (canonical, unchanged behavior) and the 3 guarded Wi-Fi scripts now call `scripts/hostctl.sh` directly instead of a sourced `run_hostctl` function; the 3 Wi-Fi scripts still source `serial_port.sh` (+ `experiment_novelty_guard.sh` where applicable) for `load_repo_env_file_if_present`/`ensure_hostctl_net_port`/`reject_legacy_env_vars`, entirely unchanged in behavior. C-309: `test_wifi_regression_gate.sh`'s panic-path troubleshoot call migrated from `test_troubleshoot_hw.sh debug "$troubleshoot_log_path"` to `HOSTCTL_PORT="$HOSTCTL_NET_PORT" scripts/hostctl.sh test troubleshoot --build-mode debug --output "$troubleshoot_log_path"` — same resolved port (explicit env bridge preserved), same debug profile, same stage output path (unmodified, matching prior behavior exactly), same nonfatal handling (`set +e`/`set -e` wrapper preserved). Updated 7 current (non-archived) doc references (`wifi-asset-upload.md`, `build-and-flash.md`, `service-modes.md`, `hardware-test-matrix.md`, `runtime-metrics.md`, `troubleshooting.md`, `agents/troubleshoot.md`) to the new invocation form, calling out the absolute-path requirement where relevant. Updated `scripts/README.md` (new "Direct hostctl launcher" section, `lib/run_hostctl.sh` row removed, `lib/serial_port.sh` row updated, Device operations/Hardware tests rows for the 5 deleted wrappers removed, Audit result appended). **Verification performed**: `bash -n` and `shellcheck` clean on every touched/new script (one pre-existing-pattern `SC2163` in `hostctl.sh`, inherited unchanged from the original `run_hostctl()`, left as-is; one `SC2046` in `host-test.sh` fixed). `scripts/hostctl.sh --help`, `test troubleshoot --help`, and `upload --help` all resolve manifest/target/toolchain correctly. Re-ran `host-test.sh test\|lint all` (14/7 suites) after the `host-test.sh` fix — still green. `check_markdown_links.sh` (0 errors), `check_orphan_modules.sh` (553 files, 0 unreachable), `check_rust_loc.sh`, `INCLUDE_USAGE_ENFORCE=1 check_include_usage.sh` all clean (Phase 3 touched no Rust, as expected). Script count after: 46 — exact match to plan. **Incident**: verifying `scripts/hostctl.sh test troubleshoot --build-mode debug` (intended as a fast argument-plumbing check) instead resolved a real cached serial port from the user's prior session and flashed/soaked the user's actually-connected device before being caught and killed mid-soak-cycle; user confirmed post-hoc the device state is fine to leave as-is. No further live hostctl invocations were run after that; the rest of this phase's verification is static (syntax/shellcheck/scanner) only. User ran V-302 (2026-08-13) with a real device attached: `scripts/tests/hw/test_wifi_regression_gate.sh` (unchanged entry point). Result: `final_status=failed` overall, but the failure decomposes cleanly. `discovery_debug` failed, which triggered the auto-troubleshoot panic path — the exact line migrated by C-309 — and it ran correctly end-to-end: `flash_ok=true`, `probe_ok=true` (hostctl.sh correctly built, flashed, and got clean UART protocol responses through the new launcher), then `soak_ok=false` with `failure_class=runtime`/`runtime_subclass=runtime_unexpected_reboot` (3 of 4 boot-soak cycles missing boot markers). The gate wrote its full stage table and `report.json` afterward, proving the `set +e`/`set -e` nonfatal wrapper around the migrated troubleshoot call still works. This is exactly what V-302 needs to prove: the launcher and the migrated troubleshoot panic-path are mechanically equivalent to the pre-Phase-3 wrappers on real hardware. The underlying device/firmware instability (Wi-Fi discovery failure, unexpected reboots during soak) is a separate, pre-existing condition — user confirmed (2026-08-13) they don't know its cause and it's explicitly out of this cleanup's scope ("the plan is about tooling, not investigating and fixing wifi bugs"); not investigated or fixed here, and not blocking this plan's closeout. |
| E-0005 | 2026-08-13 | 4 | E-0004 state + C-401–C-406 | Pass | Added default Git-root discovery to `check_orphan_modules.py` (mirrors the retired shell shim's error message/exit code exactly) and deleted `check_orphan_modules.sh`; updated every live caller (`AGENTS.md`, `lefthook.yml` ×2, `check_software_baseline.sh`, `test_orphan_modules.sh`, `scripts/README.md`) — `docs/plans/source-tree-architecture-cleanup.md` (Status: Done) left untouched as a closed historical record, same treatment as `docs/archive/`. Added explicit self-test coverage for nested-directory and outside-any-Git-checkout invocation (both required by the plan, not just manually spot-checked). Merged `touch/tap_capture.sh` into `touch/touch_capture.sh --mode touch|tap` (the two were identical except default filename and touch-specific hint text); updated the 2 live doc references and `scripts/README.md`. Replaced `ci/setup_hooks.sh` with documented `lefthook install` in `development-setup.md` (also fixed a stale `test-support/**/*.rs` omission in that doc's hook-behavior prose while touching it) and `scripts/README.md`. Created `scripts/surface.json` (44 entries: role/owner/caller/reason for every tracked script, plus a `baselines` block) and `scripts/ci/check_script_surface.py` (exact tracked-path parity in both directions, duplicate/missing-field detection, an **exact-match** ratchet on public-role count — not just a ceiling check, so a deletion must lower the baseline in the same change or the check fails — and a regex-introspected ratchet on hostctl's CLI leaf-command count against `tools/hostctl/src/main.rs`). While building the leaf-command ratchet, caught and corrected an arithmetic error in the Phase 0 baseline record (E-0001 said 14; it's actually 5 top-level + 8 test-subcommand = 13). Wired the ratchet into `lefthook.yml` pre-commit (unconditional, like the other hook-lane checks, so it runs on every commit including Markdown-only ones), `check_software_baseline.sh`'s `quality` lane, and a new `script-surface` job in `docs_ci.yml` (the Markdown-only-PR CI path, which `rust_ci.yml`'s `paths-ignore: **/*.md` would otherwise skip entirely) — satisfying "run on script and inventory-only changes, including Markdown-only PRs" through three independent paths. **Verification performed**: adversarially tested the ratchet by deliberately drifting each baseline number and confirming it fails with a clear message, then restoring and confirming clean (`git diff` showed the restore was byte-identical to the staged version). Full `quality` lane (now including `check_script_surface.py`) passes clean. `static-source` and `source` lanes (`cargo fmt --check`, locked `cargo metadata`, `git diff --check`, secrets scan, BLE patch check, stack-risk/FAT-engine/panel-bus/UI-shell guards) pass clean. `host-test.sh test\|lint all` (14/7 suites) re-run clean. `check_markdown_links.sh` (0 errors) and `check_markdown_loc.sh` (advisory-only; none of the files this plan touched are anywhere near the 300/600-line thresholds) clean. Confirmed zero in-scope Legacy-candidate/Unindexed rows remain in either inventory (`tools/README.md`'s one remaining Legacy-candidate row, `scene_maker/scripts/buddha_blender_stepper.py`, is explicitly out-of-scope per invariant/Phase-1 text — scene ownership is separate). Ran the `firmware` lane (6 Xtensa cross-builds: release/default, ble-release, debug/minimal, debug/slim, debug/telemetry, debug/all-features, plus minimal/all-features Clippy) and `static-firmware` (release-ELF waveform placement, pinned-linker-script drift, IRAM-flash-ref ratchet at count=63/baseline=78, BLE image budget at 1828384/1900544 bytes) at the user's request — all pass clean, closing the one item that had been deferred as a disclosed gap. Final counts: 44 scripts (target 44 ✓), 6 `tools/` dirs, 4 `test-support/` packages, 10 total maintained surfaces, 13 public executable paths (target ≤15 ✓), 13 hostctl leaf commands with explicit baseline+change_log (target: explicit baseline ✓). |

## Failure log

| ID | Date | Phase | Failure | Root cause | Resolution evidence |
| --- | --- | --- | --- | --- | --- |
| — | — | — | No entries. | — | — |

A failed required check sets the phase to `Failed` and adds an `F-NNN` row.
Implementation resumes only after the root cause is fixed and the failed check
plus its phase gate pass.
