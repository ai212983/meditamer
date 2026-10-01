# Waveshare ESP32-S3-RLCD-4.2 Audio Notes

**Nothing in this repository drives this hardware today.** This file records what is on the board and
what bring-up would involve, so the next person starting does not have to re-derive it from vendor
YAML.

The contrast with the Inkplate matters for anything shared between the two boards. The Inkplate has a
*buzzer*: one pitch at a time, through an MCP4018 digipot, monophonic by construction — see
[the Inkplate sound notes](../inkplate/sound.md). This board has a *codec and an amplifier*: real
PCM, stereo-capable framing, arbitrary waveforms. Any sound abstraction that grows to cover both
must not assume the Inkplate's one-note-at-a-time model is the general case.

## What is on the board

| Part | I²C address | Role |
| --- | --- | --- |
| ES8311 | `0x18` | Low-power mono audio codec — the speaker DAC path |
| ES7210 | `0x40` | Multi-channel ADC for the microphone array, with echo cancellation |

Plus, not on I²C:

- **Dual microphone array**, feeding the ES7210. Waveshare's own framing for this board is AI voice
  interaction (their XiaoZhi example), which is what the echo-cancelling ADC is there for.
- **Speaker output** on an MX1.25 2-pin header — an *external* speaker connector, so a bare board
  makes no sound until one is fitted.
- **Amplifier enable on GPIO46, active-high.** The DAC output only reaches the speaker while this is
  driven high. A silent board with correct codec registers is most likely this pin.

Both codecs share the same I²C0 bus as the RTC and the SHTC3 (GPIO13 `SDA` / GPIO14 `SCL`), so any
audio driver has to take the existing bus mutex rather than claim the peripheral.

## I²S wiring

| Signal | GPIO |
| --- | ---: |
| `MCLK` | 16 |
| `BCLK` | 9 |
| `LRCLK` / `WS` | 45 |
| Data (speaker side) | 8 |
| Data (microphone side) | 10 |

The schematic's net names settle a direction conflict between the online configs, and they are named
from the *codec's* side, which is the easy way to get this backwards: `I2S_DSDIN` is DAC-serial-data-
**in**, so GPIO8 carries audio the ESP32 transmits to the speaker; `I2S_ASDOUT` is ADC-serial-data-
**out**, so GPIO10 carries audio the ESP32 receives from the microphones. ESPHome has this right;
Zephyr's pinctrl reads backwards.

Both directions share one I²S peripheral's clocks, which is the usual shape for a codec pair like
this — the ES8311 and ES7210 are clocked together and configured independently over I²C.

## If this is ever built

Sketch only; none of it has run on this board from this repo.

1. Get the ES8311 to ACK at `0x18` and the ES7210 at `0x40` on the existing shared bus before
   touching I²S at all. A scan that finds four devices (`0x18`, `0x40`, `0x51`, `0x70`) means the bus
   is healthy — see [sensors.md](sensors.md).
2. Bring up playback first, and separately from capture. Playback needs the codec configured, the
   I²S clocks running, **and** GPIO46 high; capture needs none of that, so debugging them together
   confounds three independent failure modes.
3. Take the register sequences from Waveshare's own ESP-IDF example for this board rather than from a
   generic ES8311 driver. As with the ST7305 panel, the vendor sequence is the authority for the
   part *as wired here*, and the same rule applies: reproduce it, do not tidy it.
4. Budget the memory before writing the driver. Audio buffers are large and this board's DRAM is not
   the Inkplate's — anything statically allocated belongs in the same accounting discipline as
   [the DRAM budget](../../memory/medinote-waveshare/budget.md).
5. Expect the amplifier to be a power decision, not just a mute: leaving GPIO46 high idles the amp.
   Sleep entry ([sleep.rs](../../../../targets/medinote-waveshare/src/sleep.rs)) would need to drop
   it, and the pad-hold caveat from [board.md](board.md#sleep) applies.

## Sources

- [ES8311 datasheet](https://files.waveshare.com/wiki/common/ES8311.DS.pdf) (Waveshare mirror)
- [Waveshare wiki](https://docs.waveshare.com/ESP32-S3-RLCD-4.2) — onboard resource list, speaker
  and microphone description
- [Waveshare ESPHome RLCD voice example](https://docs.waveshare.com/ESP32-ESPHome-Tutorials/Example-RLCD-Voice)
  — the source of the I²S pin table above
- [waveshareteam/ESP32-S3-RLCD-4.2](https://github.com/waveshareteam/ESP32-S3-RLCD-4.2) — the XiaoZhi
  and ESP-IDF examples that actually exercise the codec pair
- [ESPHome device entry](https://devices.esphome.io/devices/waveshare-esp32-s3-rlcd-42/)
