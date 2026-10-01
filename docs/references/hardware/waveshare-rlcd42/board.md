# Waveshare ESP32-S3-RLCD-4.2 Board Notes

Board support and target startup follow the
[code boundaries](../../../architecture/platform-board-product-target-boundaries.md).
Board support lives in
[boards/waveshare-rlcd42](../../../../boards/waveshare-rlcd42/src/lib.rs); chip startup and task
wiring live in [targets/medinote-waveshare](../../../../targets/medinote-waveshare/src/main.rs).

Product page: [waveshare.com/esp32-s3-rlcd-4.2.htm](https://www.waveshare.com/esp32-s3-rlcd-4.2.htm).
Vendor wiki: [docs.waveshare.com/ESP32-S3-RLCD-4.2](https://docs.waveshare.com/ESP32-S3-RLCD-4.2).

## Processor and memory

| Memory | Physical capacity | Notes |
| --- | ---: | --- |
| Internal SRAM | 512 KiB | ESP32-S3 dual-core Xtensa LX7, up to 240 MHz. |
| RTC SRAM | 16 KiB | 8 KiB RTC FAST plus 8 KiB RTC SLOW. |
| Mask ROM | 384 KiB | Fixed boot and core routines. |
| External SPI flash | 16 MiB | In-module (ESP32-S3-WROOM-1-N16R8). |
| External PSRAM | 8 MiB | In-module, octal. |

Module is **ESP32-S3-WROOM-1-N16R8**. Silicon figures come from Espressif's
[ESP32-S3 datasheet](https://documentation.espressif.com/esp32-s3_datasheet_en.pdf); the module
stack from the [vendor wiki](https://docs.waveshare.com/ESP32-S3-RLCD-4.2).

The 16 MiB is not exploited today. `targets/medinote-waveshare/partitions.csv` declares a single
2 MiB `factory` app slot with NVS and PHY ahead of it — this target deliberately does **not** use the
Inkplate's single-production bootloader/OTA workflow
([factory updater guide](../../../guides/development/firmware-update.md)); see
[build-and-flash.md](../../../guides/development/build-and-flash.md#medinote--waveshare-flash).

## Pin map

Taken from the **pin table on the board schematic**
([datasheets/](datasheets/esp32-s3-rlcd-4.2-schematic.pdf), top-right block), which is the
authority and which settles the places the online configs disagree. Net names in the middle column
are Waveshare's own. Rows marked **used** are the ones this repo actually drives.

| GPIO | Net | Function | Status here |
| ---: | --- | --- | --- |
| 0 | — | `BOOT` button, active-low | **used** — must stay free for the ROM download path |
| 1, 2, 3, 7, 17, 42 | — | Unassigned; the 2x8 expansion header | free |
| 4 | `BAT_ADC` | Battery voltage, ADC1 channel 3, through a 3x divider | **used** |
| 5 | `LCD_RS` | RLCD register select — the `DC` line | **used** |
| 6 | `LCD_TE` | RLCD tearing effect / frame pulse | not driven; see [panel.md](panel.md) |
| 8 | `I2S_DSDIN` | I²S data **to** the ES8311 DAC — speaker path | unused |
| 9 | `I2S_SCLK` | I²S bit clock | unused |
| 10 | `I2S_ASDOUT` | I²S data **from** the ES7210 ADC — microphone path | unused |
| 11 | `LCD_SCL` | SPI2 `SCK` to the panel | **used** |
| 12 | `LCD_SDA` | SPI2 `MOSI` to the panel | **used** |
| 13 | `RTC_SDA` / `ESP32_SDA` | I²C0 `SDA` | **used** |
| 14 | `RTC_SCL` / `ESP32_SCL` | I²C0 `SCL` | **used** |
| 15 | `RTC_INT` | PCF85063A interrupt output | unused — see [sensors.md](sensors.md) |
| 16 | `I2S_MCLK` | I²S master clock | unused |
| 18 | `KEY` | User button, active-low | **used** — also the EXT0 light-sleep wake source |
| 19 / 20 | `USB'_N` / `USB'_P` | Native USB D− / D+ | **used** — the console |
| 21 | `MOSI` | TF card data | unused |
| 38 | `SCK` | TF card clock | unused |
| 39 | `MISO` | TF card data | unused |
| 40 | `LCD_CS` | RLCD chip select | **used** |
| 41 | `LCD_RESET` | RLCD reset | **used** |
| 43 / 44 | `U0TXD` / `U0RXD` | UART0 | unused — the console is native USB-Serial-JTAG |
| 45 | `I2S_LRCK` | I²S word select | unused |
| 46 | `PA_CTRL` | Speaker amplifier enable, active-high | unused |

Three traps in that table, two of which are why the online configs conflict:

- **`LCD_SCL` / `LCD_SDA` are not an I²C bus.** GPIO11/12 are Waveshare's net names for the panel's
  SPI clock and data. The only I²C bus is GPIO13/14.
- **I²S direction is settled by the net names.** `DSDIN` is DAC-serial-data-**in** and `ASDOUT` is
  ADC-serial-data-**out**, both named from the codec's side: GPIO8 carries audio to the speaker,
  GPIO10 carries audio from the microphones. ESPHome has this right; Zephyr's pinctrl reads
  backwards.
- **The TF card is three wires, not four.** `MOSI`/`SCK`/`MISO` on GPIO21/38/39 with no chip select,
  which is 1-bit SD mode (Zephyr models the same three as SDHC `CMD`/`CLK`/`D0`, ≤20 MHz) rather
  than SPI mode, whatever the net names suggest. The vendor's own firmware settles it: its
  [SD example](https://github.com/waveshareteam/ESP32-S3-RLCD-4.2/blob/main/02_Example/ESP-IDF/06_SD_Card/components/port_bsp/sdcard_bsp.h)
  mounts the slot through `driver/sdmmc_host.h` with `clk = 38, cmd = 21, d0 = 39, width = 1`.
  Two further details the schematic gives that the pin table cannot: the socket's `CD` pin is
  tied to ground, so there is **no card-detect signal**, and the card's `VDD` goes straight to
  `VCC3V3` with **no load switch**, so a seated card cannot be powered down — unlike the
  Inkplate, whose SD rail is gated through its expander. That idle current sits on any
  deep-sleep budget.

One footnote to the chip-select trap, so nobody re-derives it from the schematic and mistakes it
for a finding. The SD-CARD block does carry an `SDCS` net on the socket's pin 2 (`CD/D3`), pulled
up to 3V3 like `CMD` and `D0`, and routed to GPIO17 through **R7 — a footprint marked `NC`, left
unpopulated**. Waveshare laid out an SPI-mode option and did not fit it. Stuffing R7 would make the
slot a conventional 4-wire SPI card, which the existing
[`platform/storage/sdcard`](../../../../platform/storage/sdcard/src/probe/mod.rs) SPI probe already drives on the
Inkplate. That is a genuine fallback, not the plan: it is a per-unit solder mod, it consumes a
header pin, and the native 1-bit path needs no rework and is what the vendor firmware proves. Read
it as an escape hatch if the SDMMC path ever fails on this hardware.

The last loose end here is resolved: Zephyr's `sitronix,st7306` was right and Waveshare's own
"ST7305" label is wrong — the board's init stays inside the ST7306's documented address ranges and
exceeds the ST7305's at every limit. Zephyr's 312 is still not the visible geometry, which is
400x300 landscape, confirmed on the device. See
[controller-identity.md](controller-identity.md).

The vendor's `02_Example/ESP-IDF/11_U8G2_Test/main/user_config.h` in
[waveshareteam/ESP32-S3-RLCD-4.2](https://github.com/waveshareteam/ESP32-S3-RLCD-4.2) agrees with
the schematic on every panel and I²C pin, and is the origin of the pin comment in
[main.rs](../../../../targets/medinote-waveshare/src/main.rs) — "SCK=11, MOSI=12, DC=5, CS=40,
RST=41 per Waveshare's user_config.h".

## Buttons and power

- `BOOT` (GPIO0) — hold through reset for the ROM serial download path. The firmware also reads it
  as an ordinary active-low input; it must never be repurposed in a way that blocks recovery.
- `KEY` (GPIO18) — the user button. Board-side ownership is
  [`buttons::Button`](../../../../boards/waveshare-rlcd42/src/buttons.rs), a thin active-low GPIO
  wrapper; debounce and interaction recognition deliberately live target-side in
  `targets/medinote-waveshare/src/input.rs` so the recognizer can use monotonic time.
- `PWR` — a hardware power button (long press off, click on). Not visible to firmware.
- `CHG` / `WRN` LEDs — charge and warning indicators, wired to the charge path, not to GPIO.
- 18650 holder for the main cell, plus a PH1.0 rechargeable backup cell for the RTC.

## Sleep

Sleep entry is target-local
([sleep.rs](../../../../targets/medinote-waveshare/src/sleep.rs)), because GPIO wake capability and
panel retention differ per board:

- **Light sleep** (`Sleep`) — EXT0 wake on GPIO18 low, with `rtcio_pullup(true)` applied after the
  ordinary input driver releases the pin, plus a timer failsafe. The panel keeps its image.
- **Deep sleep** — timer wake only. The board has no retained hardware bias on `CS`/`RST`, so the
  digital pad domain powering down blanks the panel. Attempts to hold GPIO40/GPIO41 through the pad
  hold path caused immediate wake instead. Current sleep qualification is in
  [observation validation](../../../guides/observations/validation.md#medinote-sleep-and-synthetic-cleanup-rejection).

## Console and time

No UART bridge — the console is the S3's **native USB-Serial-JTAG**, which means the port
disappears and reappears across reset. `esp-println` writes it through raw MMIO;
[`jtag_rx`](../../../../boards/waveshare-rlcd42/src/jtag_rx.rs) reads the same `EP1`/`EP1_CONF`
registers the same way rather than constructing esp-hal's `UsbSerialJtag` driver, whose first-claim
path resets the peripheral and breaks the connection.

That RX primitive is what carries the host wall-clock handshake
([host clock synchronization](../../hostctl.md#flash-capture)): the device prints `TIME_REQUEST`, the host answers
`TIME_REPLY`. `targets/medinote-waveshare/flash.sh` answers it automatically during its bounded boot
capture; [`scripts/reply_time_request.sh`](../../../../boards/waveshare-rlcd42/scripts/reply_time_request.sh)
answers it for a board already running.

## Sources

- [Waveshare wiki](https://docs.waveshare.com/ESP32-S3-RLCD-4.2) and its
  [resources page](https://docs.waveshare.com/ESP32-S3-RLCD-4.2/Resources-And-Documents)
- [waveshareteam/ESP32-S3-RLCD-4.2](https://github.com/waveshareteam/ESP32-S3-RLCD-4.2) — Arduino,
  ESP-IDF, ESPHome and XiaoZhi examples plus prebuilt firmware
- [Zephyr board port](https://docs.zephyrproject.org/latest/boards/waveshare/esp32s3_rlcd_4_2/doc/index.html)
- [ESPHome device entry](https://devices.esphome.io/devices/waveshare-esp32-s3-rlcd-42/)
- [CNX Software announcement, 2026-01-06](https://www.cnx-software.com/2026/01/06/esp32-s3-development-board-features-4-2-inch-reflective-lcd-rlcd-dual-microphone-array-onboard-speaker/)
