# Sound Development Notes

This document captures practical guidance for building better buzzer sounds on Inkplate 4 TEMPERA, with a focus on bell-like chimes.

## Hardware facts (Inkplate 4 TEMPERA)

- Buzzer pitch is controlled through MCP4018 digital potentiometer on I2C address `0x2F`.
- Buzzer enable is internal expander pin `BUZZ_EN` (active-low).
- Pitch control range in reference code is approximately `572..2933 Hz`.
- Pitch mapping used by the Arduino reference is `constrained = clamp(freq, 572, 2933)` and then `wiper_percent = 156.499576 + (-0.130347337 * constrained)`.
- Frontlight digipot is a separate device at `0x2E`.

## Why current sound can feel like a chirp

- A chirp is usually one short sweep or a few evenly spaced notes.
- Bells are not harmonic like simple tones. They use inharmonic partials, fast attack, and decaying tails.
- Bell perception improves when the early strike has bright partials and later notes decay longer and lower.

## Bell-like sound design rules for this buzzer

- Use 2 to 5 quick strike notes in the first 60 ms.
- Use inharmonic-ish spacing, not simple octave steps.
- Keep note lengths asymmetric (for example `12, 18, 26, 55, 120 ms`).
- Add a soft second tap around 120 to 220 ms later.
- Keep frequency values in the reliable range (`700..2600 Hz` is usually a good practical window).
- Leave tiny gaps between notes (`4..20 ms`) to avoid a flat continuous tone impression.

## Starter bell patterns

Try these as `(freq_hz, length_ms, pause_ms)` triplets.

### Bell v1 (small desk bell)

- `(1976, 16, 6)`
- `(1568, 18, 6)`
- `(1319, 24, 10)`
- `(1047, 42, 18)`
- `(880, 90, 80)`
- `(1760, 20, 10)`
- `(1319, 28, 0)`

### Bell v2 (softer notification bell)

- `(1760, 14, 5)`
- `(1480, 16, 6)`
- `(1175, 26, 10)`
- `(988, 54, 24)`
- `(784, 110, 120)`

## Tuning workflow

- First tune reliability, then timbre.
- Validate fixed beeps at `750`, `1200`, `1800`, and `2400 Hz`.
- If those are stable, shape a strike-tail pattern.
- If the sound is too sharp, lower first note or shorten first 2 notes.
- If the sound is too dull, raise first note and add one short high partial.
- If it feels too mechanical, vary durations and pauses more aggressively.

## Reliability and debugging notes

- If you see `i2c write 0x2F` failures, verify `BUZZ_EN` enable timing and I2C recovery behavior.
- If one NACK causes later failures on other addresses (`0x20`, `0x48`), reset bus and recreate device handle.
- Do not assume frontlight and buzzer share the same digipot or address.
- Keep diagnostics lightweight during startup to avoid poisoning bus state.

## Realistic patterns on this buzzer

Based on Inkplate's own examples, the MCP4018 control path, and Arduino tone-style usage, these patterns are realistic and reliable:

- Single beep and multi-beep alerts (`beep(80)`, `beep(100)` style).
- Two-tone alerts (for example `750 Hz` then `2400 Hz`).
- Short monophonic songs/arpeggios (single note at a time).
- Chirps and stepped sweeps (3 to 6 notes with short pauses).
- Gentle notification motifs (soft ascending then descending notes in mid frequencies).

Patterns that are not realistic on this path:

- True bell timbre with rich simultaneous partials.
- Polyphonic chords (simultaneous independent notes).
- Rich instrument envelopes/tone-color control.

Practical constraints for this project:

- Use frequencies in roughly `572..2933 Hz` (Inkplate reference range).
- Mid-band notes (`700..1400 Hz`) generally sound less harsh than high notes.
- Keep pauses between notes (`20..200 ms`) for clearer rhythm and less "chirp" feel.
- Use short sequences (3 to 8 notes) to reduce cumulative I2C pitch-write stress.

## Preset: Gentle notification

Recommended default "gentle" motif for this codebase:

- `(784, 70, 35)`
- `(988, 85, 45)`
- `(1175, 110, 180)`
- `(988, 75, 35)`
- `(784, 120, 0)`

This shape gives a softer rise-and-fall and avoids very high pitches.

## External resources

- Inkplate 4 TEMPERA product page: [https://soldered.com/product/inkplate-4-tempera/](https://soldered.com/product/inkplate-4-tempera/)
- Inkplate Arduino library repository: [https://github.com/SolderedElectronics/Inkplate-Arduino-library](https://github.com/SolderedElectronics/Inkplate-Arduino-library)
- Inkplate 4 TEMPERA buzzer example (official): [https://github.com/SolderedElectronics/Inkplate-Arduino-library/blob/master/examples/Inkplate4TEMPERA/Advanced/Sensors/Inkplate4TEMPERA_Buzzer/Inkplate4TEMPERA_Buzzer.ino](https://github.com/SolderedElectronics/Inkplate-Arduino-library/blob/master/examples/Inkplate4TEMPERA/Advanced/Sensors/Inkplate4TEMPERA_Buzzer/Inkplate4TEMPERA_Buzzer.ino)
- MCP4018 product page and datasheet entry: [https://www.microchip.com/en-us/product/MCP4018](https://www.microchip.com/en-us/product/MCP4018)
- MCP4017/18/19 datasheet PDF: [https://ww1.microchip.com/downloads/aemDocuments/documents/MSLD/ProductDocuments/DataSheets/MCP4017-18-19-Data-Sheet-DS20002147.pdf](https://ww1.microchip.com/downloads/aemDocuments/documents/MSLD/ProductDocuments/DataSheets/MCP4017-18-19-Data-Sheet-DS20002147.pdf)
- Arduino tone melody reference (useful note table and timing structure): [https://www.arduino.cc/en/Tutorial/BuiltInExamples/toneMelody/](https://www.arduino.cc/en/Tutorial/BuiltInExamples/toneMelody/)
- Arduino tone multiple outputs reference (documents one-note-at-a-time timer behavior): [https://www.arduino.cc/en/Tutorial/BuiltInExamples/toneMultiple/](https://www.arduino.cc/en/Tutorial/BuiltInExamples/toneMultiple/)
- Risset-style additive bell example (classic bell partial approach): [https://msp.ucsd.edu/techniques/latest/book-html/node71.html](https://msp.ucsd.edu/techniques/latest/book-html/node71.html)
- Bell acoustics paper index (harmonic and inharmonic bell design context): [https://pubmed.ncbi.nlm.nih.gov/12880061/](https://pubmed.ncbi.nlm.nih.gov/12880061/)

## Local reference files in this workspace

- `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/src/lib.rs`
- `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/src/main.rs`
- `/Users/dimitri/Documents/Code/personal/Inkplate/Inkplate-Arduino-library/src/include/Buzzer.cpp`
- `/Users/dimitri/Documents/Code/personal/Inkplate/Inkplate-Arduino-library/src/include/Buzzer.h`
- `/Users/dimitri/Documents/Code/personal/Inkplate/Inkplate-Arduino-library/src/libs/MCP4018/src/MCP4018-SOLDERED.cpp`
- `/Users/dimitri/Documents/Code/personal/Inkplate/Inkplate-Arduino-library/src/boards/Inkplate4TEMPERA.h`
- `/Users/dimitri/Documents/Code/personal/Inkplate/Inkplate-Arduino-library/examples/Inkplate4TEMPERA/Advanced/Sensors/Inkplate4TEMPERA_Buzzer/Inkplate4TEMPERA_Buzzer.ino`
