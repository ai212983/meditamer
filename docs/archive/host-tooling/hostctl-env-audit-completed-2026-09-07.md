# Hostctl Environment Variable Audit — Implementation Plan

Status: Done — environment cleanup and final validation complete
Last-reviewed: 2026-09-07

## Closeout (2026-09-07)

The final current-code inventory and independent review are complete. The live
[environment reference](../../reference/tools/hostctl-env-audit.md) replaces the
old audit's stale read sites, recommendations and incorrect legacy reject-list
claim; `.env.example` catalogs the same supported names.

- Removed four general-upload retry environment reads, retaining fixed defaults
  of 180/45/0.8/2. Acceptance keeps its intentionally tighter 30/8/0.8/2 budget.
- Removed undocumented `HOSTCTL_UPLOAD_SKIP_MKDIR` and its unused branches;
  uploads retain the default creation of required parent directories.
- Retained and documented connection timeout, structured logging, target
  selection and nine flash-wrapper options. Novelty/reject checks are distinct
  from configurable upload-client behavior.
- Moved the existing ShellCheck source annotation directly beside the dynamic
  source command and documented why the file is selected at runtime.

Final validation on the current working tree:

- Stable `cargo test --locked --manifest-path tools/hostctl/Cargo.toml`:
  332 passed, zero failures.
- Stable `cargo clippy --locked --manifest-path tools/hostctl/Cargo.toml
  --all-targets -- -D warnings`: passed.
- Rustfmt check on the changed upload module: passed.
- Bash syntax and `shellcheck -x` for the launcher and serial helper: passed.
- Scoped Markdown link and whitespace checks: passed.

No firmware or device experiment was run. Existing default upload behavior is
preserved; the removed environment overrides are no longer honored. This is
working-tree validation, not a committed-tree or publication claim.

The earlier verification and implementation inventory below are retained as
history and superseded by this closeout where they describe outstanding work.

## Focused verification (2026-09-07)

Most listed Category 3 removals are implemented. A scan of the 66 names in the
remaining-work section found no non-comment Rust string-literal references except
two general-upload retry settings and the intentionally retained upload mode
(including its test). Following the general-upload policy found four live overrides:

- `HOSTCTL_UPLOAD_SD_BUSY_TOTAL_RETRY_SEC`: 180 seconds.
- `HOSTCTL_UPLOAD_NET_RECOVERY_TIMEOUT_SEC`: 45 seconds.
- `HOSTCTL_UPLOAD_NET_RECOVERY_POLL_SEC`: 0.8 seconds.
- `HOSTCTL_UPLOAD_NET_RECOVERY_CONSECUTIVE_HEALTH`: 2, clamped to at least 1.

`storage/upload/run.rs` still reads those values. Acceptance's
`wifi/acceptance/runtime_upload/helpers.rs` uses fixed 30/8/0.8/2 values and
explicitly documents the tighter acceptance budget. The old same-variable,
two-default acceptance chain is gone; matching the general and acceptance
budgets is not required. The remaining decision is whether general-upload
overrides should stay exposed. They contradict `.env.example`'s claim that all
acceptance/upload internals are fixed and are absent from its optional catalog.
Resolve that contract before marking this audit complete; this verification
makes no behavior changes.

The shell scan found historical knob names in the experiment novelty guard;
these guard checks do not establish that the upload client still reads them.
The inspected launcher and serial helper no longer expose the listed cache-path
knob. `.env.example` has the intended six active configuration/CI entries.

Validation on the current working tree:

- `rustup run stable cargo test --locked --manifest-path tools/hostctl/Cargo.toml`:
  332 passed, zero failures.
- `bash -n scripts/hostctl.sh scripts/lib/serial_port.sh`: passed.
- `shellcheck -x scripts/hostctl.sh scripts/lib/serial_port.sh`: SC1090 at
  `scripts/lib/serial_port.sh:243` for dynamic `source "$env_path"`; not a clean
  ShellCheck pass. No shell changes were made or warning suppressed.

This is focused verification, not a full fresh classification of every current
Hostctl environment variable. The original inventory below is historical work
tracking; the findings above supersede its claims about still-live read sites.

## Context

The audit at `docs/reference/tools/hostctl-env-audit.md` classified ~100 `HOSTCTL_*` env vars into four categories. The goal is to reduce the env surface to only what is genuinely needed (machine-local config + justified ops/CI knobs), hard-coding the rest.

## What has been done

1. **Policy path hard-coded** — `HOSTCTL_NET_POLICY_PATH` removed from Rust (3 sites: `wifi/acceptance/mod.rs`, `wifi/discovery/mod.rs`, `ble_phase1s/setup.rs`) and from shell `required` lists. The Rust code now uses `env!("CARGO_MANIFEST_DIR")/scenarios/wifi-policy.default.json` directly.
2. **Dead vars removed from `hostctl.sh`** — `HOSTCTL_NET_POLICY_PATH` and `HOSTCTL_FIRMWARE_UPDATE_LOG_PATH` removed from the absolutization list.
3. **Dual-default bug fixed** — `helpers.rs` (acceptance upload retry policy) now reads only `HOSTCTL_NET_UPLOAD_*` names with canonical defaults (30/8/0.8/2), eliminating the two-layer override chain.
4. **Flash capture boot window** — `HOSTCTL_FLASH_CAPTURE_BOOT_WINDOW_MS` env fallback removed from `flash_capture/runtime.rs`; the script→flag path is the only path.
5. **`.env.example` cleaned** — active entries reduced to 4 machine-local vars + `STACK_RISK_LOCAL_ARRAY_MAX_BYTES` + `SONAR_TOKEN`. Commented catalog updated to list only justified optional overrides.
6. **`docs/guides/wifi-asset-upload.md` cleaned** — removed decided blackout A/B knobs, dead `POLICY_PATH`/`DISCOVERY_PROFILE_PATH` from sample commands, and trimmed regression-gate/guardrail lists.

## What remains (Category 3 — unnecessary exposure)

The audit lists ~55 vars that have hard-coded defaults in Rust but are still read from env. These should be converted to compile-time constants. The work is mechanical: for each var, replace `env_utils::parse_env_*("VAR", default)?` with the literal `default` (or a named `const` if shared).

### High-priority removals (clear wins, no behavior change)

| Var | Current read-site | Action |
|---|---|---|
| `HOSTCTL_NET_DISCOVERY_PROFILE_PATH` | `wifi/discovery/mod.rs` | Already has `env!("CARGO_MANIFEST_DIR")` fallback; remove env read entirely |
| `HOSTCTL_SERIAL_PORT_CACHE_PATH` | `hostctl.sh`, `serial_port.sh` | Keep script fallback `:-logs/.state/...`; remove from docs |
| `HOSTCTL_REPAINT_SETTLE_MS` / `HOSTCTL_TIMESET_SETTLE_MS` / `HOSTCTL_TIMESTATUS_SETTLE_MS` / `HOSTCTL_TIME_SYNC_SETTLE_MS` / `HOSTCTL_FLASH_CAPTURE_POST_SETTLE_MS` | `serial.rs`, `flash_capture/runtime.rs` | All default `200`; consolidate to one `const SETTLE_MS: u64 = 200` |
| `HOSTCTL_REPAINT_RETRIES` / `HOSTCTL_REPAINT_RETRY_DELAY_MS` / `HOSTCTL_REPAINT_WAIT_ACK` / `HOSTCTL_REPAINT_ACK_TIMEOUT_MS` | `serial.rs` | Serial-protocol constants; hard-code |
| `HOSTCTL_MODE_SMOKE_SETTLE_MS` / `HOSTCTL_MODE_SMOKE_POST_UPLOAD_STATUS_REPEATS` / `HOSTCTL_MODE_SMOKE_POST_UPLOAD_PING_REPEATS` | `runtime_modes/run.rs` | Smoke-test internals; hard-code |
| `HOSTCTL_TROUBLESHOOT_FLASH_FIRST` / `HOSTCTL_TROUBLESHOOT_FLASH_RETRIES` / `HOSTCTL_TROUBLESHOOT_PROBE_RETRIES` / `HOSTCTL_TROUBLESHOOT_PROBE_DELAY_MS` / `HOSTCTL_TROUBLESHOOT_PROBE_TIMEOUT_MS` / `HOSTCTL_TROUBLESHOOT_SOAK_CYCLES` | `troubleshoot/mod.rs` | Troubleshoot recipe constants; hard-code |
| `HOSTCTL_SDCARD_FLASH_FIRST` / `HOSTCTL_SDCARD_BASE_PATH` / `HOSTCTL_SDCARD_SDWAIT_TIMEOUT_MS` / `HOSTCTL_SDCARD_VERIFY_LBA` | `storage/sdcard` | Test-internal; hard-code |
| `HOSTCTL_NET_STARTUP_HEALTH_HYSTERESIS` / `HOSTCTL_NET_STARTUP_HEALTH_SUCCESS_STREAK` / `HOSTCTL_NET_STARTUP_HEALTH_REQ_TIMEOUT_SEC` / `HOSTCTL_NET_STARTUP_HEALTH_HYSTERESIS_TIMEOUT_SEC` / `HOSTCTL_NET_STARTUP_HEALTH_POLL_MS` / `HOSTCTL_NET_STARTUP_HEALTH_RECOVER_RETRIES` | `runtime_core/health.rs` | Six sub-knobs for one behavior; consolidate to one `HYSTERESIS` toggle, hard-code the rest |
| `HOSTCTL_NET_BOOT_DISCOVERY_MAX_UPTIME_MS` / `HOSTCTL_NET_BOOT_DISCOVERY_TIMEOUT_MS` / `HOSTCTL_NET_BOOT_DISCOVERY_SETTLE_MS` / `HOSTCTL_NET_BOOT_DISCOVERY_READY_ONLY_FALLBACK` | `runtime_core/start.rs` | Gate internals; hard-code |
| `HOSTCTL_NET_RUNTIME_READY_TIMEOUT_MS` / `HOSTCTL_NET_LISTENER_READY_GRACE_MS` / `HOSTCTL_NET_WAIT_READY_RECOVER_RETRIES` / `HOSTCTL_NET_FORCE_STOP_BEFORE_RECOVER` / `HOSTCTL_NET_ENSURE_OPERATING_MODE` | `runtime_core` | Timing/recovery internals; hard-code |
| `HOSTCTL_MAIN_STACK_HEADROOM_MIN_BYTES` / `HOSTCTL_TOUCH_CORE_STACK_HEADROOM_MIN_BYTES` / `HOSTCTL_TOUCH_ACTIVE_GAP_MAX_MS` / `HOSTCTL_NET_MIN_INTERNAL_FREE_BYTES` | `start.rs` gates | Firmware floor constants; hard-code |
| `HOSTCTL_NET_UPLOAD_TIMEOUT_SEC` / `HOSTCTL_NET_VERIFY_TIMEOUT_SEC` / `HOSTCTL_NET_RECOVER_READY_TIMEOUT_SEC` / `HOSTCTL_NET_RECOVER_READY_POLL_SEC` / `HOSTCTL_NET_OPERATION_RETRIES` / `HOSTCTL_NET_REQ_READ_BODY_RESET_MAX_DELTA` / `HOSTCTL_NET_UPLOAD_REFRESH_ON_FAILURE` | `acceptance` runtime | Acceptance internals; hard-code |
| `HOSTCTL_UPLOAD_DISABLE_POOL` / `HOSTCTL_UPLOAD_FORCE_CONN_CLOSE` / `HOSTCTL_UPLOAD_FRESH_CLIENT_PER_UPLOAD` / `HOSTCTL_UPLOAD_TCP_NODELAY` / `HOSTCTL_UPLOAD_DIRECT_BURST_SENDER` / `HOSTCTL_UPLOAD_DIRECT_BURST_BYTES` / `HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS` / `HOSTCTL_UPLOAD_TRANSPORT_RESET_FAST_RETRY` / `HOSTCTL_UPLOAD_TRANSPORT_RESET_FAST_RETRY_STREAK` / `HOSTCTL_UPLOAD_TRANSPORT_RESET_CHUNK_FALLBACK` / `HOSTCTL_UPLOAD_TRANSPORT_RESET_CHUNK_FALLBACK_STREAK` / `HOSTCTL_NET_REUSE_UPLOAD_CLIENT` | `upload` client config/transfer | Blackout-era A/B knobs; consolidate behind chosen configuration; keep at most `HOSTCTL_UPLOAD_MODE` + `CHUNK_SIZE` exposed |

### Category 4 fixes (wired wrongly)

| Var | Fix |
|---|---|
| `HOSTCTL_NET_ENFORCE_POLICY_FLOORS` | Remove env read; always enforce. If escape hatch needed, make it explicit CLI `--allow-below-policy-floors` |
| `HOSTCTL_UPLOAD_NET_RECOVERY_TIMEOUT_SEC` / `HOSTCTL_UPLOAD_SD_BUSY_TOTAL_RETRY_SEC` | Already fixed in `helpers.rs`; verify `run.rs` defaults align (30/8) or leave as-is if the general path genuinely needs different defaults |
| `HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS` | Make default unconditional (`0`); remove silent dependence on `HOSTCTL_UPLOAD_DIRECT_BURST_SENDER` |

### Legacy reject lists

The audit claimed `HOSTCTL_WIFI_UPLOAD_*` legacy reject lists exist in hw scripts. **This is incorrect** — the actual legacy reject list is in `hostctl.sh` for `UPLOAD_*` (without `HOSTCTL_` prefix). The hw scripts only reject `HOSTCTL_PORT`/`HOSTCTL_BAUD` in net context, which is correct and should stay.

## Verification checklist

After each batch of removals:

1. `rustup run stable cargo test --locked --manifest-path tools/hostctl/Cargo.toml` — must pass; report any pre-existing failures separately
2. `bash -n` + `shellcheck -x` on every touched script
3. `scripts/ci/check_markdown_links.sh` on touched docs
4. `git --no-optional-locks diff --stat -- tools scripts docs .env.example` — confirm only expected files changed

## Final state target

`.env.example` active entries:
- `HOSTCTL_NET_PORT`, `HOSTCTL_NET_BAUD`, `HOSTCTL_NET_SSID`, `HOSTCTL_NET_PASSWORD` (machine-local)
- `STACK_RISK_LOCAL_ARRAY_MAX_BYTES` (CI)
- `SONAR_TOKEN` (CI)

Commented optional overrides (justified):
- `HOSTCTL_PORT`, `HOSTCTL_BAUD`, `HOSTCTL_PORT_HINT`
- `HOSTCTL_NET_LOG_PATH`, `HOSTCTL_NET_CYCLES`, `HOSTCTL_NET_SOAK_CYCLES`
- `HOSTCTL_NET_ALLOW_LOG_APPEND`, `HOSTCTL_NET_LOCK_WAIT_SEC`
- `HOSTCTL_NET_SKIP_HOST_WIFI_CHECK`
- `HOSTCTL_UPLOAD_TOKEN`, `HOSTCTL_UPLOAD_MODE`, `HOSTCTL_UPLOAD_CHUNK_SIZE`
- `HOSTCTL_UPLOAD_SEND_DIAG`, `HOSTCTL_UPLOAD_SEND_DIAG_DEEP`, `HOSTCTL_UPLOAD_SEND_DIAG_PATH`
- `HOSTCTL_NET_PANIC_AUTO_TROUBLESHOOT`, `HOSTCTL_NET_REGRESSION_OUTPUT_DIR`
- `HOSTCTL_EXPERIMENT_NOVELTY_GUARD`, `HOSTCTL_EXPERIMENT_NOVELTY_OVERRIDE`
- `HOSTCTL_HOST_RUSTUP_TOOLCHAIN`

Everything else: hard-coded in Rust, not exposed.
