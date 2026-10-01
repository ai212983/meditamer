# Refresh probe development

The two probe targets exercise different firmware stacks against one generated
workload:

- `reference/` uses the installed Soldered Arduino library;
- `panel/` uses this repository's Rust `InkplateHal` driver;
- `common/spec.toml` owns cadence, run lengths, geometry, marker glyphs, and
  expected framebuffer hashes.

After changing the shared workload, regenerate and inspect both language
outputs:

```sh
python3 tools/rendering/refresh-probe/common/generate.py
python3 tools/rendering/refresh-probe/common/generate.py --check
```

Build both targets with their wrappers:

```sh
tools/rendering/refresh-probe/reference/build.sh partial-1bit
tools/rendering/refresh-probe/reference/build.sh profile-partial-1bit
tools/rendering/refresh-probe/reference/build.sh full-1bit
tools/rendering/refresh-probe/reference/build.sh full-3bit
tools/rendering/refresh-probe/reference/build.sh profile-full-1bit
tools/rendering/refresh-probe/reference/build.sh profile-full-3bit
tools/rendering/refresh-probe/reference/build.sh all
tools/rendering/refresh-probe/panel/build.sh partial-1bit
tools/rendering/refresh-probe/panel/build.sh profile-partial-1bit
tools/rendering/refresh-probe/panel/build.sh partial-spans
tools/rendering/refresh-probe/panel/build.sh partial-spans 20 250
tools/rendering/refresh-probe/panel/build.sh full-1bit
tools/rendering/refresh-probe/panel/build.sh full-3bit
tools/rendering/refresh-probe/panel/build.sh profile-full-1bit
tools/rendering/refresh-probe/panel/build.sh profile-full-3bit
tools/rendering/refresh-probe/panel/build.sh full-soak
tools/rendering/refresh-probe/panel/build.sh full-soak 500 0
tools/rendering/refresh-probe/panel/build.sh transition
tools/rendering/refresh-probe/panel/build.sh transition 100 250
tools/rendering/refresh-probe/panel/build.sh ghosting
tools/rendering/refresh-probe/panel/build.sh ghosting 20 0
```

`partial-1bit`, `full-1bit`, and `full-3bit` are comparable performance
benchmarks. Each uses 100 measured samples, one unmeasured mode-matched warm-up,
a 250 ms refresh-end-to-start interval, and fixture generation outside the
timing brackets. Partial holds panel power on; full refreshes power-cycle it per
sample. Validate and summarize completed captures with the shared tool:

```sh
python3 tools/rendering/refresh-probe/summarize.py partial-1bit REFERENCE_CAPTURE PANEL_CAPTURE
python3 tools/rendering/refresh-probe/summarize.py full-1bit REFERENCE_CAPTURE PANEL_CAPTURE
python3 tools/rendering/refresh-probe/summarize.py full-3bit REFERENCE_CAPTURE PANEL_CAPTURE
python3 tools/rendering/refresh-probe/summarize.py profile-partial-1bit REFERENCE_CAPTURE PANEL_CAPTURE
python3 tools/rendering/refresh-probe/summarize.py profile-full-1bit REFERENCE_CAPTURE PANEL_CAPTURE
python3 tools/rendering/refresh-probe/summarize.py profile-full-3bit REFERENCE_CAPTURE PANEL_CAPTURE
```

The summarizer rejects incomplete or non-comparable captures before reporting
sample count, mean, sample standard deviation, median, and range. It checks the
protocol metadata, sample sequence, mode-specific power state, cadence, visible
markers, and each numbered grayscale framebuffer hash.

The three `profile-*` targets use the same fixture and cadence contracts with 20
samples. They additionally validate and report matched phase means. Profile
totals are diagnostic because timestamp calls add overhead; never copy them
into the headline performance table.

The 3-bit fixture rebuilds the shared grayscale pattern and overlays the same
three-digit marker before every refresh. Both happen outside the transaction
timer. The summarizer independently reconstructs and checks the resulting hash
for every sample, so a stale image, wrong counter, or workload drift is rejected.
The packed framebuffer compensates for the panel's counterclockwise memory
presentation so the logical fixture is upright. This byte-order correction does
not alter scan counts or the previously recorded transaction timings; no device
rerun was performed for that visual-only correction.

`full-soak` is the distinct visual corruption test. It defaults to the shared
`full_soak_cycles` count and does not produce a performance-table result. Keep
phase-profile builds separate as well: their instrumentation is for attribution,
not headline timings.

`partial-spans` runs the shared bottom-of-panel scan-row workloads from
`common/spec.toml`. Its sample-count argument is per span. The visible marker
uses `1xx`, `2xx`, and `3xx` for the ascending span buckets, respectively, and
the firmware halts if the measured changed-row geometry differs from the
requested workload.

`transition` displays the pair count on both frames. It holds a vertical-bar
background constant and changes only the centered counter badge before each
partial update; the following full update redraws that exact target. Firmware
validates that the changed framebuffer bytes stay inside the badge bounds and
halts if the sparse-delta contract is violated. This keeps the final marker at
`500` during a 500-pair soak instead of wrapping the three-digit display after
999 individual updates.

Binary-full modes use the production contract: a fixed-zero hardware
source loop, the reference 74-iteration hardware clean loop, immediate
reference clean and row-start edges, the immediate reference row boundary, and
zero inter-pass delay. The reference pass counts and two-level `LUTB` / `LUT2`
translation remain unchanged. Only `profile-full-1bit` adds phase timestamps;
performance and soak modes remain uninstrumented. Partial and grayscale timing
remain independent.

Qualification boundary: CI checks only linked placement in
`scripts/ci/check_panel_waveform_placement.sh`. After changing the waveform
source, run `check_scan_contract.sh` against the
affected ELF and record the result with the change. Assembly shape is separate
from panel observation.

The rejected historical `peripheral-clean-packed-{5,10,12,14,20}mhz`
experiments moved only the 67 constant-data clean passes to I2S1 parallel DMA.
They configured the invariant
descriptor chain once, packed three identical rows into each repeated descriptor,
reused 96-sample DMA blocks to encode the lead and tail guards, preserved a 51.2
microsecond GPIO-routing guard as the sample rate changed, preserved the
reference row-control writes as distinct samples, and encoded the first row as
its unique eight-byte prefix plus the ordinary-row suffix. They retained
the fixed-zero software framebuffer/settle loop, reference clean edges,
reference row starts/boundaries, and production partial path selected by the
remaining arguments. The firmware checked the actual I2S divider before
installing the driver and recorded the expected divider in its ready line. A
build and passing ELF gate are host evidence only. Both the original packed
10 MHz stream and the corrected distinct-control-sample 10 MHz stream advanced
the counter without the black/white cleaning phases and left heavy ghosting.
The corrected 5 MHz lower-end trial visibly corrupted the screen and was
stopped; production firmware was then restored. These are two different
failure modes, not a passing/failing frequency bracket. The continuous
CL-as-data shape is rejected. Its implementation and build selectors have been
removed; the detailed timing record retains the evidence.

The historical `peripheral-clean-packed-self-check-10mhz` run was a
panel-power/OE-off diagnostic, not a refresh. It observed no exact one-hot lane
states, but the
result could not distinguish an empty sampling loop from zero or multi-bit pad
states, so it did not establish route validity. `peripheral-bck-benchmark-16mhz`
is the separate official-style throughput falsifier. With panel power and OE
off, it measures one warm-up plus 100 timed 600-row passes using a 152-byte
aligned row, the stock per-row DMA reset/send/wait path, CPU control writes, and
GPIO/BCK ownership switching. Its 9.563 ms mean exceeded the current 9.244 ms
software source pass and the 8.8 ms materiality threshold. The follow-up
`peripheral-bck-preconfigured-16mhz` bracket hoists only four invariant DMA
configuration writes while retaining every reset, wait, timeout, descriptor
check, GPIO control, and remux operation. It improved the mean to 9.094 ms, but
that is only 0.150 ms faster than software and still 0.294 ms above the
materiality threshold. The BCK source path is therefore closed without a
powered-panel trial. Neither diagnostic is a performance-table result or
physical waveform qualification.

The final non-peripheral source-loop falsifier tested whether the two `memw`
barriers per clean pulse were the remaining throughput limit. A dedicated
power-disabled/OE-off harness reproduced the active two-barrier source-pass
mean at 9.241 ms versus 9.244 ms in the powered phase profile. The proposed
one-barrier shape then measured the same 9.241 ms at both 16 fixed NOPs and the
zero-NOP lower endpoint. Neither endpoint met the 9.113 ms/pass materiality
threshold, so there is no performance passing endpoint or midpoint to bisect.
The bracket was rejected without applying the new pulse shape to the panel;
its temporary selector and harness have been removed.

The panel wrapper gives each device a compile-time bus rate that is applied
under the shared-bus mutex. Binary-full modes default to
400 kHz for the panel-owned PCAL6416A expander, TPS65186 PMIC, and MCP4018
digital potentiometers; each is specified for Fast-mode I2C. Partial-only and
grayscale-full modes retain their existing 100 kHz benchmark defaults. Touch, IMU, RTC,
environmental, and battery handles also remain explicitly configured at
100 kHz. `MEDITAMER_REFRESH_PROBE_I2C_KHZ=100` retains the prior full-mode rate
for comparison runs; `MEDITAMER_REFRESH_PROBE_I2C_KHZ=400` explicitly selects
the higher rate for partial or grayscale modes. The ready line records the
selected rate; artifact names add a suffix when it differs from the mode default.

`ghosting` conditions the panel with the requested number of alternating full
refreshes, performs one solid-white reveal, and then emits observation markers
at 0, 5, 15, 30, and 60 seconds without further screen updates. It emits a
`cycle_complete` record for every conditioning refresh and a separate reveal
completion record. Ghosting is a graded image-quality observation, not a
pass/fail corruption gate. Run it at the vendor-recommended minimum five-second
full-refresh interval and compare the same fixture and cadence with the
reference library before drawing conclusions. Until the reference probe exposes
that matched workload, panel-only ghosting observations are exploratory. Do not
use this mode's timings as the matched performance result. Keep zero-interval
`full-soak` for corruption, stall, and delayed-degradation qualification.

Production partial refresh uses the reference hold, reference row boundary,
hardware-loop scan, and bounded transition preparation. `bounded` prepares
only the transition suffix consumed by the reverse scan and retains the
original full-frame loop when all 600 rows are scanned.

The reference wrapper requires the normal unmodified InkplateLibrary 11.1.4
installation. It copies that library to a temporary directory, applies the
version-pinned probe instrumentation from `reference/patches/`, and compiles
against the temporary copy. It never patches the globally installed library,
so a fresh repository checkout is sufficient on any machine with the ordinary
Arduino dependency installed.

Flash and capture through `scripts/device/flash.sh` or `hostctl flash-capture`.
Ignored runtime artifacts belong under `logs/refresh-probe/`. Keep probe
timing compile-time so every tested hot loop is recoverable from its ELF.

Size stream captures for the complete workload, not just the timed refresh
call. Full-frame pattern generation is outside the driver timing brackets. In
particular, use at least a 120-second stream window for a 500-cycle, zero-interval
panel partial run; 90 seconds normally reaches only checkpoint 450. The
100-pair partial-to-full experiment needs a 210-second stream window; 165
seconds captured only 95 pairs even though the device continued. Use at least
750 seconds for 500 full-only cycles and 900 seconds for 500 partial-to-full
pairs. A 100-cycle ghosting run at the required five-second interval plus its
complete 60-second reveal hold needs at least a 750-second capture window.

After changing the waveform source, run the manual scan-contract check and
record its result with the change before any comparison run or soak. Counts
from a different compiled loop are not interchangeable.

Use the measured reference gap as one candidate-selection signal, not as the
objective or a stopping rule. Also inspect pass count, removable repeated work,
source and assembly shape, code simplification, applicability of existing
physical evidence, and the cost and risk of a new qualification bracket. A
candidate may legitimately make the Rust path faster than the reference; the
partial path's removal of three cleanup source passes is the model for a large
structural win that a gap-only comparison would miss.

Do not chase tiny timing reductions that add or preserve complexity. A selected
change should provide either a material performance benefit or a concrete code
quality benefit such as removing unsupported timing machinery, reducing paths,
or matching the documented reference sequence. Both benefits are welcome but
are not required.
