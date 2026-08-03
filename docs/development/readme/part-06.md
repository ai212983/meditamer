# Part 06: Advanced Wi-Fi Diagnostics

Additional Wi-Fi blackout diagnostics and regression-gate notes are kept here as
a distinct advanced-diagnostics topic.

## Diagnostic Knobs

- hostctl wrappers execute from `/tmp`; relative `HOSTCTL_NET_LOG_PATH` values are
  written under `/tmp` (not repo `./logs`). Use absolute paths for deterministic
  artifact location, or copy resulting files into repo `logs/` before documenting.
- Wi-Fi discovery country override diagnostics (firmware build-time knob):
  - `MEDITAMER_WIFI_COUNTRY_US_OVERRIDE=1` (default `0`)
  - latest decision: keep default off (`US` override did not recover bounded
    all-zero discovery recurrence in Step 65 A/B).
- Wi-Fi scan-entry driver-state diagnostics (firmware build-time knob):
  - `MEDITAMER_WIFI_SCAN_ENTRY_DRIVER_STATE_DIAG=1` (default `0`)
  - emits low-level `esp_wifi_get_protocol/get_country` snapshot at scan entry;
    keep default off (diagnostic-only).
- Wi-Fi scan-entry direct-IDF comparator diagnostics (firmware build-time knob):
  - `MEDITAMER_WIFI_SCAN_ENTRY_IDF_COMPARE_DIAG=1` (default `0`)
  - runs direct `esp_wifi_scan_start/get_ap_num/get_ap_records` snapshot at scan
    entry for comparator logging; keep default off (diagnostic-only).
- Wi-Fi scan-entry promiscuous RX sweep diagnostics (firmware build-time knob):
  - `MEDITAMER_WIFI_SCAN_ENTRY_PROMISC_DIAG=1` (default `0`)
  - optional dwell override:
    `MEDITAMER_WIFI_SCAN_ENTRY_PROMISC_DIAG_DWELL_MS=<50..3000>` (default `120`)
  - runs channel sweep (`8/1/6/11`) with promiscuous packet counters at scan
    entry; keep default off (diagnostic-only).
- Wi-Fi first-start internal log bump diagnostics (firmware build-time knob):
  - `MEDITAMER_WIFI_FIRST_START_IDF_LOG_DIAG=1` (default `0`)
  - temporarily raises Wi-Fi internal driver log level to `DEBUG` from first
    `start_ok` until first `scan_done`, then restores `INFO`
  - intended only for first-start blackout capture; keep default off
    (diagnostic-only).
- Wi-Fi C-like discovery-start diagnostics (firmware build-time knob):
  - `MEDITAMER_WIFI_C_LIKE_DISCOVERY_START=1` (default `0`)
  - starts bare `WIFI_MODE_STA` before first scan and delays full station
    config until after the first raw post-start scan
  - latest bounded first-start repro still showed zero promiscuous RX and zero
    raw broad scan before config application; keep default off and treat as a
    rejected blackout-root-cause branch.
- Wi-Fi boot scan-only diagnostics (firmware build-time knob):
  - `MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG=1` (default `0`)
  - runs one boot-time Rust `esp-radio` control path before `NETCFG` wait:
    `set_mode(STA) -> start -> broad scan -> stop`
  - latest bounded boot capture still returned `result_count=0`; keep default
    off and use only when isolating Rust bring-up below the upload state machine.
- Wi-Fi `esp-radio` ISR-context predicate diagnostics (firmware build-time knob):
  - `MEDITAMER_WIFI_ESP_RADIO_USE_REAL_ISR_CHECK=1` (default `0`)
  - changes Wi-Fi OS-adapter `is_from_isr()` from unconditional `true` to
    `crate::is_interrupts_disabled()` for bounded A/B work
  - latest boot-scan comparator still returned zero Rust scan and zero direct
    IDF compare APs; keep default off and treat as rejected root-cause branch.
- Wi-Fi `esp-radio` IDF-like init defaults without NVS (firmware build-time knob):
  - `MEDITAMER_WIFI_ESP_RADIO_USE_IDF_INIT_DEFAULTS_NO_NVS=1` (default `0`)
  - uses the remaining IDF-style `wifi_init_config_t` defaults while forcing
    `nvs_enable=0` to avoid the known `nvs_open` panic in the no-std adapter
  - latest boot-scan comparator still returned zero Rust scan and zero direct
    IDF compare APs; keep default off and treat as rejected root-cause branch.
- Wi-Fi setup-time drop/reinit diagnostics (firmware build-time knob):
  - `MEDITAMER_WIFI_SETUP_REINIT_DIAG=1` (default `0`)
  - performs a one-time `esp_radio::wifi::new(...)` -> drop/deinit ->
    `esp_radio::wifi::new(...)` cycle during upload runtime setup before the
    network stack is built
  - latest bounded probe regressed device liveness after flash (no serial
    console response, `espflash monitor` reset/connect failure); keep default
    off and treat as rejected for blackout recovery.
- Wi-Fi post-stop mode bounce diagnostics (firmware build-time knob):
  - `MEDITAMER_WIFI_MODE_NULL_STA_RESET_AFTER_STOP=1` (default `0`)
  - applies `WIFI_MODE_NULL -> WIFI_MODE_STA` after stop during hard recovery;
    latest Step 65 bounded A/B showed no discovery recovery (keep default off).
- Wi-Fi zero-discovery software-reset guards (firmware build-time knobs):
  - `MEDITAMER_WIFI_SOFTWARE_RESET_ON_ZERO_DISCOVERY_TERMINAL=1` (default `0`)
  - `MEDITAMER_WIFI_SOFTWARE_RESET_ON_ZERO_DISCOVERY_HARD_GUARD=1` (default `0`)
  - hard-guard reset path is experimental and requires matching host expected
    reboot handling; keep non-default while validation remains in progress.
- do not use `esp_wifi_restore()` as a recovery primitive in this stack path:
  - attempted branch panicked at runtime (`not yet implemented:
    misc_nvs_restore` in `esp-radio`).

## Regression Gate

Wi-Fi zero-discovery diagnostic workflow:

```bash
HOSTCTL_NET_PORT=/dev/cu.usbserial-540 \
HOSTCTL_NET_BAUD=115200 \
HOSTCTL_NET_SSID='<wifi-ssid>' \
HOSTCTL_NET_PASSWORD='<wifi-password>' \
HOSTCTL_NET_POLICY_PATH=./tools/hostctl/scenarios/wifi-policy.default.json \
HOSTCTL_NET_DISCOVERY_PROFILE_PATH=./tools/hostctl/scenarios/wifi-discovery-debug.default.toml \
HOSTCTL_NET_LOG_PATH=./logs/wifi_discovery_debug_manual.log \
scripts/tests/hw/test_wifi_discovery_debug.sh
```

- runs via `hostctl test wifi-discovery-debug` behind the script wrapper.
- strategy and pass/fail thresholds are declarative TOML in
  `tools/hostctl/scenarios/wifi-discovery-debug.default.toml`.
- default discovery profile temporarily disables HTTP listener during probe rounds
  (`disable_listener_during_probe_rounds=true`) to reduce radio/memory pressure
  while preserving Wi-Fi discovery.
- workflow orchestration remains declarative in
  `tools/hostctl/scenarios/wifi-discovery-debug.sw.yaml`.
- reports round-level counters for:
  - zero-result scan events
  - non-zero scan events
  - `no_ap_found` disconnect events
  - target SSID visibility.
- root-cause and guardrails reference:
  `docs/development/wifi-discovery-regression-guardrails.md`.

Wi-Fi/upload regression gate (panic-first, fail-fast):

```bash
scripts/tests/hw/test_wifi_regression_gate.sh
```

- sequence: discovery debug -> acceptance 1-cycle -> acceptance 3-cycle -> optional soak
- emits per-stage logs and machine-readable `report.json`
- when panic/reboot markers are detected, the gate captures panic excerpt and can auto-run troubleshoot workflow

Optional regression-gate env vars:

- `HOSTCTL_NET_SOAK_CYCLES` (`0` default; skip soak)
- `HOSTCTL_NET_PANIC_AUTO_TROUBLESHOOT` (`1` default)
- `HOSTCTL_NET_PANIC_CONTEXT_LINES` (`80` default)
- `HOSTCTL_NET_PANIC_EXCERPT_PATH` (optional override path)
- `HOSTCTL_NET_REGRESSION_REPORT_PATH` (optional override path)
- `HOSTCTL_NET_REQ_READ_BODY_RESET_MAX_DELTA` (`0` default; fail run if `METRICS UPLOAD req_read_body_reset` increases more than this delta)

Wi-Fi workflow guardrail env vars:

- `HOSTCTL_NET_LOCK_PATH` (optional lock file path override)
- `HOSTCTL_NET_LOCK_WAIT_SEC` (`0` default; fail-fast lock)
- `HOSTCTL_NET_ALLOW_LOG_APPEND` (`0` default; enforce unique log path)
- `HOSTCTL_NET_ENFORCE_POLICY_FLOORS` (`1` default)
- `HOSTCTL_EXPERIMENT_NOVELTY_GUARD` (`1` default; set `0` to bypass decision-ledger guard)
- `HOSTCTL_EXPERIMENT_NOVELTY_OVERRIDE` (`0` default; set `1` to allow intentional reruns of already-decided knobs)
