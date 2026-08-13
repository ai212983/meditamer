# LVGL eight-level partial refresh findings

## Scope

This note records the feasibility findings for combining LVGL partial rendering
with the Inkplate 4 TEMPERA's native grayscale waveform. The reference-library
baseline is [Inkplate Arduino library commit
`839da188`](https://github.com/SolderedElectronics/Inkplate-Arduino-library/tree/839da1884d2087e74afff9d23bda038b4571fab0).

"Eight-bit" has three different meanings in this stack:

- the panel electrical source bus is eight GPIO lines wide;
- LVGL renders one eight-bit luminance value (`L8`) per pixel;
- the panel exposes eight physical gray levels, which is three-bit grayscale,
  not 256-level grayscale.

## Current LVGL and panel formats

LVGL 9.5.4 renders dirty regions into a 600 by 16 line `L8` buffer. The flush
callback converts those luminance pixels into the persistent one-bit Inkplate
framebuffer using component-aware deterministic dithering:

- [`src/firmware/ui/lvgl/mod.rs`](../../src/firmware/ui/lvgl/mod.rs)
- [`src/firmware/ui/lvgl/dither.rs`](../../src/firmware/ui/lvgl/dither.rs)

The production interaction path is therefore:

`partial L8 render -> dirty L8-to-I1 conversion -> binary partial waveform`.

The Rust driver also supports full native grayscale. It accepts a packed
four-bit-per-pixel framebuffer, rounds each nibble to one of eight physical
levels, and runs the reference eight-phase waveform:

- [`src/platform/inkplate/mod.rs`](../../src/platform/inkplate/mod.rs)
- [`src/platform/inkplate/display/async_impl.rs`](../../src/platform/inkplate/display/async_impl.rs)

A 600 by 600 packed Gray4 framebuffer occupies 180,000 bytes. A grayscale
refresh invalidates the binary partial baseline because the physical screen no
longer has one known binary value per pixel.

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

## Recommended product architecture

- Use full native grayscale for stable full-screen image reconstruction.
- Use the validated binary partial waveform for LVGL interaction and animation.
- For a progress indicator over a grayscale image, reserve a known white or
  black binary tile during the full image render. Maintain a local
  binary baseline for that tile and emit skip commands everywhere outside it.
- Keep full-refresh policy semantic: startup, complete screen/activity changes,
  recovery after failure, and explicit image-quality maintenance. A small
  repeatedly updated status tile must not force a global full refresh by count.

## Validation required before native grayscale experiments advance

- all 64 gray transition pairs in both isolated and adjacent tiles;
- left, center, and right gate positions;
- repeated toggles and mixed transition directions;
- unchanged-region preservation and boundary halos;
- cold and warm panel behavior, VCOM range, and temperature range;
- failure recovery followed by an explicit full refresh;
- serial timing plus physical photographs. Logs alone are not visual evidence.

Native partial grayscale must remain experimental until those gates pass.

## Completed binary partial-refresh experiments

The July 31 device experiments established these runtime facts:

- the production partial path completed a 12-cycle soak with 12 successful
  partial refreshes;
- standard and shortened vertical scans completed at left, center, and right
  positions, but the raw panel still requires complete source rows;
- omitting cleanup phases reduced measured waveform time, but the serial timing
  results do not establish acceptable physical image quality;
- the experiment commands and their device wrapper scripts were removed after
  these findings were recorded. Any future waveform change needs a new bounded
  experiment and the physical validation listed above.

## Binary panel-power lease

LVGL and its successful-partial panel-power lease are production behavior. Build
the firmware normally:

```sh
scripts/build/build.sh release
```

Every successful binary partial refresh in the production LVGL path parks the
powered panel and renews one 3,000 ms idle deadline from refresh completion.
This policy is independent of buttons and touch coordinates, so it also covers
LVGL timers, progress indicators, and other non-touch partial updates.

A no-change flush does not renew the deadline. Full refreshes, partial-to-full
fallbacks, and failed transactions shut the panel down instead of leasing it.
Firmware logs each renewal, timeout shutdown, and shutdown duration.
