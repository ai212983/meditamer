# Script inventory

Audited 2026-08-13 against the current working tree. This ledger records static
repository evidence: CI and hook invocation, calls from another script, build
configuration, and current (non-archived) documentation. It does not claim to
measure commands run outside the repository.

Status meanings:

- **Automated** — invoked by CI, Git hooks, Cargo/build configuration, or the
  aggregate software baseline.
- **Current manual** — exposed by a current guide or active operator workflow,
  but not automatically run on every change.
- **Support** — sourced or called by another current script/tool.
- **Unindexed** — the implementation is current and callable, but no current
  documentation or caller was found.
- **Legacy candidate** — no current caller or runbook was found; evidence is
  confined to archived investigations, or to another legacy candidate.

The status describes reachability, not quality or permission to delete. Hardware
scripts remain manual because they require a board, serial port, credentials, or
human interaction.

## Direct hostctl launcher

| Script | Status | Purpose and evidence |
| --- | --- | --- |
| [`hostctl.sh`](hostctl.sh) | Current manual | Advanced explicit-mode `hostctl` launcher; Cargo starts in `/tmp`, then the CLI restores the repository-root runtime directory so relative inputs and evidence paths remain stable. `scripts/device/flash.sh` remains the canonical flash-and-capture wrapper. |

## Build

| Script | Status | Purpose and evidence |
| --- | --- | --- |
| [`build/build.sh`](build/build.sh) | Automated | Canonical firmware build/Clippy entry point; called by the aggregate baseline and hostctl flash-capture. |
| [`build/ota_bootloader.sh`](build/ota_bootloader.sh) | Support | Builds the pinned ESP-IDF A/B bootloader; called by the Xtensa runner and hostctl flash path. |
| [`build/xtensa_runner.sh`](build/xtensa_runner.sh) | Automated | Cargo runner configured in `.cargo/config.toml`; flashes with the A/B bootloader and partition table. |

## CI and repository checks

| Script | Status | Purpose and evidence |
| --- | --- | --- |
| [`ci/check_ble_controller_patch.sh`](ci/check_ble_controller_patch.sh) | Automated | Verifies the bounded vendored BLE transport and ESP32 ISR queue-validation patch; aggregate `source` lane. |
| [`ci/check_ble_image_budget.sh`](ci/check_ble_image_budget.sh) | Automated | Enforces the BLE firmware image ceiling; aggregate `static-firmware` lane. |
| [`ci/check_commit_message.sh`](ci/check_commit_message.sh) | Automated | Conventional commit-message hook in `lefthook.yml`. |
| [`ci/check_fat_engine_stackless.sh`](ci/check_fat_engine_stackless.sh) | Automated | Rejects async, heap-backed, or recursively advanced FAT-engine state; aggregate `static-source` lane. |
| [`ci/check_include_usage.sh`](ci/check_include_usage.sh) | Automated | Inventories/enforces generated-only `include!`; hooks and aggregate quality lane. |
| [`ci/check_iram_flash_refs.sh`](ci/check_iram_flash_refs.sh) | Automated | Ratchets IRAM references into flash-mapped data; aggregate `static-firmware` lane. |
| [`ci/check_markdown_links.sh`](ci/check_markdown_links.sh) | Automated | Checks staged or all live Markdown links; pre-commit hook. |
| [`ci/check_markdown_loc.sh`](ci/check_markdown_loc.sh) | Automated | Advisory Markdown length check; hook, docs CI, and aggregate quality lane. |
| [`ci/check_orphan_modules.py`](ci/check_orphan_modules.py) | Automated | Rust source reachability analyzer; hooks and aggregate quality lane. Defaults `--repo-root` via `git rev-parse --show-toplevel`; the shell entry point was removed by Phase 4 (C-401/C-402). |
| [`ci/check_panel_bus_gating.sh`](ci/check_panel_bus_gating.sh) | Automated | Guards panel-bus suspend/resume ownership; aggregate `static-source` lane. |
| [`ci/check_panel_waveform_placement.sh`](ci/check_panel_waveform_placement.sh) | Automated | Checks release-ELF waveform symbol placement; aggregate `static-firmware` lane. |
| [`ci/check_pinned_linker_scripts.sh`](ci/check_pinned_linker_scripts.sh) | Automated | Detects drift in the pinned ESP32 linker override; aggregate `static-firmware` lane. |
| [`ci/check_rust_loc.sh`](ci/check_rust_loc.sh) | Automated | Advisory Rust raw-line check; pre-commit and aggregate quality lane. |
| [`ci/check_script_surface.py`](ci/check_script_surface.py) | Automated | Ratchets `scripts/surface.json` against the tracked script surface, the public-executable-path count, and the hostctl CLI leaf-command count; unconditional pre-commit hook, docs-only CI, and aggregate quality lane (so Markdown-only PRs are covered too). |
| [`ci/check_secrets.sh`](ci/check_secrets.sh) | Automated | Scans tracked/staged content for secrets; hook, CI, and aggregate source lane. |
| [`ci/check_software_baseline.sh`](ci/check_software_baseline.sh) | Automated | Aggregate source, host, firmware, static, and quality lane dispatcher used by CI and hooks. |
| [`ci/check_stack_risk.sh`](ci/check_stack_risk.sh) | Automated | Flags large firmware local arrays; aggregate `static-source` lane. |
| [`ci/check_ui_shell_ownership.sh`](ci/check_ui_shell_ownership.sh) | Automated | Enforces UI-shell/backend ownership boundaries; aggregate `static-source` lane. |
| [`ci/coverage_host.sh`](ci/coverage_host.sh) | Automated | Produces merged host LCOV for harnesses and hostctl; PR CI and optional Sonar input. |
| [`ci/lint_code_analysis.sh`](ci/lint_code_analysis.sh) | Automated | Enforces the rust-code-analysis SLOC/complexity ratchet; hooks and aggregate quality lane. |
| [`ci/lint_rust_analyzer.sh`](ci/lint_rust_analyzer.sh) | Automated | Runs the repository rust-analyzer baseline; aggregate quality lane. |
| [`ci/sonar_scan.sh`](ci/sonar_scan.sh) | Current manual | Local SonarQube scan and quality-gate poller; documented by the development-setup guide. |

## Device operations

| Script | Status | Purpose and evidence |
| --- | --- | --- |
| [`device/cold_boot_matrix.sh`](device/cold_boot_matrix.sh) | Current manual | Human-assisted reset-button boot-path matrix; current hardware matrix, troubleshooting guide, and archived validation record. |
| [`device/flash.sh`](device/flash.sh) | Current manual | Canonical flash-and-boot-capture wrapper; mandated by `AGENTS.md` and current guides. |
| [`device/generate_firmware_signing_key.sh`](device/generate_firmware_signing_key.sh) | Current manual | Creates a protected signing seed and prints its public key through hostctl; current build/flash guide. |
| [`device/monitor.sh`](device/monitor.sh) | Current manual | Passive serial monitor/capture helper; current guides and touch capture wrappers. |
| [`device/soak_boot.sh`](device/soak_boot.sh) | Current manual | Repeated reset/boot-marker soak; current troubleshooting and DRAM guidance. |
| [`device/soak_refresh.sh`](device/soak_refresh.sh) | Current manual | Long refresh/panic soak; current troubleshooting guide and hardware matrix. |

## Shared libraries

| Script | Status | Purpose and evidence |
| --- | --- | --- |
| [`lib/experiment_novelty_guard.sh`](lib/experiment_novelty_guard.sh) | Support | Enforces the archived Wi-Fi/upload decision ledger before current acceptance/regression runs. |
| [`lib/serial_port.sh`](lib/serial_port.sh) | Support | Shared macOS/Linux serial-port discovery, plus the hostctl port-cache/env helpers used by the guarded Wi-Fi acceptance/discovery/regression scripts (folded in from `lib/run_hostctl.sh`). |

## Host test/lint dispatcher

| Script | Status | Purpose and evidence |
| --- | --- | --- |
| [`host-test.sh`](host-test.sh) | Automated | Authoritative host test/lint dispatcher; `ci/check_software_baseline.sh` (`host-tests`/`host-lint`) and `ci/coverage_host.sh` all drive suites through `host-suites.tsv`. Also a current manual entry point: `scripts/host-test.sh <test\|lint> <suite\|all> [<host-target>] [-- <args>]`. |

`host-suites.tsv` is the registry (not a script; not counted in the script
inventory). It is the sole test/lint/coverage membership source — see
[test-support/README.md](../test-support/README.md) for the packages it
covers outside `tools/`.

## Host tests

All scripts in this section are **Automated** by the aggregate `host-tests`
lane in `ci/check_software_baseline.sh`. The eight per-harness/tool wrappers
previously listed here (app-state, BLE transport, event-config, event-engine,
hostctl, touch-core, touch-replay, UI-shell) were consolidated into
`host-test.sh` by Phase 2 of the
[scripts and tools surface cleanup](../docs/archive/host-tooling/scripts-tools-surface-cleanup.md)
(change set C-201/C-202/C-208 through C-215); their suites are unchanged, just
reachable via `scripts/host-test.sh test <suite>` instead of a dedicated
script. The three self-tests below remain standalone since they test other
CI scripts directly, not a harness crate.

| Script | Purpose |
| --- | --- |
| [`tests/host/test_check_stack_risk.sh`](tests/host/test_check_stack_risk.sh) | Self-tests the stack-risk guard (part of the `static-source` lane, not `host-tests`). |
| [`tests/host/test_code_analysis_guard.sh`](tests/host/test_code_analysis_guard.sh) | Self-tests code-analysis enforcement and ratcheting. |
| [`tests/host/test_include_usage.sh`](tests/host/test_include_usage.sh) | Self-tests the generated-only `include!` policy. |
| [`tests/host/test_orphan_modules.sh`](tests/host/test_orphan_modules.sh) | Self-tests Rust source reachability detection. |

## Hardware tests

| Script | Status | Purpose and evidence |
| --- | --- | --- |
| [`tests/hw/test_wifi_acceptance.sh`](tests/hw/test_wifi_acceptance.sh) | Current manual | Hostctl Wi-Fi/upload acceptance workflow; current network guides and regression gate. |
| [`tests/hw/test_wifi_discovery_debug.sh`](tests/hw/test_wifi_discovery_debug.sh) | Current manual | Hostctl discovery diagnostic; current Wi-Fi guides and regression gate. |
| [`tests/hw/test_wifi_regression_gate.sh`](tests/hw/test_wifi_regression_gate.sh) | Current manual | Surviving Wi-Fi regression gate mandated by `AGENTS.md` for network changes. |

## Touch capture

| Script | Status | Purpose and evidence |
| --- | --- | --- |
| [`touch/make_touch_fixture.sh`](touch/make_touch_fixture.sh) | Current manual | Converts a device capture into replay inputs; documented by the touch-replay tool. |
| [`touch/touch_capture.sh`](touch/touch_capture.sh) | Current manual | Captures raw/decoded touch traces (`--mode touch`, default) or serial tap/event-engine traces (`--mode tap`); documented by the touch-replay tool and the event-engine guide. Merged from the former `tap_capture.sh` (Phase 4, C-403/C-404). |

## Audit result

Phase 1 of the [scripts and tools surface cleanup](../docs/archive/host-tooling/scripts-tools-surface-cleanup.md)
removed the six closed Wi-Fi investigation scripts previously listed here (the
two standalone ESP-IDF Wi-Fi comparator wrappers, their environment library,
the Wi-Fi partition dumper, and the two MAC-window extractors); see
`docs/archive/host-tooling/scripts-tools-surface-cleanup-ledger.md` change set C-101 through
C-106 for the record. `device/repaint.sh` (retirement candidate at that point,
since no runbook referenced it) was resolved by Phase 3: removed in favor of
direct `hostctl.sh repaint --command <cmd>`.

Phase 2 replaced the eight per-suite host wrappers and `ci/lint_host_tools.sh`
with the `host-test.sh`/`host-suites.tsv` dispatcher and registry (C-201
through C-216), and relocated four test-only Cargo packages to
`test-support/host/` (see that directory's README).

Phase 3 promoted `lib/run_hostctl.sh`'s launch capability to top-level
`hostctl.sh` (folding its remaining Wi-Fi port/env helpers into
`lib/serial_port.sh`) and removed seven wrappers now redundant with direct
`hostctl.sh` invocation: `assets/upload_assets_http.sh`,
`device/firmware_update.sh`, `device/repaint.sh`,
`device/runtime_modes_smoke.sh`, `tests/hw/test_sdcard_hw.sh`,
`tests/hw/test_sdcard_burst_regression.sh`, and
`tests/hw/test_troubleshoot_hw.sh` (C-301 through C-309). The retained Wi-Fi
regression gate's panic-path troubleshoot call now invokes `hostctl.sh`
directly with the same resolved port, debug profile, stage output path, and
nonfatal handling.

Phase 4 removed `ci/check_orphan_modules.sh` (folding default Git-root
discovery into the `.py` implementation), merged `touch/tap_capture.sh` into
`touch/touch_capture.sh --mode touch|tap`, and removed `ci/setup_hooks.sh` in
favor of documented `lefthook install` (C-401 through C-405). It also added
`surface.json` (per-script role/owner/caller/reason metadata; not a script
itself, not counted in this inventory) and its ratchet, `ci/check_script_surface.py`
(C-406), closing the plan at the exact target: 44 tracked scripts, 13 public
executable paths (ceiling 15), 13 hostctl CLI leaf commands.
