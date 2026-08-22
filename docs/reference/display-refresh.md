# Display Refresh: Formats, Modes, and the Panel-Power Lease

As of: 2026-08-18

## "Eight-bit" has three different meanings in this stack

- the panel electrical source bus is eight GPIO lines wide;
- LVGL renders one eight-bit luminance value (`L8`) per pixel;
- the panel exposes eight physical gray levels, which is three-bit grayscale,
  not 256-level grayscale.

Keep these distinct when reading code or logs that say "8-bit".

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

## Recommended product architecture

- Use full native grayscale for stable full-screen image reconstruction.
- Use the validated binary partial waveform for LVGL interaction and animation.
- For a progress indicator over a grayscale image, reserve a known white or
  black binary tile during the full image render. Maintain a local
  binary baseline for that tile and emit skip commands everywhere outside it.
- Keep full-refresh policy semantic: startup, complete screen/activity changes,
  recovery after failure, and explicit image-quality maintenance. A small
  repeatedly updated status tile must not force a global full refresh by count.

Native eight-level (grayscale) partial refresh is not implemented and remains a
waveform-research project rather than a framebuffer-format switch — see the
closed feasibility study in
[`archive/research/native-grayscale-partial-refresh.md`](../archive/research/native-grayscale-partial-refresh.md)
before reopening that direction.

## Validated binary partial-refresh behavior

Device experiments (2026-07-31) established these runtime facts for the
production binary partial path:

- a 12-cycle soak completed 12 successful partial refreshes;
- standard and shortened vertical scans completed at left, center, and right
  positions, but the raw panel still requires complete source rows — there is
  no addressable controller-side X/Y window, since the panel is a raw
  source/gate scan chain;
- omitting cleanup phases reduced measured waveform time, but the serial
  timing results alone do not establish acceptable physical image quality.

The experiment commands and their device wrapper scripts were removed after
these findings were recorded. Any future waveform change needs a new bounded
experiment plus physical validation (photographs, not just serial timing).

## Binary panel-power lease

LVGL and its successful-partial panel-power lease are production behavior —
this is the "3-second panel-power lease" referenced in
[`compile-time-features.md`](compile-time-features.md). Build the firmware
normally:

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
