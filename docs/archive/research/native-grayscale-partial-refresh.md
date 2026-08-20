# Native Eight-Level Partial Refresh: Feasibility Findings

Closed feasibility study for combining LVGL partial rendering with the
Inkplate 4 TEMPERA's native grayscale waveform. Extracted from `docs/notes/`
on 2026-08-18; the accepted production architecture that resulted from this
study is recorded in
[`reference/display-refresh.md`](../../reference/display-refresh.md).

## Scope

The reference-library baseline is [Inkplate Arduino library commit
`839da188`](https://github.com/SolderedElectronics/Inkplate-Arduino-library/tree/839da1884d2087e74afff9d23bda038b4571fab0).

## Reference-driver findings

The reference driver explicitly limits `partialUpdate()` to black-and-white
mode and returns without updating in grayscale mode:

- [`Inkplate4TEMPERADriver.cpp`, `partialUpdate`, lines
  400-419](https://github.com/SolderedElectronics/Inkplate-Arduino-library/blob/839da1884d2087e74afff9d23bda038b4571fab0/src/boards/Inkplate4TEMPERA/Inkplate4TEMPERADriver.cpp#L400-L419)
- [`Inkplate4TEMPERADriver.h`, public API, lines
  43-52](https://github.com/SolderedElectronics/Inkplate-Arduino-library/blob/839da1884d2087e74afff9d23bda038b4571fab0/src/boards/Inkplate4TEMPERA/Inkplate4TEMPERADriver.h#L43-L52)

Reference binary partial refresh:

1. Diffs the previous and current one-bit framebuffers.
2. Encodes a two-bit drive command for every changed pixel and a skip command
   for every unchanged pixel.
3. Shifts all 600 source pixels and advances all 600 gates for nine passes.
4. Runs two discharge cleanup passes and one skip cleanup pass.

There is no addressable controller-side X/Y window. The panel is a raw
source/gate scan chain. Every active gate row must receive a complete source
row, so an X rectangle cannot simply omit horizontal clocks.

Reference native grayscale refresh:

1. Runs 65 full conditioning frames: 5 white, 15 black, 15 white, 15 black,
   and 15 white.
2. Runs eight full target-gray waveform phases.
3. Runs one final skip frame and parks the gate scan.

See [`display3b`, lines
248-295](https://github.com/SolderedElectronics/Inkplate-Arduino-library/blob/839da1884d2087e74afff9d23bda038b4571fab0/src/boards/Inkplate4TEMPERA/Inkplate4TEMPERADriver.cpp#L248-L295).
Its waveform describes an absolute target level after global conditioning; it
does not describe an old-gray-to-new-gray transition.

The reference image pipeline may accept eight-bit luminance, but it quantizes
or dithers that input to the eight native levels:

- [`ImageDither.cpp`, 8-bit input to 3-bit output, lines
  19-57](https://github.com/SolderedElectronics/Inkplate-Arduino-library/blob/839da1884d2087e74afff9d23bda038b4571fab0/src/graphics/Image/ImageDither.cpp#L19-L57)
- [`waveforms.h`, eight target levels by eight phases, lines
  1-8](https://github.com/SolderedElectronics/Inkplate-Arduino-library/blob/839da1884d2087e74afff9d23bda038b4571fab0/src/boards/Inkplate4TEMPERA/waveforms.h#L1-L8)

## Feasibility of native partial grayscale

Native eight-level partial refresh is a waveform-research project rather than
a framebuffer-format switch. A robust transition waveform needs to account for
all 64 old-level/new-level pairs.

Two possible research directions are:

1. Characterize an empirical 8 by 8 transition waveform matrix. This needs
   direction-specific pulse sequences, repeated-cycle ghosting measurements,
   boundary checks, VCOM coverage, and temperature coverage.
2. Condition only a selected region to known white, then apply the existing
   eight target phases there while emitting skip commands elsewhere. This is
   easier to reason about but may still need most of the 65 conditioning passes,
   so it is unlikely to be a latency improvement.

Potential persistent PSRAM costs are:

- Gray4 target plus one-bit changed mask: 180,000 + 45,000 bytes;
- exact current and previous Gray4 frames: 360,000 bytes;
- eight precomputed two-bit waveform planes: another 720,000 bytes.

Memory capacity alone does not establish waveform correctness.

## Validation required before native grayscale experiments advance

- all 64 gray transition pairs in both isolated and adjacent tiles;
- left, center, and right gate positions;
- repeated toggles and mixed transition directions;
- unchanged-region preservation and boundary halos;
- cold and warm panel behavior, VCOM range, and temperature range;
- failure recovery followed by an explicit full refresh;
- serial timing plus physical photographs. Logs alone are not visual evidence.

Native partial grayscale must remain experimental until those gates pass.
