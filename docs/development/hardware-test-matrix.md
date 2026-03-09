# Hardware Test Matrix (ESP-HAL)

This checklist is the Phase 6 validation gate for the current `esp-hal` firmware.

## Environment

- Board: Inkplate 4 TEMPERA (ESP32)
- MCU module: ESP32-WROVER-E ([CNX Software, 2023-10-04](https://www.cnx-software.com/2023/10/04/inkplate-4-tempera-epaper-display-supports-esphome-arduino-and-micropython/))
- Port: hostctl wrappers use `HOSTCTL_PORT=/dev/cu.usbserial-540`; espflash-based soak/cold-boot scripts use `ESPFLASH_PORT=/dev/cu.usbserial-540`
- Firmware: current `debug` build from `scripts/device/flash.sh debug`

## 1. Reset-Cycle Soak (Automated)

Command:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 SOAK_WINDOW_SEC=8 scripts/device/soak_boot.sh 20
```

Pass criteria:

- `pass=20 fail=0`
- No missing required markers in any cycle log
- `display uptime screen: ok` is optional in reset soak by default; enable strict mode with `SOAK_REQUIRE_UPTIME=1` if needed.

## 2. SD Card I/O Validation (Automated)

Command:

```bash
HOSTCTL_PORT=/dev/cu.usbserial-540 scripts/tests/hw/test_sdcard_hw.sh
```

Pass criteria:

- Script exits with `SD-card hardware test passed`
- Log contains successful operations for:
  - probe (`sdprobe[manual]: card_detected`)
  - FAT flow (`mkdir_ok`, `write_ok`, `read_ok`, `append_ok`, `stat_ok`, `trunc_ok`, `ren_ok`, `rm_ok`)
  - raw sector verify (`sdrw[manual]: verify_ok`)
  - burst/backpressure flow with no `SDFAT* BUSY` in burst window
  - failure-path checks (`rm_error ... NotEmpty`, `ren_error ... AlreadyExists`, `read_error ... NotFound`, `sdrw[manual]: refused_lba0`, `CMD ERR` for oversized payload)
  - `SDWAIT DONE ... code=` values match expected outcomes (`ok`, `operation_failed`, `not_found`, `refused_lba0`)

Default behavior does not flash firmware before running. To include flash in the run:

```bash
HOSTCTL_PORT=/dev/cu.usbserial-540 HOSTCTL_SDCARD_FLASH_FIRST=1 scripts/tests/hw/test_sdcard_hw.sh debug
```

Burst/backpressure regression only:

```bash
HOSTCTL_PORT=/dev/cu.usbserial-540 scripts/tests/hw/test_sdcard_burst_regression.sh
```

Suite selection:

```bash
HOSTCTL_PORT=/dev/cu.usbserial-540 HOSTCTL_SDCARD_SUITE=baseline scripts/tests/hw/test_sdcard_hw.sh
HOSTCTL_PORT=/dev/cu.usbserial-540 HOSTCTL_SDCARD_SUITE=burst scripts/tests/hw/test_sdcard_hw.sh
HOSTCTL_PORT=/dev/cu.usbserial-540 HOSTCTL_SDCARD_SUITE=failures scripts/tests/hw/test_sdcard_hw.sh
```

## 2B. Wi-Fi/Upload Regression Gate (Automated)

Canonical command:

```bash
HOSTCTL_NET_PORT=/dev/cu.usbserial-540 \
HOSTCTL_NET_BAUD=115200 \
HOSTCTL_NET_SSID='<wifi-ssid>' \
HOSTCTL_NET_PASSWORD='<wifi-password>' \
HOSTCTL_NET_POLICY_PATH=./tools/hostctl/scenarios/wifi-policy.default.json \
scripts/tests/hw/test_wifi_regression_gate.sh
```

What this gate runs:

1. discovery debug (bounded)
2. acceptance 1-cycle
3. acceptance 3-cycle
4. optional soak (`HOSTCTL_NET_SOAK_CYCLES`)

Pass criteria:

- final status is `passed`
- `report.json` exists and all required stage logs are present
- discovery counters satisfy:
  - `zero_discovery_rounds == 0`
  - `scan_nonzero_events > 0`
  - `ssid_seen_rounds > 0`
- panic/reboot signatures are absent

If panic is detected:

- preserve panic excerpt artifact
- run troubleshoot workflow once
- attach troubleshoot output with regression report

## 3. Cold Boot Cycles (Manual)

Procedure:

1. Run helper:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/device/cold_boot_matrix.sh 20
```

For slow boot/display bring-up paths, increase timing guards:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 \
COLD_BOOT_CONNECT_TIMEOUT_SEC=50 \
COLD_BOOT_WINDOW_SEC=60 \
scripts/device/cold_boot_matrix.sh 20
```

2. For each prompted cycle:
- physically disconnect power
- wait ~5 seconds
- press Enter to arm capture
- reconnect power immediately after pressing Enter

Pass criteria:

- 20/20 cycles with all required markers
- No boot hang or reset loop

## 4. Long Refresh Stability

Goal: validate display loop stability over time.

Procedure:

1. Flash debug build.
2. Run:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/device/soak_refresh.sh 7200
```

3. Verify summary output and saved log path.

Pass criteria:

- No panic/reboot
- Continuous refresh log output for full run

## 5. Frontlight/Buzzer Repetition

Procedure:

- Add a temporary diagnostic branch that loops frontlight write/beep 100 times, or trigger equivalent app path.
- Capture monitor output and final count summary.

Pass criteria:

- 100 successful frontlight writes
- 100 successful buzzer operations
- No persistent I2C lockup

## 6. I2C Fault Recovery

Procedure:

- During runtime, induce transient I2C stress (brief disconnect/noise, if hardware setup allows).
- Observe whether retries recover and normal operation resumes.

Pass criteria:

- System recovers without reboot in transient fault scenario
- If recovery fails, failure mode is explicit and reproducible in logs

## Reporting Template

- Date/time:
- Firmware commit:
- Test run ID:
- Wi-Fi/upload regression gate report path:
- Panic detected (yes/no):
- Panic class + first marker (if yes):
- Panic excerpt path (if yes):
- Troubleshoot run path/result (if panic):
- Reset-cycle soak result:
- Cold boot result:
- Long refresh result:
- Frontlight/buzzer repetition result:
- I2C fault recovery result:
- Open issues:
