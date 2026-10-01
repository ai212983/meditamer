# Wi-Fi Blackout Diagnostics (archived)

Instrumentation built to isolate the Wi-Fi zero-discovery blackout. The fault is
resolved, so none of this is part of the normal workflow. It is kept for the
experiment novelty gate: every knob below records a decision, and most record a
rejected root-cause branch. Check here before re-running any of them.

The surviving live guard is [docs/guides/wifi-regression-gate.md](../../guides/wifi-regression-gate.md).

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

### C Wi-Fi Control App

Use the official-style ESP-IDF control app when you need to compare this board
against mature C Wi-Fi lifecycle behavior instead of the Rust `esp-radio`
stack:

```bash
scripts/device/wifi_control_idf.sh build
scripts/device/wifi_control_idf.sh flash
scripts/device/wifi_control_idf.sh monitor
```

Behavior:

- default build is scan-only because `CONFIG_WIFI_CONTROL_SSID` defaults empty
- if you later set a non-empty SSID/password in the app config, the same app
  switches to STA-connect mode

ESP-IDF selection:

- wrapper prefers `IDF_APP_ROOT` if set
- otherwise it auto-picks the newest local install under
  `.embuild/espressif/esp-idf/v*`
- for an external install, also export `IDF_TOOLS_PATH` before invoking the
  wrapper so `export.sh` uses the matching toolchain
- the wrapper now auto-resets a stale non-CMake
  `.embuild/idf_apps/wifi_control/build` directory left by failed early runs

Recommended when comparing against the current Wi-Fi blackout:

```bash
export IDF_APP_ROOT="$HOME/.esp-idf/v5.5.2"
export IDF_TOOLS_PATH="$HOME/.espressif"
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/device/wifi_control_idf.sh flash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/device/wifi_control_idf.sh monitor
```

### Wi-Fi Partition Dumps

Use the repo-local helper when debugging Wi-Fi discovery blackout or lower-level
flash state:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/device/dump_wifi_partitions.sh
```

The helper writes a timestamped artifact directory under `logs/flash_dumps/`
and captures:

- `nvs` MD5 and raw dump
- `phy_init` MD5 and raw dump
- first-byte hexdumps for both partitions
- stdout/stderr logs for every `espflash` command
- `summary.txt` with port, baud, read profile, MD5s, and output sizes

Default raw-read transport profile is intentionally conservative because it was
the stable path for `nvs` in blackout debugging:

- `ESPFLASH_BAUD=115200`
- `WIFI_FLASH_DUMP_BLOCK_SIZE=0x100`
- `WIFI_FLASH_DUMP_MAX_IN_FLIGHT=1`

Optional env vars:

- `WIFI_FLASH_DUMP_OUTPUT_ROOT` (default `./logs/flash_dumps`)
- `WIFI_FLASH_DUMP_TIMESTAMP` (default current local timestamp)
- `WIFI_FLASH_DUMP_NVS_ADDRESS` (default `0x9000`)
- `WIFI_FLASH_DUMP_NVS_LENGTH` (default `0x6000`)
- `WIFI_FLASH_DUMP_PHY_INIT_ADDRESS` (default `0xF000`)
- `WIFI_FLASH_DUMP_PHY_INIT_LENGTH` (default `0x1000`)
- `WIFI_FLASH_DUMP_HEXDUMP_BYTES` (default `128`)

Keep using repo-local absolute/anchored paths for artifacts. Do not rely on
wrapper defaults that may execute from `/tmp`.

## Rust ESP-IDF Wi-Fi Control Probe

Standalone Rust-on-ESP-IDF scan probe:

```bash
IDF_APP_ROOT="$HOME/.esp-idf/v5.3.4" \
IDF_TOOLS_PATH="$HOME/.espressif" \
scripts/device/wifi_control_idf_rust.sh build
```

Flash and monitor:

```bash
IDF_APP_ROOT="$HOME/.esp-idf/v5.3.4" \
IDF_TOOLS_PATH="$HOME/.espressif" \
ESPFLASH_PORT=/dev/cu.usbserial-540 \
scripts/device/wifi_control_idf_rust.sh flash

IDF_APP_ROOT="$HOME/.esp-idf/v5.3.4" \
IDF_TOOLS_PATH="$HOME/.espressif" \
ESPFLASH_PORT=/dev/cu.usbserial-540 \
scripts/device/wifi_control_idf_rust.sh monitor
```

Notes:

- current crate set (`esp-idf-svc 0.51`, `esp-idf-hal 0.45.2`) requires a
  supported ESP-IDF 5.3.x install for this probe
- ESP-IDF 5.5.2 currently fails this Rust probe build in `esp-idf-hal` on TWAI
  bindings, so use `v5.3.4` for the comparison path
