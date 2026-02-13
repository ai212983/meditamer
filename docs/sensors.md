# Inkplate 4 TEMPERA Sensor Notes

This project targets **Inkplate 4 TEMPERA** (`src/main.rs` prints this explicitly), and the parent Arduino reference library confirms the onboard sensor/peripheral set.

## Onboard sensor inventory

| Sensor area | Chip / controller | Typical I2C address | Data exposed |
| --- | --- | --- | --- |
| Gesture + proximity + light + color | APDS9960 | `0x39` | Gesture (`UP/DOWN/LEFT/RIGHT/NEAR/FAR`), proximity, ambient light, RGB channels |
| Environmental | BME688 | `0x76` or `0x77` (auto-detected by library) | Temperature, humidity, pressure, gas resistance, derived altitude |
| IMU | LSM6DS3 | `0x6B` (default in wrapper) | 3-axis accel + 3-axis gyro |
| Fuel gauge | BQ27441-G1A | `0x55` | SoC, voltage, current, capacity, power, health |
| Touchscreen (2-point capacitive touch) | Elan controller | `0x15` | Touch count + XY coordinates |
| Panel temperature (PMIC internal sensor) | TPS65186 | `0x48` | EPD power IC temperature register (used by `readTemperature()`) |
| Battery voltage (ADC path, not dedicated IC) | ESP32 ADC + divider/mosfet gating | GPIO35 path | Pack voltage estimate used by `readBattery()` |

## Practical bring-up details

- APDS9960, BME688, LSM6DS3, and Fuel Gauge are put to sleep on board init in the Arduino reference and are expected to be woken before use.
- Wake/sleep API in reference: `wakePeripheral(...)` / `sleepPeripheral(...)`.
- Wake flags in board header:
  - `INKPLATE_ACCELEROMETER = 0x02`
  - `INKPLATE_BME688 = 0x04`
  - `INKPLATE_APDS9960 = 0x08`
  - `INKPLATE_FUEL_GAUGE = 0x01`
- Note: `INKPLATE_FUEL_GAUGE` shares value `0x01` with `INKPLATE_BUZZER` in the board header. For sensor wake/sleep logic this still works because only fuel-gauge handling exists in those methods.
- Touchscreen bring-up sequence in reference is `tsInit(true)`, then poll `tsAvailable()` and read with `tsGetData(x, y)`.

## Current Rust status in this repo

- Implemented now:
  - I2C bus infrastructure (`src/lib.rs`)
  - Internal/External IO expander setup (`0x20`/`0x21`)
  - EPD PMIC control (`0x48`), frontlight, buzzer
  - Sensor interrupt pins are configured (`INT_APDS`, `INT1_LSM`, `INT2_LSM`, `FG_GPOUT`)
- Not implemented yet:
  - Actual APDS9960/BME688/LSM6DS3/BQ27441 drivers
  - Touchscreen driver (`TS_ADDR = 0x15`) integration
  - PMIC temperature read helper equivalent to Arduino `readTemperature()`

## Suggested implementation order (Rust)

1. BME688 (straightforward polling path, easy validation with stable readings)
2. LSM6DS3 (simple WHO_AM_I + accel/gyro reads)
3. APDS9960 (more register/config work; start with proximity/ambient before gestures)
4. BQ27441 fuel gauge (requires capacity setup for realistic SoC)
5. Touchscreen (interrupt + coordinate transform handling)
6. PMIC temperature helper (small utility, useful for diagnostics)

## Useful interaction ideas

Prefer sensor fusion where possible: combining at least two independent signals (motion + proximity, touch + orientation, etc.) is usually more robust than single-sensor thresholds.

| Interaction | Type | Sensor input(s) | Fusion / trigger logic (example) | Device response |
| --- | --- | --- | --- | --- |
| Day/night mode switch | Environment | APDS9960 ambient light + RGB, BME688 pressure trend | Lux threshold with hysteresis, ignore brief shadows, optionally bias with weather trend | Change refresh cadence, brightness theme, and frontlight default |
| Sunrise/sunset transition smoothing | Environment | APDS9960 ambient + RGB, BME688 temperature trend | Slow trend over 10-30 min plus temperature drift consistency check | Fade UI contrast and frontlight gradually to avoid abrupt changes |
| Indoor air comfort indicator | Environment | BME688 temperature + humidity + gas, APDS9960 ambient | Compute comfort/VOC proxy and suppress notifications at night (low lux) | Show subtle status badge and suggest short ventilation break |
| Weather-shift anticipation | Environment | BME688 pressure + humidity, LSM6DS3 motion state | Pressure/humidity change over 1-3 hours, only alert when device is actively used | Adjust meditation/session suggestions (calmer guidance on rapid drops) |
| Movement-aware power save | Environment | LSM6DS3 accel + gyro, APDS9960 proximity, touch inactivity | No meaningful motion + no near-hand + no touches for N minutes | Reduce polling, defer full refreshes, dim frontlight |
| Pick-up wake | User | LSM6DS3 accel + gyro, APDS9960 proximity/light | Orientation change + accel spike + hand-near or light-change confirmation | Wake screen quickly to most relevant card |
| Double-tap on enclosure | User | LSM6DS3 high-frequency accel, touch controller state | Two impulse peaks within short window while no touch points are active | Toggle quick action (pause/resume, bookmark, or chime) |
| Gesture page navigation | User | APDS9960 gesture, LSM6DS3 stability check | `LEFT/RIGHT` gesture accepted only when device is relatively stationary | Previous/next page without touching display |
| Near/far interaction mode | User | APDS9960 proximity + ambient, touchscreen activity | Hand enters/leaves threshold and no active touch conflict | Show contextual controls on approach, hide on leave |
| Touch-first focus interaction | User | Touch XY/count, LSM6DS3 orientation, APDS9960 proximity | Interpret tap/long-press/2-finger differently by orientation and hand-near state | Tap to open, long press for options, two-finger for quick settings |
| Intentional silence / do-not-disturb | User | APDS9960 `FAR`, touch long press, LSM6DS3 stable pose | Long press + hand-away + stationary pose for 2-3s confirmation | Mute buzzer/haptics and reduce non-critical prompts |
| Low-battery protective mode | Environment | BQ27441 SoC/current, ADC voltage, LSM6DS3 activity level | SoC below threshold or high discharge, with stronger limits when active motion is low | Reduce sensor sampling and frontlight, switch to low-power UI |
| Thermal safeguard behavior | Environment | PMIC temperature (`TPS65186`), BME688 temp + humidity | Over-temp threshold with humidity-aware margin to avoid rapid oscillation | Delay expensive refresh cycles and show thermal warning |

## Reference locations used

- `../Inkplate-Arduino-library/src/Inkplate.h`
- `../Inkplate-Arduino-library/src/boards/Inkplate4TEMPERA.h`
- `../Inkplate-Arduino-library/src/boards/Inkplate4TEMPERA.cpp`
- `../Inkplate-Arduino-library/src/include/TouchElan.h`
- `../Inkplate-Arduino-library/src/include/TouchElan.cpp`
- `../Inkplate-Arduino-library/examples/Inkplate4TEMPERA/Advanced/Sensors/*`
