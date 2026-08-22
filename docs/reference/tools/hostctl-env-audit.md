# Hostctl Environment Variable Audit

Exhaustive ledger of every distinct `HOSTCTL_*` token in `tools/hostctl/src/**/*.rs`
and `scripts/**`, with a per-var classification and recommendation. Audit-only: no
removals are implemented here.

## Evidence and method

- Token inventory: `grep -roE 'HOSTCTL_[A-Z0-9_]+' tools/hostctl/src scripts | sort -u`.
- Read-site context: `grep -rnB2 -A8 'HOSTCTL_'` over the same trees; every default
  below is quoted from the actual read-site (`parse_env_*` default argument,
  `unwrap_or*` fallback, or shell `${VAR:-default}`).
- Parent-agent claim check: "zero hard-required vars" is **not accurate**. Rust has
  three `.context("... must be set")` hard requirements (`HOSTCTL_NET_PORT`,
  `HOSTCTL_NET_SSID` in `wifi/acceptance/mod.rs`, `wifi/discovery/mod.rs`,
  `ble_phase1s/setup.rs`), and the hw test scripts additionally hard-require
  `HOSTCTL_NET_BAUD` and `HOSTCTL_NET_PASSWORD` via their `required=(...)` loops.
- Non-`HOSTCTL_` knobs read by hostctl (`ESPFLASH_SKIP_UPDATE_CHECK`,
  `ESPFLASH_ENABLE_FALLBACK`, `FLASH_SET_TIME_AFTER_FLASH`, `STACK_RISK_*`,
  `SONAR_TOKEN`) are out of scope for this ledger.
- `scripts/ci/check_secrets.sh` matches the `HOSTCTL_NET_` prefix in a regex; that is
  not a variable read and is excluded.

## Classification legend

1. **Machine-local/config** — genuinely user/machine-specific. Stays env-exposed.
2. **Keep exposed** — hard-coded default, exposure justified (ops/CI/diagnostic knob).
3. **Unnecessary exposure** — has a default; exposure adds no real value. Recommend
   dropping from template/docs (and, in a follow-up, from code where trivially safe).
4. **Wired wrongly** — needed, but the read-shape is wrong; fix named per entry.

---

## Category 1 — Machine-local/config (stays env)

| Var | Read-site | Notes |
|---|---|---|
| `HOSTCTL_PORT` | `env_utils::require_port` | Serial port; autodetect fallback; hard error if inconclusive |
| `HOSTCTL_BAUD` | `env_utils::baud_from_env(default)` | Baud override for generic serial commands |
| `HOSTCTL_PORT_HINT` | `port_detect.rs` `.ok()` | Substring hint to disambiguate autodetect |
| `HOSTCTL_NET_PORT` | `wifi/acceptance`, `wifi/discovery` `.context(...)` | **Hard-required** by net workflows |
| `HOSTCTL_NET_BAUD` | same, `.unwrap_or(115200)` | Default exists but hw scripts hard-require it |
| `HOSTCTL_NET_SSID` | same, `.context(...)` | **Hard-required**; secret-adjacent |
| `HOSTCTL_NET_PASSWORD` | same, `.unwrap_or_default()` | Machine-local secret |
| `HOSTCTL_UPLOAD_TOKEN` | `wifi/acceptance/mod.rs` `.ok()` | Device auth token when firmware enables it |
| `HOSTCTL_HOST_RUSTUP_TOOLCHAIN` | `hostctl.sh`, `host-test.sh` `:-stable` | Host toolchain pin |

## Category 2 — Keep exposed (justified)

| Var | Read-site / default | Justification |
|---|---|---|
| `HOSTCTL_LOG_JSON_PATH` | `logging.rs` `.ok()` | Structured log opt-in for CI artifact capture |
| `HOSTCTL_NET_LOG_PATH` | wifi workflows, timestamped default | Stage log routing; regression gate sets it per stage |
| `HOSTCTL_NET_CYCLES` | `parse_env_u32(..., 3)` | Acceptance run length; gate script overrides per stage |
| `HOSTCTL_NET_SOAK_CYCLES` | gate script `:-0` | Soak-stage length knob |
| `HOSTCTL_NET_PANIC_AUTO_TROUBLESHOOT` | gate script `:-1` | Ops toggle for auto-troubleshoot on panic |
| `HOSTCTL_NET_REGRESSION_OUTPUT_DIR` | gate script default `./logs/wifi_regression_gate_<ts>` | Report destination for CI |
| `HOSTCTL_NET_ALLOW_LOG_APPEND` | guardrails `false` | Needed to re-run into an existing log path deliberately |
| `HOSTCTL_NET_LOCK_WAIT_SEC` | guardrails `parse_env_f64(..., 0.0)` | Fail-fast→wait switch when sharing a bench port |
| `HOSTCTL_NET_SKIP_HOST_WIFI_CHECK` | `false` | Bench hosts not on the target SSID (CI/containers) |
| `HOSTCTL_NET_ENFORCE_POLICY_FLOORS` | guardrails `parse_env_bool01(..., true)` | See Category 4 note — read-shape is actually coherent; kept as kill-switch |
| `HOSTCTL_UPLOAD_MODE` | `transfer.rs` auto/direct/chunked | Documented A/B transport selector |
| `HOSTCTL_UPLOAD_CHUNK_SIZE` | `parse_env_u64(..., 65536)` | Documented; throughput tuning |
| `HOSTCTL_UPLOAD_CONNECT_TIMEOUT_SEC` | `parse_env_f64(..., 4.0)` | Weak-link networks |
| `HOSTCTL_UPLOAD_SEND_DIAG` | `false` | Documented host-side upload diagnostics |
| `HOSTCTL_UPLOAD_SEND_DIAG_DEEP` | `false` | Documented deep instrumentation |
| `HOSTCTL_UPLOAD_SEND_DIAG_PATH` | `.ok()` | Diag sidecar path |
| `HOSTCTL_EXPERIMENT_NOVELTY_GUARD` | guard script `:-1` | Decision-ledger gate |
| `HOSTCTL_EXPERIMENT_NOVELTY_OVERRIDE` | guard script `:-0` | Explicit reconfirmation escape hatch |
| `HOSTCTL_FLASH_CAPTURE_LOG_PATH` | `flash.sh` → `--log` | flash-capture artifact path |
| `HOSTCTL_FLASH_CAPTURE_MODE` | `flash.sh` → `--capture-mode` | Flash policy wrapper interface |
| `HOSTCTL_FLASH_CAPTURE_FLASH_MODE` | `flash.sh` → `--flash-mode` | ditto |
| `HOSTCTL_FLASH_CAPTURE_IMAGE` | `flash.sh` → `--image` | ditto |
| `HOSTCTL_FLASH_CAPTURE_POST_COMMAND` | `flash.sh` → `--post-command` | ditto |
| `HOSTCTL_FLASH_CAPTURE_POST_PATTERN` | `flash.sh` → `--post-pattern` | ditto |
| `HOSTCTL_FLASH_CAPTURE_POST_TIMEOUT_MS` | `flash.sh` → `--post-timeout-ms` | ditto |
| `HOSTCTL_FLASH_CAPTURE_BOOT_WINDOW_MS` | `flash.sh` → flag; Rust fallback `8000` | See Category 4: dual read-path |

## Category 3 — Unnecessary exposure (drop from template/docs)

| Var | Read-site / default | Recommendation |
|---|---|---|
| `HOSTCTL_NET_LOCK_PATH` | guardrails, default `default_lock_path(port)` | Derivable from port; drop from docs; keep code default only |
| `HOSTCTL_NET_PANIC_CONTEXT_LINES` | gate script `:-80` | Bake into gate script; drop exposure |
| `HOSTCTL_NET_PANIC_EXCERPT_PATH` | gate script, `${run_dir}/panic_excerpt.log` | Derive from output dir; drop exposure |
| `HOSTCTL_NET_REGRESSION_REPORT_PATH` | gate script, `${run_dir}/report.json` | Derive from output dir; drop exposure |
| `HOSTCTL_SERIAL_PORT_CACHE_PATH` | `hostctl.sh`, `serial_port.sh` `:-logs/.state/...` | Internal plumbing; drop exposure |
| `HOSTCTL_REPAINT_CMD` | `serial.rs` default `"REPAINT"` | CLI arg `--command` already exists; drop env path |
| `HOSTCTL_REPAINT_LOG_PATH` | `serial.rs` `var_os` | Use `--output` option pattern; drop |
| `HOSTCTL_REPAINT_SETTLE_MS` / `HOSTCTL_TIMESET_SETTLE_MS` / `HOSTCTL_TIMESTATUS_SETTLE_MS` / `HOSTCTL_TIME_SYNC_SETTLE_MS` / `HOSTCTL_FLASH_CAPTURE_POST_SETTLE_MS` | all default `200` | Consolidate to one internal constant; never tuned in practice |
| `HOSTCTL_REPAINT_RETRIES` (`2`) / `HOSTCTL_REPAINT_RETRY_DELAY_MS` (`500`) / `HOSTCTL_REPAINT_WAIT_ACK` (`true`) / `HOSTCTL_REPAINT_ACK_TIMEOUT_MS` (`15000`) | serial.rs | Serial-protocol constants; hard-code |
| `HOSTCTL_MODE_SMOKE_SETTLE_MS` (`0`) / `HOSTCTL_MODE_SMOKE_POST_UPLOAD_STATUS_REPEATS` (`3`) / `HOSTCTL_MODE_SMOKE_POST_UPLOAD_PING_REPEATS` (`2`) | runtime_modes/run.rs | Smoke-test internals; hard-code |
| `HOSTCTL_TROUBLESHOOT_FLASH_FIRST` (`true`) / `_FLASH_RETRIES` (`2`) / `_PROBE_RETRIES` (`6`) / `_PROBE_DELAY_MS` (`700`) / `_PROBE_TIMEOUT_MS` (`4000`) / `_SOAK_CYCLES` (`4`) | troubleshoot/mod.rs | Troubleshoot recipe constants; hard-code (promote to CLI flags only if needed) |
| `HOSTCTL_SDCARD_FLASH_FIRST` (`false`) / `HOSTCTL_SDCARD_BASE_PATH` (`/sd<hhmmss>`) / `HOSTCTL_SDCARD_SDWAIT_TIMEOUT_MS` (`300000`) / `HOSTCTL_SDCARD_VERIFY_LBA` (`2048`) | storage/sdcard | Test-internal; hard-code or CLI flag |
| `HOSTCTL_NET_STARTUP_HEALTH_HYSTERESIS` (`true`) / `_SUCCESS_STREAK` (`3`) / `_REQ_TIMEOUT_SEC` (`1.5`) / `_HYSTERESIS_TIMEOUT_SEC` (`20`) / `_POLL_MS` (`300`) / `_RECOVER_RETRIES` (`1`) | runtime_core/health.rs | Six sub-knobs for one behavior; consolidate behind a single `..._HYSTERESIS` on/off, hard-code the rest |
| `HOSTCTL_NET_BOOT_DISCOVERY_MAX_UPTIME_MS` (`30000`) / `_TIMEOUT_MS` (`180000`) / `_SETTLE_MS` (`6000`) / `_READY_ONLY_FALLBACK` (`false`) | runtime_core/start.rs | Gate internals; hard-code |
| `HOSTCTL_NET_RUNTIME_READY_TIMEOUT_MS` (`45000`) / `HOSTCTL_NET_LISTENER_READY_GRACE_MS` (`2000`) / `HOSTCTL_NET_WAIT_READY_RECOVER_RETRIES` (`1`) / `HOSTCTL_NET_FORCE_STOP_BEFORE_RECOVER` (`true`) / `HOSTCTL_NET_ENSURE_OPERATING_MODE` (`true`) | runtime_core | Timing/recovery internals; hard-code |
| `HOSTCTL_MAIN_STACK_HEADROOM_MIN_BYTES` (`8192`) / `HOSTCTL_TOUCH_CORE_STACK_HEADROOM_MIN_BYTES` (`1024`) / `HOSTCTL_TOUCH_ACTIVE_GAP_MAX_MS` (`16`) / `HOSTCTL_NET_MIN_INTERNAL_FREE_BYTES` (`16384`) | start.rs gates | Firmware floor constants; belong in code or policy JSON, not env |
| `HOSTCTL_NET_UPLOAD_TIMEOUT_SEC` (`180`) / `HOSTCTL_NET_VERIFY_TIMEOUT_SEC` (`30`) / `HOSTCTL_NET_RECOVER_READY_TIMEOUT_SEC` (`12`) / `HOSTCTL_NET_RECOVER_READY_POLL_SEC` (`0.4`) / `HOSTCTL_NET_OPERATION_RETRIES` (`3`) / `HOSTCTL_NET_REQ_READ_BODY_RESET_MAX_DELTA` (`0`) / `HOSTCTL_NET_UPLOAD_REFRESH_ON_FAILURE` (`true`) | acceptance runtime | Acceptance internals; hard-code |
| `HOSTCTL_UPLOAD_DISABLE_POOL` / `HOSTCTL_UPLOAD_FORCE_CONN_CLOSE` / `HOSTCTL_UPLOAD_FRESH_CLIENT_PER_UPLOAD` / `HOSTCTL_UPLOAD_TCP_NODELAY` / `HOSTCTL_UPLOAD_DIRECT_BURST_SENDER` / `HOSTCTL_UPLOAD_DIRECT_BURST_BYTES` / `HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS` / `HOSTCTL_UPLOAD_TRANSPORT_RESET_FAST_RETRY` (+`_STREAK`) / `HOSTCTL_UPLOAD_TRANSPORT_RESET_CHUNK_FALLBACK` (+`_STREAK`) / `HOSTCTL_NET_REUSE_UPLOAD_CLIENT` | upload client config/transfer | Blackout-era A/B knobs; the novelty guard and the archived decision ledger show these experiments are decided. Consolidate behind the chosen configuration; keep at most `HOSTCTL_UPLOAD_MODE` + `CHUNK_SIZE` exposed |
| `HOSTCTL_NET_DISCOVERY_PROFILE_PATH` | discovery/mod.rs, built-in default profile | Profile is versioned in-repo; drop exposure unless a real second profile appears |
| `HOSTCTL_NET_POLICY_PATH` | **no reader** — only `hostctl.sh` absolutization list; Rust hard-codes `scenarios/wifi-policy.default.json` | Dead; remove from `hostctl.sh` list and docs |
| `HOSTCTL_FIRMWARE_UPDATE_LOG_PATH` | **no reader** — only `hostctl.sh` absolutization list | Dead; remove |

## Category 4 — Genuinely needed but wired wrongly

| Var | Evidence | Fix |
|---|---|---|
| `HOSTCTL_NET_ENFORCE_POLICY_FLOORS` | guardrails.rs:39 reads `parse_env_bool01(..., true)`. The task brief claimed default=false/override=true inversion; **that claim is wrong** — default is enforce-on and the name matches the semantic. The real defect is that the safety floor can be silently disabled by env at all. | Remove the env read; always enforce. If an escape hatch is truly needed, make it an explicit CLI `--allow-below-policy-floors` flag so the choice is visible in the invocation. |
| `HOSTCTL_UPLOAD_NET_RECOVERY_TIMEOUT_SEC` | Default `45.0` in `upload/run.rs:238` but `8.0` in `acceptance/runtime_upload/helpers.rs:12` — same var, two defaults by caller. | Pick one default; if acceptance genuinely needs a tighter budget, use the `HOSTCTL_NET_UPLOAD_*` wrapper consistently instead of re-reading the same var with a different default. |
| `HOSTCTL_UPLOAD_SD_BUSY_TOTAL_RETRY_SEC` | Default `180.0` in run.rs vs `30.0` in helpers.rs — same defect. | Same fix. |
| `HOSTCTL_NET_UPLOAD_NET_RECOVERY_TIMEOUT_SEC` / `_POLL_SEC` / `_CONSECUTIVE_HEALTH` / `HOSTCTL_NET_UPLOAD_SD_BUSY_TOTAL_RETRY_SEC` | helpers.rs reads the `HOSTCTL_UPLOAD_*` var, then re-reads the `HOSTCTL_NET_UPLOAD_*` var on top — a two-layer override chain for four values. | Collapse: acceptance should read only its own `HOSTCTL_NET_UPLOAD_*` names with code defaults, or drop the wrappers entirely. |
| `HOSTCTL_UPLOAD_PRE_PUT_DELAY_MS` | config.rs:47 — default silently depends on `HOSTCTL_UPLOAD_DIRECT_BURST_SENDER` state. | Make the default unconditional (`0`); let burst mode set its own constant internally. |
| `HOSTCTL_FLASH_CAPTURE_BOOT_WINDOW_MS` | Read both in `flash.sh` (forwarded to `--boot-window-ms`) and as Rust env fallback (`runtime.rs:59`, default 8000) — two paths to the same value. | Keep the script→flag path only; delete the Rust env fallback (flag `unwrap_or(8000)`). |

## Script-only / legacy reject-only vars

- **Script-plumb only (absolutization lists, cache, toolchain):**
  `HOSTCTL_SERIAL_PORT_CACHE_PATH`, `HOSTCTL_HOST_RUSTUP_TOOLCHAIN`,
  `HOSTCTL_FIRMWARE_UPDATE_LOG_PATH`, `HOSTCTL_NET_POLICY_PATH` — the last two have
  no consumer at all and are removal candidates outright.
- **Legacy `HOSTCTL_WIFI_UPLOAD_*` reject-list (19 vars):** `CYCLES`,
  `PAYLOAD_BYTES`, `CONNECT_TIMEOUT_SEC`, `LISTEN_TIMEOUT_SEC`, `HTTP_TIMEOUT_SEC`,
  `HEALTH_TIMEOUT_SEC`, `STAT_TIMEOUT_MS`, `IP_DISCOVERY_TIMEOUT_SEC`,
  `OPERATION_RETRIES`, `REMOTE_ROOT`, `PAYLOAD_PATH`, `SSID`, `PASSWORD`,
  `LOCK_PATH`, `TEST_NAME`, `DHCP_TIMEOUT_MS`, `PINNED_DHCP_TIMEOUT_MS` — appear only
  in `reject_legacy_env_vars` calls in the hw test scripts. **Candidates for removal
  together with the reject calls** once the deprecation window is judged closed; also
  note `HOSTCTL_PORT`/`HOSTCTL_BAUD` appear in those same reject lists (they are
  rejected only in net-workflow context, where the `HOSTCTL_NET_*` forms are
  canonical — that is correct and should stay).

## Recommended `.env.example` / docs surface (categories 1 + justified 2)

`.env.example` active entries: `HOSTCTL_NET_PORT`, `HOSTCTL_NET_BAUD`,
`HOSTCTL_NET_SSID`, `HOSTCTL_NET_PASSWORD` (unchanged — current template is correct).
Commented optional catalog should list only:
`HOSTCTL_PORT`, `HOSTCTL_BAUD`, `HOSTCTL_PORT_HINT`,
`HOSTCTL_NET_LOG_PATH`, `HOSTCTL_NET_CYCLES`, `HOSTCTL_NET_SOAK_CYCLES`,
`HOSTCTL_NET_ALLOW_LOG_APPEND`, `HOSTCTL_NET_LOCK_WAIT_SEC`,
`HOSTCTL_NET_SKIP_HOST_WIFI_CHECK`, `HOSTCTL_UPLOAD_TOKEN`,
`HOSTCTL_UPLOAD_MODE`, `HOSTCTL_UPLOAD_CHUNK_SIZE`,
`HOSTCTL_UPLOAD_SEND_DIAG`, `HOSTCTL_UPLOAD_SEND_DIAG_PATH`,
`HOSTCTL_HOST_RUSTUP_TOOLCHAIN`.

`docs/guides/wifi-asset-upload.md` and `docs/guides/development-setup.md` should
document the same set plus the diag/experiment-guard knobs they already cover
(`HOSTCTL_UPLOAD_SEND_DIAG_DEEP`, `HOSTCTL_EXPERIMENT_NOVELTY_*`,
`HOSTCTL_NET_PANIC_AUTO_TROUBLESHOOT`, `HOSTCTL_NET_REGRESSION_OUTPUT_DIR`).

**Drop from template/docs (category 3):** the `.env.example` commented entries
`HOSTCTL_NET_LOCK_PATH`, `HOSTCTL_NET_ENFORCE_POLICY_FLOORS` (pending the cat-4 fix),
`HOSTCTL_NET_PANIC_CONTEXT_LINES`, `HOSTCTL_NET_PANIC_EXCERPT_PATH`,
`HOSTCTL_NET_REGRESSION_REPORT_PATH`; the dead `HOSTCTL_NET_POLICY_PATH` from the
`wifi-asset-upload.md` command examples (lines ~84, ~191); and the decided blackout
A/B knob block (`DISABLE_POOL`, `FORCE_CONN_CLOSE`, `FRESH_CLIENT_PER_UPLOAD`,
`TCP_NODELAY`, `PRE_PUT_DELAY_MS`, `NET_RECOVERY_CONSECUTIVE_HEALTH`,
`NET_REUSE_UPLOAD_CLIENT`, `NET_REQ_READ_BODY_RESET_MAX_DELTA`,
`HOSTCTL_NET_LOCK_PATH`) from `wifi-asset-upload.md`.
