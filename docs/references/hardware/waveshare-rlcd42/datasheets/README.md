# Waveshare ESP32-S3-RLCD-4.2 datasheets

Two documents are mirrored locally, on the same principle as the Inkplate folder's
[tps65185x](../../inkplate/datasheets/) — pin the ones whose contents get argued about, link the
rest rather than checking in 40 MB of PDFs by default.

| Local file | Size | What it settles |
| --- | ---: | --- |
| [st7306-datasheet.pdf](st7306-datasheet.pdf) | 4.5 MB | **The one that matches this hardware.** 720x480 max, and its CASET/RASET/GATESET ranges are the ones the vendor init actually stays inside — see [controller-identity.md](../controller-identity.md) |
| [st7305-datasheet.pdf](st7305-datasheet.pdf) | 4.5 MB | The part Waveshare *names*, and wrong for this panel: it documents 264x320 and the board's init exceeds its every geometry limit. Still good for the serial protocol, MADCTL, and FRCTRL/OSCSET, which the two parts share. Do not take geometry, address ranges, or RAM size from it |
| [esp32-s3-rlcd-4.2-schematic.pdf](esp32-s3-rlcd-4.2-schematic.pdf) | 1.0 MB | The board's own pin table — the authority for [board.md](../board.md#pin-map), and the reason the I²S direction and I²C ordering conflicts between Zephyr and ESPHome are settled rather than open |

Everything else stays upstream.

| Part | Role | Source |
| --- | --- | --- |
| ST7306 | Reflective-LCD controller — the match | mirrored above · [upstream](https://v4.cecdn.yun300.cn/100001_1909185148/ST_7306_V0_1.pdf) (GooDisplay) |
| ST7305 | The part the vendor names; wrong for this panel | mirrored above · [upstream](https://files.waveshare.com/wiki/common/ST_7305_V0_2.pdf) |
| ESP32-S3 | SoC datasheet | [Espressif](https://documentation.espressif.com/esp32-s3_datasheet_en.pdf) |
| ESP32-S3 | Technical reference manual | [Espressif](https://documentation.espressif.com/esp32-s3_technical_reference_manual_en.pdf) |
| PCF85063A | Real-time clock | [NXP](https://www.nxp.com/docs/en/data-sheet/PCF85063A.pdf) · [Waveshare mirror](https://files.waveshare.com/wiki/common/Pcf85063atl1118-NdPQpTGE-loeW7GbZ7.pdf) |
| SHTC3 | Temperature and humidity | [Waveshare mirror](https://files.waveshare.com/wiki/common/SHTC3_Datasheet.pdf) |
| ES8311 | Audio codec | [Waveshare mirror](https://files.waveshare.com/wiki/common/ES8311.DS.pdf) |
| ES7210 | Microphone-array ADC | Not published on the vendor resources page; see the [Waveshare wiki](https://docs.waveshare.com/ESP32-S3-RLCD-4.2) |

Board-level design files:

| Document | Source |
| --- | --- |
| Schematic | mirrored above · [upstream](https://files.waveshare.com/wiki/ESP32-S3-RLCD-4.2/ESP32-S3-RLCD-4.2-schematic.pdf) |
| 3D structure and dimensions | [ESP32-S3-RLCD-4.2-3dFile.rar](https://files.waveshare.com/wiki/ESP32-S3-RLCD-4.2/ESP32-S3-RLCD-4.2-3dFile.rar) |
| Example code and prebuilt firmware | [waveshareteam/ESP32-S3-RLCD-4.2](https://github.com/waveshareteam/ESP32-S3-RLCD-4.2) |

The full index is Waveshare's
[resources page](https://docs.waveshare.com/ESP32-S3-RLCD-4.2/Resources-And-Documents).
