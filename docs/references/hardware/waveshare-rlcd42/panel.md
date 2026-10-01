# ST7305 Reflective-LCD Panel Notes

The Waveshare ESP32-S3-RLCD-4.2 carries a 4.2" **fully reflective** monochrome LCD: 300x400 native,
1 bpp, no backlight, ambient light only. It is not e-paper — the image is held by a self-refreshing
scan, not by bistable ink — but it reads like e-paper in sunlight and holds an image at fractions of
a milliwatt.

Driver: [boards/waveshare-rlcd42/src/panel.rs](../../../../boards/waveshare-rlcd42/src/panel.rs),
translated from Waveshare's ESP-IDF U8g2 component
(`02_Example/ESP-IDF/11_U8G2_Test/components/u8g2_st7305`), which is the authoritative source for
the init sequence and framebuffer packing. Datasheets are mirrored in
[datasheets/](datasheets/README.md) — read the ST7306 one for anything geometric, and
[controller-identity.md](controller-identity.md) for why.

## Geometry and orientation

Native is **portrait 300x400**. The board presents **landscape 400x300** to callers, because
Waveshare's own stack draws in 400x300 and applies U8g2's `R1` rotation, and its LVGL port is
initialised at 400x300 too. Confirmed on the device, 2026-08-28: 400x300, rotated to landscape.
`set_pixel` rotates 90° clockwise into the native frame — logical top-left lands at native
top-right — and callers never see native coordinates.

The framebuffer is in **page format**: one byte holds eight vertically adjacent native pixels, bit
`y % 8`. That is what the packing consumes directly. Total 300 x 50 = 15,000 bytes.

Polarity: the framebuffer's contract is *bit set = ink*, matching the Inkplate. This panel renders a
set bit as paper, so the inversion happens once, inside the packing loop, rather than by making
every caller reason backwards.

## Addressing — none of it is guessable

- Columns are addressed in **groups of twelve pixels**, starting at address `0x12`, and the window
  counts **downward**: the `0x2A` command takes `0x3C - addr_end` first.
- Each transmitted byte covers a **4-pixel-wide by 2-pixel-tall block**, assembled through the
  driver's `PACK` table from four source columns at one of four sub-row shifts. A full tile row is
  4 x 75 bytes.
- One tile row is eight pixels tall and spans **four** row addresses (`0x2B` takes
  `page * 4 ..= page * 4 + 3`).

Partial refresh narrows both windows, not just the rows: the dirty box is rounded outward to a
twelve-pixel column-group boundary and everything inside is sent. The bounding box is tracked in
*native* coordinates, since that is what the window commands address, so the rotation is applied
when recording it exactly as `set_pixel` applies it when writing.

## Init sequence

`St7305::init` reproduces Waveshare's values verbatim. **Neither datasheet documents most of these
registers**, so they are reproduced rather than derived and should not be "tidied":

| Command | Payload | Meaning |
| --- | --- | --- |
| `0xD6` | `17 02` | NVM load control |
| `0xD1` | `01` | Booster enable |
| `0xC0` | `11 04` | Gate voltage |
| `0xC1` / `0xC2` | `69 x4` / `19 x4` | VSHP / VSLP |
| `0xC4` / `0xC5` | `4B x4` / `19 x4` | VSHN / VSLN |
| `0xD8` | `80 E9` | OSCSET — parameter 1 `0x80` is OSCSW=000 |
| `0xB2` | `02` | FRCTRL, frame rates |
| `0xB3` / `0xB4` | 10 / 8 bytes | Update period gate/source, high- then low-power |
| `0x62` | `32 03 1F` | Gate timing |
| `0xB7` | `13` | Source EQ |
| `0xB0` | `64` | Duty — 400 lines |
| `0x11` | — | Sleep out, then 120 ms |
| `0xC9` | `00` | Source voltage select |
| `0x36` | `48` | Memory access: MX + BGR |
| `0x3A` | `11` | 1 bpp mode |
| `0xB9` / `0xB8` | `20` / `29` | Source / panel setting |
| `0x21` | — | Display inversion on |
| `0x2A` / `0x2B` | `12 2A` / `00 C7` | Full column / row window |
| `0x35` | `00` | Tearing effect on |
| `0xD0` | `FF` | Auto power-down off |
| `0x38` | — | High-power mode |
| `0x29` | — | Display on |

Hardware reset before all of it: RST high 50 ms, low 20 ms, high 50 ms. Waveshare's factory driver
enables a pull-up on RST; the target applies the same active-mode fail-safe to both idle-high panel
controls (`CS`, `RST`). Those software pulls are **not** retained when the digital pad domain powers
down — which is why deep sleep blanks the panel.

SPI runs at 24 MHz, mode 0, on SPI2.

## FRCTRL is self-refresh, not write latency

The rate FRCTRL (`0xB2`) sets is the panel's *self-refresh* rate — how often it re-scans to maintain
the image it already holds, closer to a DRAM refresh than to a monitor's frame rate. **Writes are
not gated by it.** A one-second clock keeps ticking one second at a time while TE pulses every four,
because a write refreshes the region it touches immediately.

Bit 4 selects the high-power rate, bits 2:0 the low-power one; the high-power meaning depends on
OSCSET parameter 1, which the init leaves at `0x80` (OSCSW=000):

| Setting | Datasheet | Measured on TE |
| --- | --- | --- |
| HPM, HFRA=0 | 25.5 Hz | 26 Hz |
| HPM, HFRA=1 | 51 Hz | 52 Hz |
| LPM, LFRA=2 | 1 Hz | 1 Hz |
| LPM, LFRA=5 | 8 Hz | 8 Hz |
| LPM, LFRA=0 | 0.25 Hz | one pulse per 4 s |

The board therefore runs both extremes: `HighPowerRate::Full` (51 Hz) because that is what anything
animated wants and it costs nothing while idle, and `LowPowerRate::Hz0_25` because the refresh
circuitry then runs four times less often than at the vendor default with no downside.

## Power modes

u8g2's source annotates `0x38` as *low* power in one place and *high* in another. The board measured
it on the TE pin rather than trusting the annotation:

| Mode | TE (panel frame rate) | Write path |
| --- | --- | --- |
| `0x38` High | **26 Hz** | 870 fps |
| `0x39` Low | **1 Hz** | 870 fps |

A 26x difference, so `0x38` is high power and the dissenting comment is wrong. The second column is
the more useful finding: **the write path is unaffected by the mode.**

Product consequence, from the Hourglass work: keep the panel in its **low-power drive** while
animated, paused, complete, and outside the app. Identified-panel validation showed *reduced
contrast* in high-power mode, while regional writes stay immediate in either. Runtime CPU/performance
intent must not change the LCD drive waveform.

There is no current meter on this board — the `0x40` that looks like an INA219 is the ES7210
microphone ADC. Battery *voltage* on ADC1 channel 3 cannot compare two builds' idle draw in any
practical time, so power-mode claims rest on TE, not on measured milliamps.

## Measured write performance

- Full frame: 15,000 bytes.
- A 24x24 block through `set_pixel`: **144 bytes, 870 fps**, 1.15 ms per frame of which 0.048 ms is
  SPI — 25x faster on 104x less data than the same sweep through `framebuffer_mut`.
- Partial refresh of a typical dirty box: ~2,016 bytes against 15,000.

`framebuffer_mut` cannot know what the caller touched, so it conservatively marks the whole panel
dirty and the next refresh is indistinguishable from a full one. Prefer `set_pixel` — or LVGL's
`blit_l8`, which records the real bounding box — for anything smaller than the screen.

At 870 fps the write path is ~33x faster than the glass it drives. The bottleneck is the panel, so
further optimisation of the write path buys power, not visible speed.

## `board::Panel` and refresh

`St7305` implements `board::Panel` for geometry and `blit_l8` (threshold: below mid-grey is ink).
`refresh`/`supports` are **concrete methods, not trait methods** — the Inkplate's refresh is
asynchronous I²C-sequenced waveform work and this one is a synchronous SPI push, and forcing both
through one synchronous trait method was never honest. `RefreshMode`/`RefreshError` stay shared. Both
`Full` and `Partial` are supported.

## Which controller is this?

Almost certainly an **ST7306**, not the ST7305 the vendor driver names: every geometry limit in the
ST7305 datasheet is exceeded by the vendor's own init, and every one fits the ST7306's. The full
argument, the failed ID-register probe, and what it means for greyscale are in
[controller-identity.md](controller-identity.md).

Practical consequence for this file: **use the ST7306 datasheet for geometry, address ranges, and
RAM.** The ST7305 one is still good for the serial protocol and the FRCTRL/OSCSET decoding above,
which measured correctly against it. The `St7305` type keeps its name for now because that is what
the vendor component it was translated from calls the part.

## Sources

- [ST7305 datasheet V0.2](https://files.waveshare.com/wiki/common/ST_7305_V0_2.pdf) (Waveshare mirror)
- Waveshare's `u8g2_st7305` ESP-IDF component in
  [waveshareteam/ESP32-S3-RLCD-4.2](https://github.com/waveshareteam/ESP32-S3-RLCD-4.2)
- [historical ADR-0015](../../../archive/architecture/0015-two-product-platform-workspace.md) — the panel bring-up,
  measurement, and board-extraction record this file summarises
