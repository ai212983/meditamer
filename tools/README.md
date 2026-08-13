# Tool inventory

Audited 2026-08-13 against the current working tree. “Used” here means a tool
is a production build dependency, is invoked by CI/hooks/current wrappers, or
has a current non-archived operator runbook. This is static reachability, not
shell-history telemetry.

Status meanings:

- **Production** — required while building or operating the firmware.
- **Automated test support** — exercised by the current host baseline, lint, or
  coverage lanes.
- **Maintained standalone** — tested/documented, but no firmware consumer was
  found.
- **Legacy candidate** — no current operational caller was found; remaining
  evidence is historical or archived.

## Top-level tools

| Tool | Status | Purpose and evidence |
| --- | --- | --- |
| [`event_config_compiler/`](event_config_compiler/) | Production | Root `build.rs` build-dependency that compiles `config/events.toml`; also host-tested, linted, and covered. |
| [`hostctl/`](hostctl/) | Production | Primary device orchestration, flashing, capture, upload, firmware-update, and hardware-test CLI; current wrappers, guides, tests, lint, and coverage all depend on it. |
| [`ota_bootloader/`](ota_bootloader/) | Production | Pinned ESP-IDF A/B bootloader project; built by `scripts/build/ota_bootloader.sh` for firmware flashing. |
| [`scene_maker/`](scene_maker/) | Maintained standalone | Builds and inspects `.scenebundle` assets; CI tests it and scene-viewer helpers call it, but no current firmware consumer was found. |
| [`scene_viewer/`](scene_viewer/) | Maintained standalone | Offline `.scenebundle` renderer/emulator; CI-tested and documented, but no current firmware consumer was found. |
| [`touch_replay/`](touch_replay/) | Automated test support | Deterministic touch replay plus host-side firmware tests; test, strict Clippy, coverage, and capture-to-fixture workflows. |

Four test-only Cargo packages that used to live here (`app_state_store_host_harness/`,
`ble_transport_host_harness/`, `event_engine_host_harness/`,
`ui_shell_host_harness/`) moved to
[`../test-support/host/`](../test-support/README.md) in Phase 2 of the
[scripts and tools surface cleanup](../docs/plans/scripts-tools-surface-cleanup.md)
(change set C-203 through C-206) — they reuse firmware source on the host and
carry no production dependency or operator runbook, unlike everything else in
this table.

## Hostctl scenario and policy files

Hostctl loads the wired workflow files by basename from Rust, so absence of a
literal full-path reference is not evidence that a scenario is unused.

| File | Status | Evidence |
| --- | --- | --- |
| [`hostctl/scenarios/ble-phase1d.sw.yaml`](hostctl/scenarios/ble-phase1d.sw.yaml) | Production | Loaded by the `hostctl test ble-phase1d` implementation and covered by workflow-contract tests/current BLE plan. |
| [`hostctl/scenarios/firmware-update.sw.yaml`](hostctl/scenarios/firmware-update.sw.yaml) | Production | Loaded by `hostctl firmware-update` and its tests/current build-and-flash guide. |
| [`hostctl/scenarios/flash-capture.sw.yaml`](hostctl/scenarios/flash-capture.sw.yaml) | Production | Canonical flash/capture orchestration mandated by `AGENTS.md`; loaded and contract-tested by hostctl. |
| [`hostctl/scenarios/runtime-modes-smoke.sw.yaml`](hostctl/scenarios/runtime-modes-smoke.sw.yaml) | Production | Loaded by `hostctl test runtime-modes-smoke` and tested in hostctl. |
| [`hostctl/scenarios/sdcard-hw.sw.yaml`](hostctl/scenarios/sdcard-hw.sw.yaml) | Production | Loaded by both SD-card hostctl test paths. |
| [`hostctl/scenarios/troubleshoot.sw.yaml`](hostctl/scenarios/troubleshoot.sw.yaml) | Production | Loaded by the canonical troubleshooting command and documented in current guides. |
| [`hostctl/scenarios/ui-lifecycle.sw.yaml`](hostctl/scenarios/ui-lifecycle.sw.yaml) | Production | Loaded by `hostctl test ui-lifecycle`, contract-tested, and documented in the current troubleshooting guide. |
| [`hostctl/scenarios/wifi-acceptance.sw.yaml`](hostctl/scenarios/wifi-acceptance.sw.yaml) | Production | Loaded by the current Wi-Fi acceptance implementation and documented in network guides. |
| [`hostctl/scenarios/wifi-discovery-debug.sw.yaml`](hostctl/scenarios/wifi-discovery-debug.sw.yaml) | Production | Loaded by the current discovery diagnostic and documented by the Wi-Fi regression gate. |
| [`hostctl/scenarios/wifi-discovery-debug.default.toml`](hostctl/scenarios/wifi-discovery-debug.default.toml) | Production | Default discovery profile loaded by hostctl and referenced by the Wi-Fi guides/gate. |
| [`hostctl/scenarios/wifi-policy.default.json`](hostctl/scenarios/wifi-policy.default.json) | Production | Default bounded network policy referenced by current guides and hardware tests. |

## Nested executable helpers

| Entry point | Status | Evidence |
| --- | --- | --- |
| [`scene_maker/scripts/bake_ply_scene.py`](scene_maker/scripts/bake_ply_scene.py) | Maintained standalone | Documented by scene-maker and called by both scene-viewer render suites. |
| [`scene_maker/scripts/setup_buddha_scene_via_blender_mcp.py`](scene_maker/scripts/setup_buddha_scene_via_blender_mcp.py) | Maintained standalone | Documented by scene-maker and called by the Blender scene-viewer suite. |
| [`scene_maker/scripts/buddha_blender_stepper.py`](scene_maker/scripts/buddha_blender_stepper.py) | Legacy candidate | Only self-referential usage examples were found; no caller or README entry uses it. |
| [`scene_viewer/scripts/render_buddha_3d_scene.sh`](scene_viewer/scripts/render_buddha_3d_scene.sh) | Maintained standalone | Documented scene-viewer suite; calls scene-maker then scene-viewer. |
| [`scene_viewer/scripts/render_buddha_blender_scene.sh`](scene_viewer/scripts/render_buddha_blender_scene.sh) | Maintained standalone | Documented Blender reference/emulation suite. |
| [`touch_replay/import_touch_log.py`](touch_replay/import_touch_log.py) | Automated test support | Called by `scripts/touch/make_touch_fixture.sh`. |
| [`touch_replay/run_fixtures.sh`](touch_replay/run_fixtures.sh) | Automated test support | Called by `scripts/host-test.sh test touch-replay` and documented by touch-replay. |

## Audit result

Phase 1 of the [scripts and tools surface cleanup](../docs/plans/scripts-tools-surface-cleanup.md)
removed the two retirement-candidate top-level tools (`esp_idf_wifi_control/`
and `esp_idf_wifi_control_rust/`) and the orphaned `wifi-chaos.sw.yaml`
scenario file; see `docs/plans/scripts-tools-surface-cleanup-ledger.md` change
set C-107 through C-109 for the record. The scene-maker and scene-viewer pair
are still deliberately tested and documented, but they are standalone asset
experiments rather than part of the current firmware path.
`buddha_blender_stepper.py` has no live inbound path and is out of this
cleanup's scope (scene ownership is separate).

Phase 2 relocated the four test-only host-harness packages listed above to
[`../test-support/host/`](../test-support/README.md) (C-203 through C-206)
and replaced `scripts/ci/lint_host_tools.sh` with the
`scripts/host-test.sh`/`scripts/host-suites.tsv` dispatcher and registry.
