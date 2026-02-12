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

## Reference locations used

- `../Inkplate-Arduino-library/src/Inkplate.h`
- `../Inkplate-Arduino-library/src/boards/Inkplate4TEMPERA.h`
- `../Inkplate-Arduino-library/src/boards/Inkplate4TEMPERA.cpp`
- `../Inkplate-Arduino-library/src/include/TouchElan.h`
- `../Inkplate-Arduino-library/src/include/TouchElan.cpp`
- `../Inkplate-Arduino-library/examples/Inkplate4TEMPERA/Advanced/Sensors/*`
