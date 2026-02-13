# Hardware Test Matrix (ESP-HAL)

This checklist is the Phase 6 validation gate for the current `esp-hal` firmware.

## Environment

- Board: Inkplate 4 TEMPERA (ESP32)
- Port: `ESPFLASH_PORT=/dev/cu.usbserial-540` (adjust for your host)
- Firmware: current `debug` build from `scripts/flash.sh debug`

## 1. Reset-Cycle Soak (Automated)

Command:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 SOAK_WINDOW_SEC=8 scripts/soak_boot.sh 20
```

Pass criteria:

- `pass=20 fail=0`
- No missing required markers in any cycle log
- `display uptime screen: ok` is optional in reset soak by default; enable strict mode with `SOAK_REQUIRE_UPTIME=1` if needed.

## 2. Cold Boot Cycles (Manual)

Procedure:

1. Run helper:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/cold_boot_matrix.sh 20
```

For slow boot/display bring-up paths, increase timing guards:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 \
COLD_BOOT_CONNECT_TIMEOUT_SEC=50 \
COLD_BOOT_WINDOW_SEC=60 \
scripts/cold_boot_matrix.sh 20
```

2. For each prompted cycle:
- physically disconnect power
- wait ~5 seconds
- press Enter to arm capture
- reconnect power immediately after pressing Enter

Pass criteria:

- 20/20 cycles with all required markers
- No boot hang or reset loop

## 3. Long Refresh Stability

Goal: validate display loop stability over time.

Procedure:

1. Flash debug build.
2. Run:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-540 scripts/soak_refresh.sh 7200
```

3. Verify summary output and saved log path.

Pass criteria:

- No panic/reboot
- Continuous refresh log output for full run

## 4. Frontlight/Buzzer Repetition

Procedure:

- Add a temporary diagnostic branch that loops frontlight write/beep 100 times, or trigger equivalent app path.
- Capture monitor output and final count summary.

Pass criteria:

- 100 successful frontlight writes
- 100 successful buzzer operations
- No persistent I2C lockup

## 5. I2C Fault Recovery

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
- Reset-cycle soak result:
- Cold boot result:
- Long refresh result:
- Frontlight/buzzer repetition result:
- I2C fault recovery result:
- Open issues:
