# Hardware Test Matrix (ESP-HAL)

This checklist is the Phase 6 validation gate for the current `esp-hal` firmware.

## Environment

- Board: Inkplate 4 TEMPERA (ESP32)
- MCU module: ESP32-WROVER-E ([CNX Software, 2023-10-04](https://www.cnx-software.com/2023/10/04/inkplate-4-tempera-epaper-display-supports-esphome-arduino-and-micropython/))
- Port: `scripts/hostctl.sh` invocations use `HOSTCTL_PORT=/dev/cu.usbserial-540`; espflash-based soak/cold-boot scripts use `ESPFLASH_PORT=/dev/cu.usbserial-540`
- Firmware: current `debug` build from `scripts/device/flash.sh debug` (wrapper over `hostctl flash-capture`)
- Boot-capture artifacts: `logs/.../flash.log`, `capture.log`, `summary.txt` from the most recent flash-capture run

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
HOSTCTL_PORT=/dev/cu.usbserial-540 scripts/hostctl.sh test sdcard-hw
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
HOSTCTL_PORT=/dev/cu.usbserial-540 HOSTCTL_SDCARD_FLASH_FIRST=1 scripts/hostctl.sh test sdcard-hw --build-mode debug
```

Burst/backpressure regression only:

```bash
HOSTCTL_PORT=/dev/cu.usbserial-540 scripts/hostctl.sh test sdcard-burst-regression
```

Suite selection:

```bash
HOSTCTL_PORT=/dev/cu.usbserial-540 scripts/hostctl.sh test sdcard-hw --suite baseline
HOSTCTL_PORT=/dev/cu.usbserial-540 scripts/hostctl.sh test sdcard-hw --suite burst
HOSTCTL_PORT=/dev/cu.usbserial-540 scripts/hostctl.sh test sdcard-hw --suite failures
HOSTCTL_PORT=/dev/cu.usbserial-540 scripts/hostctl.sh test sdcard-hw --suite no-card
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

## 2C. DMA SD/FAT Gate

DMA is the sole SD backend. Build both profiles without changing the established 36 MHz data-clock
default:

```bash
scripts/build/build.sh debug
scripts/build/build.sh release
```

The physical devices have different storage capabilities, so they have different mandatory lanes:

| Device | SD card | Debug and release acceptance |
| --- | --- | --- |
| Device 1 | Present | Full `cutover` suite plus Wi-Fi/upload 1-, 3-, and 10-cycle gates |
| Device 2 | Absent | `no-card` suite plus runtime-mode, display/IMU/touch/shared-I2C, and Wi-Fi discovery gates |

For Device 1, flash each profile with `scripts/device/flash.sh`, enable `TELEMSET SD ON`, and send
`TOUCHSCHEDRESET` immediately before the workload. Run 20 probes, the complete SD suite, expected
failures, burst regression, 20 nested-directory writes, and the Wi-Fi/upload 1-, 3-, and 10-cycle
gates. Preserve both `stack_diag` and `touch_core_stack_diag` minima in the summary.

For Device 2, run `scripts/hostctl.sh test sdcard-hw --suite no-card`. It must complete 20
absent-card probes with `status=error code=init_failed` after bounded `NoResponse` initialization
attempts, then pass the same stack, internal-memory, touch-scheduling, panic, reset, and
true-timeout checks. Also run
`scripts/hostctl.sh test runtime-modes-smoke --suite no-storage` and Wi-Fi discovery.
Device 2's lane does not count as SD/FAT correctness or upload-throughput evidence.

Pass criteria:

- `active_gap_max_ms <= 16`;
- minimum stack headroom is 8 KiB, with 12 KiB as the target;
- minimum dedicated touch-core stack headroom is 1 KiB;
- internal free memory is at least 16 KiB;
- normal uploads retain `cmd25_fallback_bursts=0`;
- no panic, reset, timeout, stale test directory, or incomplete upload session;
- median upload throughput is no more than 10% below the latest valid baseline.

Device-1 final evidence on 2026-07-17 passes debug and release. Debug recorded 28,408-byte main
and 3,332-byte touch-core stack minima; release recorded 24,712 and 3,220 bytes. Ten-cycle upload
averages were 138.03 KiB/s debug and 190.41 KiB/s release, with 4 ms and 3 ms touch loop maxima.
Device-2 debug and release no-card/runtime lanes remain required before promotion. Promotion
requires Device 1's full storage lanes and Device 2's capability-appropriate lanes; an absent card
on Device 2 is a recorded hardware limitation, not an SD test pass.

## 2D. Adaptive IMU Acquisition

Build and flash the default `odr=416`, `idle=20`, `active=125` configuration, then issue `METRICS` before and after each scenario.

Scenarios:

1. Leave the device still until the scheduler demotes to idle.
2. Perform intended tap sequences on each tested enclosure side.
3. Perform touch-only swipes, placement motion, and large swings without taps.
4. Trigger full and partial display refreshes while collecting metrics.
5. Enter and leave upload mode.

Pass criteria:

- the first tap promotes acquisition and the scheduler later demotes after the configured hold;
- `IMU_SCHED active_n` advances independently during display refresh;
- touch contact increments `touch_skip` without I2C faults or worse touch scheduling;
- upload mode increments `upload_skip` and resume records a discontinuity;
- intended tap actions occur once, with no stale action after upload;
- transient I2C failure enters retry and recovers without reboot.

Repeat the cadence check with `40/80` and `100/125` configurations before changing defaults.

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
