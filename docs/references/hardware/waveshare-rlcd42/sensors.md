# Waveshare ESP32-S3-RLCD-4.2 Sensor Notes

A much smaller inventory than the Inkplate's — no IMU, no gesture sensor, no fuel gauge, no
touchscreen. Everything on the bus is on **I²C0, GPIO13 `SDA` / GPIO14 `SCL`**, one shared bus behind
an `embassy_sync` mutex, with a 40 ms software transaction timeout so a device that never ACKs
cannot hang the board.

## I²C inventory

An exact-board scan finds four devices, and all four are accounted for:

| Address | Chip | Role | Status here |
| --- | --- | --- | --- |
| `0x18` | ES8311 | Low-power audio codec (speaker DAC) | not implemented — see [sound.md](sound.md) |
| `0x40` | ES7210 | Dual-mic ADC with echo cancellation | not implemented — see [sound.md](sound.md) |
| `0x51` | PCF85063A | Real-time clock | **implemented** |
| `0x70` | SHTC3 | Temperature and humidity | **implemented** |

`0x40` is worth calling out: it is the address an INA219 current monitor would occupy, and it is
**not** one. There is no current-sense hardware on this board.

## SHTC3 — temperature and humidity

Driver: [boards/waveshare-rlcd42/shtc3](../../../../boards/waveshare-rlcd42/shtc3/src/lib.rs).
Generic over `embedded_hal_async::i2c::I2c`, shaped like `platform/time/rtc`, so it names no chip and the
CRC and fixed-point conversions are testable on the host.

- Fixed 7-bit address `0x70`; no address strap.
- Commands: `0xEFC8` read-ID, `0x805D` soft reset, `0xB098` sleep, `0x3517` wake-up, `0x7866`
  measure (temperature first, **clock stretching disabled** so the bus is never held).
- CRC-8, polynomial `0x31`, init `0xFF` — Sensirion's usual parameters. A word whose checksum fails
  is discarded rather than reported, because a corrupt humidity reading looks entirely plausible.
- Conversions, in fixed point to keep floats off the device:
  `T_m°C = 175000 * raw / 2^16 - 45000`, `RH_m% = 100000 * raw / 2^16`.

**Readings are returned uncorrected.** Waveshare's own code subtracts a fixed 4 °C for self-heating,
but that is a property of where the sensor sits relative to warm components *on this board*, not of
the part. The offset is applied by the product instead —
`medinote::config::SELF_HEATING_MC = 4_000` — so a driver shared with another board is not silently
wrong there.

Acquisition is a target-side observation provider
([environment.rs](../../../../targets/medinote-waveshare/src/environment.rs)) rather than an inline
read, so a conversion never blocks button sampling or rendering.

## PCF85063A — wall clock

Driver: [platform/time/rtc](../../../../platform/time/rtc/src/lib.rs) — shared with the Inkplate, which
carries the same part. Address `0x51`. Register-level authority is NXP's
[PCF85063A datasheet](https://www.nxp.com/docs/en/data-sheet/PCF85063A.pdf).

- Two-digit BCD year, so the calendar code covers a 2000–2099 window.
- The chip's one byte of undedicated free RAM holds the fixed local UTC offset, encoded by
  [`offset.rs`](../../../../platform/time/rtc/src/offset.rs). The marker byte prior Arduino PCF85063A
  libraries write there ("time was set") is never confused with a real offset.
- **`RTC_INT` is wired to GPIO15.** The schematic's pin table names it; none of the online board
  configs mention it, and nothing here drives it. It is the alarm/timer interrupt output, so it is
  the pin an alarm-driven wake would use instead of the current timer failsafe.
- Board-side backup: a PH1.0 rechargeable cell keeps the RTC running with main power removed. The
  backup-power retention must be checked separately from
  [software-controlled sleep](../../../guides/observations/validation.md#medinote-sleep-and-synthetic-cleanup-rejection).

The clock is provisioned over the USB-Serial-JTAG handshake described in
[host clock synchronization](../../hostctl.md#flash-capture), not over the network — a `TIME_REQUEST` at cold boot,
answered once. A valid RTC means no `TIME_REQUEST` is printed on later wakes, which is exactly what
the deep-sleep retention test asserts.

## Battery voltage

No fuel gauge. Pack voltage only, through **ADC1 channel 3 on GPIO4** behind a fixed **3x divider**:
`millivolts = pin_mv * 3`. Attenuation `_11dB`, curve calibration (`AdcCalCurve`).

State of charge is a linear 3.0 V–4.12 V map in
[`medinote::presentation::battery_percent_from_mv`](../../../../products/medinote/src/presentation.rs);
that is a presentation approximation, not a coulomb count, and it should not be quoted as one.

The reading is a provider task
([battery.rs](../../../../targets/medinote-waveshare/src/battery.rs)) rather than a blocking read:
esp-hal's `read_blocking` is a genuine busy-spin, so the driver polls `read_oneshot` and yields
between polls under a 50 ms wall-clock bound, with a drop guard that cancels an in-flight conversion.

Charge management is on the 18650 path with `CHG` and `WRN` indicator LEDs. Neither the charger nor
the LEDs is visible to firmware.

## What this board does not have

Worth stating plainly, because the Inkplate's inventory sets a different expectation and code shared
between the two must not assume it:

- No IMU — no tap, orientation, or pick-up detection.
- No APDS9960 — no ambient light, proximity, colour, or gesture input.
- No pressure or gas sensing (the SHTC3 is temperature and humidity only).
- No touchscreen. Input is one `KEY` button, plus BLE HID from a paired remote
  ([cheertok.rs](../../../../targets/medinote-waveshare/src/cheertok.rs)).
- No fuel gauge and no current sense.
- No buzzer or haptics. Sound, if it is ever built, goes through the audio codec —
  see [sound.md](sound.md).

## Sources

- [SHTC3 datasheet](https://files.waveshare.com/wiki/common/SHTC3_Datasheet.pdf) (Waveshare mirror)
- [PCF85063A datasheet](https://www.nxp.com/docs/en/data-sheet/PCF85063A.pdf) (NXP)
- Waveshare's ESP-IDF examples in
  [waveshareteam/ESP32-S3-RLCD-4.2](https://github.com/waveshareteam/ESP32-S3-RLCD-4.2) — the
  authoritative source for the SHTC3 command values and timings as wired on this board
- [historical ADR-0015](../../../archive/architecture/0015-two-product-platform-workspace.md) — the I²C scan and the
  self-heating decision
