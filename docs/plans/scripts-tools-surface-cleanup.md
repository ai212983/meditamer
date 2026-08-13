# Scripts and Tools Surface Cleanup Plan

- Status: Done
- Last-reviewed: 2026-08-13
- Owner: Firmware + Host Tooling
- Scope: `scripts/`, top-level `tools/`, host test support, and their direct callers, hooks, CI, and current docs
- Evidence: [script inventory](../../scripts/README.md), [tool inventory](../../tools/README.md),
  [implementation ledger](scripts-tools-surface-cleanup-ledger.md)

## Objective

Reduce 67 working-tree scripts to 44 and delete two of 12 maintained tool/support surfaces. Classify
the survivors honestly as six operational tools plus four test-support packages. Ratchet public executable entry points to 15; do not weaken device proof or move orchestration into hostctl Rust.

## Baseline

Audit revision: `803077a0816b191342f80c7ee1d2edaba6eafb1c`.

| Measure | HEAD | Working tree |
| --- | ---: | ---: |
| First-party `.sh`/`.py` below `scripts/` | 52 | 67 |
| Script LOC | — | 5,747 |
| Operational/support directories below `tools/` | 12 | 12 |
| Packages below `test-support/` | 0 | 0 |
| Total maintained tool/support surfaces | 12 | 12 |

Counts include tracked and untracked source files; exclude READMEs, caches, bytecode, and artifacts; and ignore executable bits. Replace this table with execution-base values in Phase 0.

## Invariants

1. Keep `scripts/device/flash.sh` canonical; direct `hostctl flash-capture` remains an advanced explicit-mode path.
2. Preserve native-host isolation, lockfile enforcement, focused reproduction, and lane exit semantics.
3. Keep branching, fallback, retries, and gate flow in workflow YAML; Rust owns primitive actions and context I/O.
4. Preserve the full Wi-Fi novelty preflight and regression gate before landing Wi-Fi, network, or upload changes.
5. Retain the current Wi-Fi acceptance, discovery, regression, and novelty-guard scripts. Adding workflow-engine composition solely to remove them is outside this cleanup.
6. Do not combine independent CI policies only to reduce file count.
7. Do not edit frozen `docs/archive/` evidence.
8. Limit cleanup commits to this surface and the direct caller, hook, CI, and documentation updates required by it.

## Phase 0: Establish an executable baseline

Record the execution base SHA and a caller/arguments/environment/credentials/paths/ports/reports/artifacts/exit-code matrix.

Record separate baselines for public executable paths and leaf commands. The 15-entry target applies only to executable paths; preserve every leaf contract in this cleanup.

Classify paths as:

- typed local operands, relative to the captured invocation directory;
- repo-owned defaults/policies/artifacts, relative to the repository root;
- remote paths and command strings, never normalized.

Acceptance:

- exact baseline and command contracts are recorded;
- every proposed deletion has no invocable live caller or maintained runbook;
- historical evidence may name removed tools but must not present them as runnable.

## Phase 1: Remove closed Wi-Fi investigation artifacts

Delete six scripts: the NVS/PHY dumper, both ESP-IDF Wi-Fi comparator wrappers, both MAC-window extractors, and `esp_idf_env.sh`. Delete both `tools/esp_idf_wifi_control*` directories.

Delete `tools/hostctl/scenarios/wifi-chaos.sw.yaml` after recording that generic tests parse it but no CLI/runtime loads it. Do not touch `buddha_blender_stepper.py`; scene ownership is separate.

Acceptance:

- live callers and runbooks are absent outside this plan and the inventories;
- DRAM/reference text retains valid evidence provenance without runnable claims;
- archives remain untouched and both inventories are updated.

Expected script count: 61. Expected top-level tools: 10.

## Phase 2: Consolidate host suites without changing membership

Create `scripts/host-test.sh` with `scripts/host-suites.tsv` as the authoritative suite registry; keep `check_software_baseline.sh` as aggregate owner. Remove the eight per-harness wrappers and `lint_host_tools.sh`.

Relocate four test-only Cargo packages out of `tools/` without merging them into the embedded root
crate: app-state, event-engine, UI-shell, and the first-party ESP Radio BLE contract below `test-support/host/`.
Preserve independent native manifests, locks, feature/cfg behavior, source wiring, and the app-state shim tree.
Keep CLI-bearing `touch_replay` and production `event_config_compiler` in `tools/`.
Add `test-support/README.md` as its owner/purpose/caller ledger and link it from the updated tools ledger.

Preserve three distinct current sets:

- tests: three policy self-tests, eight named harness/fixture suites,
  `scene_maker`, `scene_viewer`, and host-feature `sdcard`;
- strict lint: app-state, event-config, event-engine, UI shell, touch, hostctl,
  and host-feature `sdcard` (not BLE or scene tools);
- coverage: app-state, event-config, event-engine, UI shell, touch, and hostctl.

Acceptance:

- `--list` proves before/after membership parity and records exclusions;
- focused `test|lint <suite> [-- <args>]` commands preserve supplied target/arg
  forwarding, hostctl env sanitization/toolchain/target dir, BLE ephemeral target
  dir, UI LVGL config, SD features, touch fixtures, `--locked`, `--target`,
  `--all-targets`, and `-D warnings` for their current suites;
- existing aggregate lane names and coverage artifacts remain compatible;
- the dispatcher, aggregate baseline, and coverage runner consume the same authoritative registry;
- every first-party scanner, baseline/path rename, hook glob, and CI filter recognizes `test-support/`:
  rustfmt, include-usage, orphan reachability, Rust LOC, code analysis, test/lint, and coverage;
- test-size exemptions remain intentional, but include/source-reachability checks cannot be bypassed;
- target/out/cache ignores cover the new root; relocation tests prove identical native target, lock,
  feature, lane membership, exits, and clean porcelain from root and nested paths;
- manifest-relative `#[path]`, app-state shims, event-engine build paths/dependency, all callers, and old-path absence are verified transactionally;
- hook globs, CI paths, PR template, and current runbooks watch/name the
  dispatcher and registry;
- current job/hook parallelism is preserved; no new internal parallelism is
  required.

Expected scripts: 53. Expected taxonomy: 6 tools + 4 test-support packages = 10 maintained surfaces.

## Phase 3: Add one hostctl launcher; retire non-Wi-Fi wrappers

Promote `run_hostctl.sh` to `scripts/hostctl.sh`, limited to native Cargo launch preparation. It must
not source broad `.env.local` credentials or require a port unnecessarily; Rust owns typed paths.

Unify repo-root/default log-lock placement. Port order is CLI, command-specific environment, valid
cache, then unambiguous autodetect. A missing explicit port fails; nothing defaults under `/tmp`.

Migrate and remove seven wrappers: upload, firmware update, repaint,
runtime-mode smoke, SD-card, SD burst, and troubleshoot. Keep Wi-Fi acceptance,
discovery, regression, and novelty-guard scripts as the public guarded paths.

Before removing troubleshoot, migrate the retained regression gate's panic path
to `scripts/hostctl.sh test troubleshoot` with the same resolved port, debug
profile, stage output path, and nonfatal troubleshoot-result handling.

Direct public Wi-Fi acceptance/regression documentation uses the guarded wrappers;
no public path may bypass novelty preflight.

Record and retain every `flash.sh` profile, port/hint, preserved-image, IDF,
capture/flash mode, baud, post-command, and artifact-output mapping before any
simplification. Ordinary docs continue to use `flash.sh`.

Acceptance:

- the Phase 0 matrix proves equivalent launcher commands and secret
  non-propagation from arbitrary invocation directories;
- root/path/port/cache behavior has focused tests;
- identified-device Wi-Fi regression proves the launcher/troubleshoot migration;
- all live callers and runbooks use surviving commands.

Expected script count: 46.

## Phase 4: Small duplication and closeout ratchet

- Remove the orphan shell shim only after Python provides default Git-root
  discovery, retains explicit `--repo-root` for fixtures, and is tested from a
  nested directory and outside any Git checkout.
- Merge tap/general touch capture into `touch_capture.sh --mode touch|tap`.
- Replace `setup_hooks.sh` with documented `lefthook install`.

Add `scripts/surface.json` metadata for every first-party script: internal
or public role, owner, caller/runbook, and reason an existing entry point cannot
cover it. Add `scripts/ci/check_script_surface.py` for exact tracked-path
parity, duplicates/stale entries, public executable paths, and documented leaf
commands. Lower exact baselines with deletions so capacity cannot be silently
reused. Run it on script and inventory-only changes, including Markdown-only
PRs.

Closeout acceptance:

- exact script count is 44 and public executable paths are at most 15;
- documented leaf command contracts have an explicit baseline and change log;
- in-scope script and top-level-tool legacy-candidate and unindexed entries are zero;
- source, host tests/lint, coverage, static-source, quality, firmware, release
  artifact, hostctl workflow, and focused inventory checks pass;
- canonical flash/capture smoke and Phase 3 device evidence are retained;
- plan status changes from Proposed to Active only when execution is approved;
- plan status remains Active until every closeout item passes, then becomes Done.

Keep `scene_maker`, `scene_viewer`, and their nested helpers unchanged. This plan closes with 6 tools plus 4 indexed test-support packages.
