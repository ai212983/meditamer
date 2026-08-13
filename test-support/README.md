# Test-support inventory

`test-support/` holds first-party Cargo packages that exist only to run
firmware source on the host — they are never a production build dependency
and carry no operator runbook. They are relocated out of `tools/` (which
mixes them with production tooling) by the
[scripts and tools surface cleanup](../docs/plans/scripts-tools-surface-cleanup.md);
see `docs/plans/scripts-tools-surface-cleanup-ledger.md` change set C-203
through C-207 for the record.

Each package keeps its own independent `Cargo.toml`/`Cargo.lock` (native host
target, not cross-compiled for the firmware) and is not part of a Cargo
workspace. Manifest-relative `#[path]` attributes pull the real firmware
modules in from `src/firmware/...` on the host; moving a package changes the
`../` depth of every such attribute, which is why relocations here are never
a plain directory move.

`scripts/host-suites.tsv` is the authoritative test/lint/coverage membership
registry for these packages (and for the production tools that share the
host-test/lint/coverage lanes — `tools/event_config_compiler`,
`tools/touch_replay`, `tools/hostctl`, `packages/sdcard`). Run
`scripts/host-test.sh --list` to see current membership, or
`scripts/host-test.sh <test|lint> <suite>` to run one directly.

## Packages

| Package | Owner | Caller/runbook | Reason it cannot be covered by an existing entry point |
| --- | --- | --- | --- |
| [`host/app_state_store_host_harness/`](host/app_state_store_host_harness/) | Firmware + Host Tooling | `scripts/host-test.sh test\|lint app-state`; aggregate host-tests/host-lint/coverage lanes | Reuses `src/firmware/app_state/*` and `src/firmware/ui/shell/*` on the host via `#[path]` shims; no production or CLI entry point exercises this path. |
| [`host/ble_transport_host_harness/`](host/ble_transport_host_harness/) | Firmware + Host Tooling | `scripts/host-test.sh test ble-transport`; aggregate host-tests lane | Host proof for the bounded first-party ESP Radio BLE transport patch (`vendor/esp-radio-*-bounded`); not lint/coverage-tracked (BLE is excluded from those lanes by design). |
| [`host/event_engine_host_harness/`](host/event_engine_host_harness/) | Firmware + Host Tooling | `scripts/host-test.sh test\|lint event-engine`; aggregate host-tests/host-lint/coverage lanes | Hosts `src/firmware/event_engine` and `src/firmware/imu/scheduler.rs` for host-side regression; depends on `tools/event_config_compiler` as a build-dependency. |
| [`host/net_status_host_harness/`](host/net_status_host_harness/) | Firmware + Host Tooling | `scripts/host-test.sh test\|lint net-status`; aggregate host-tests/host-lint lanes | Hosts the real observability atomics, recorders, and snapshot to prove HTTP listener lifecycle cannot clear the Wi-Fi task's DHCP lease. |
| [`host/ui_shell_host_harness/`](host/ui_shell_host_harness/) | Firmware + Host Tooling | `scripts/host-test.sh test\|lint ui-shell`; aggregate host-tests/host-lint/coverage lanes | Hosts `src/firmware/ui/shell` and LVGL overlay semantics on the host; requires `DEP_LV_CONFIG_PATH` to resolve `lightvgl-sys`'s `lv_conf.h`. |

## Verified equivalence

Every package was tested, strict-linted, and coverage-run in place at its new
path before the old `tools/` path was removed (see ledger E-0003): identical
test counts and pass/fail results, identical Clippy `-D warnings` result, and
non-zero coverage produced for each `coverage=yes` suite.
