# Inkplate 4 TEMPERA buzzer hardware

This reference describes the V1.2.0 circuit and nominal component behaviour.
Calculated frequencies are not calibrated measurements of an assembled board.

## Circuit and controls

| Component | Function |
| --- | --- |
| U16, MCP4017T-103E/LT | 10 kOhm rheostat, I2C address `0x2F`, codes 0..127 |
| U18, TLC555CDR | Astable oscillator with nominal 50% duty cycle |
| R59 / C62 | 2.2 kOhm series resistor / 100 nF timing capacitor |
| U20, MLT-7525 | Transducer driven through a MOSFET push-pull stage |
| Q13 / `BUZZ_EN` | Switches `3V3_BUZZER` through active-low expander P1_4 |

The schematic uses an MCP4018 symbol for U16; the specified MCP4017 part exposes
resistor terminals B and W. Use the part's terminal behaviour when interpreting
rheostat code direction.

The switched rail powers the rheostat, timer and output stage together.
Rail-off removes their power; it is not an independent audio mute. The rheostat
must be powered for pitch programming. Its volatile wiper loads code `0x3F`
on power-on reset. Short off intervals may retain charge or timer state and
do not establish that a complete reset occurred.

The controls share GPIO21/22 I2C with sensors and other board functions.
The expander latches its outputs; SoC sleep does not itself clear an enabled
buzzer rail. The ULP cannot use these I2C pins.

## Nominal pitch network

For the nominal astable topology:

```text
R(code) = R59 + Rw + RAB * code / 127
f(code) = 0.7213 / (R(code) * C62)
```

With R59 = 2200 Ohm, RAB = 10000 Ohm, C62 = 100 nF and **assumed** wiper
resistance Rw = 100 Ohm, code 0 is approximately 3136 Hz and code 127 is
approximately 586 Hz. Increasing the code increases resistance and lowers
frequency. The frequency spacing between adjacent codes is non-uniform.

RAB tolerance alone is +/-20%; capacitor, resistor, timer and wiper variation
add uncertainty. Wiper resistance depends on voltage and code. These nominal
endpoints do not establish an exact musical range or a universal pitch-error
bound for assembled units.

## Physical capabilities and measurement limits

There is one square-wave oscillator. The circuit exposes pitch programming and
power switching, with no independent waveform, duty-cycle or amplitude control.
It cannot directly produce simultaneous independently controlled partials.
Square-wave harmonics and the transducer/enclosure response affect acoustic
level and timbre; fundamental frequency alone does not determine loudness.

Rail gating power-cycles the circuit rather than multiplying its output by an
ideal gain envelope. Startup, discharge and oscillator transitions may affect
the resulting sound. Actual pitch per code, power-on settling, short-off reset
behaviour and acoustic decay remain uncharacterized by controlled measurements.
Listening impressions do not establish those electrical or acoustic properties.

## Sources

- [Board hardware design](https://github.com/SolderedElectronics/Soldered-Inkplate-4-TEMPERA-with-glass-panel-hardware-design):
  `CAD/V1.2.0/Main board/SENSORS.kicad_sch`, sheet 7.
- [MCP4017/18/19 datasheet](https://ww1.microchip.com/downloads/aemDocuments/documents/MSLD/ProductDocuments/DataSheets/MCP4017-18-19-Data-Sheet-DS20002147.pdf),
  especially sections 4.1 and 6 for rheostat and power-on behaviour.
- Sibling baseline: `../Inkplate-Arduino-library/src/features/Buzzer/Buzzer.cpp`.

For device operation, use the [buzzer listening guide](../../../guides/audio/buzzer-listening.md).
