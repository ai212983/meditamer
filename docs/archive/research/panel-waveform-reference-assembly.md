# Panel Waveform Timing Plan

- Status: Done
- Last-reviewed: 2026-08-31

## Goal

Replace incidental Inkplate timing with a small, measured deterministic
contract that:

1. keeps partial refresh reliable; and
2. executes the official binary full-refresh workload as fast as signal
   integrity and image quality permit; and
3. does the same for native 3-bit grayscale with independent timing.

The official implementation is the comparison baseline, not an electrical
specification for the ED038TH2 panel.

The investigation has two axes: waveform quantity (pass count, pulse count,
and ordering) and execution rate (pulse width, row/pass overhead, and compiled
loop throughput). Same-workload timing experiments freeze the first axis and
optimize the second. Reducing waveform quantity is a separate investigation.
Every rate constraint must come from captured behavior or panel evidence, with
an explicit tolerance; a convenient CPU-cycle count is not a justification.

## Current baseline

- The official 240 MHz build uses ordered GPIO writes and memory barriers with no
  explicit hold. Binary/partial are in flash; Gray4 is in IRAM. ELF verifies placement.
- Rust runs timing-critical scans from internal RAM. Binary full now uses the
  official ordered GPIO and CKV/LE instruction sequence; partial retains its
  independent 48-cycle CCOUNT hold.
- The retained 12/48 CCOUNT waits are execution-rate baselines, not waveform
  counts or targets.
- Binary full, partial, and Gray4 now have independent timing paths. ELF gates
  guard their `.rwtext` placement and compiled timing implementation.
- Physical testing confirmed clean regular Ambient Home full refreshes at 12
  cycles. Captured duration is about 1.43-1.45 seconds, down from 2.57-2.59
  seconds at 48 cycles.
- Binary full refresh still matches the official 78-pass waveform: 67 cleaning
  passes, 10 image passes, and 1 settle pass. That is 7,066,800 CL pulses and
  46,800 row endings per refresh.
- Exact fixed-pattern comparators measured official binary full at an 866.322 ms
  mean, Rust 12-cycle full at 1,224.297 ms, and the qualified ordered-write plus
  CKV/LE stage at 836.194 ms. Applying the qualified fixed-size unchecked-read
  traversal reduced the final candidate to an 812.041 ms mean: 6.27 percent
  faster than official with a 26 us three-run spread. Physical inspection found
  the fixed pattern clean.
- The promoted production ELF reached `RUNTIME_READY`; startup and Ambient Home
  full refreshes were 1.033 and 1.036 seconds, and the unchanged partial path
  subsequently completed in 430 ms.
- Native 3-bit grayscale matches the official 74 passes: 65 cleaning, 8 Gray4,
  and 1 final skip; 6,704,400 CL pulses and 44,400 row endings. Rust 48 measures
  2.337041 seconds waveform; all-Gray4 12 measures 1.258120 seconds, a throughput
  data point rather than a proposed magic hold count.
- The official fixed pattern measures 0.849385–0.849425 seconds across two clean
  runs. Rust all-Gray4 12 is 48.1 percent slower, so 12 is not the target.
- Rust's ordered set/clear and CKV/LE boundary plus fixed-size unchecked framebuffer
  reads measure 0.851007–0.851010 seconds across three clean runs: 32.4 percent
  faster than Rust 12 and only 0.189 percent slower than official. ELF gates prove
  the timing and traversal contracts; physical inspection matched the clean image.
- The Rust Gray4 driver now has a diagnostic-only fixed-pattern target fixture
  with a 180,000-byte PSRAM framebuffer and independent cleaning/framebuffer
  timing. Product UI still does not call it.
- Earlier shared-path tests were clean at 12 cycles and corrupted at 9 and 6
  cycles during partial updates. This proves that 12 provides useful margin;
  it does not prove a panel minimum or rule out a full-only experiment.

Exact ELF hashes, retained artifact directories, traces, and all binary/Gray4
timing results are in
[the assembly note](../../notes/panel-waveform-reference-assembly-2026-08-31.md).

## Scope

Compare these official functions from the sibling Inkplate library:

- `EPDDriver::partialUpdate()` for partial-refresh reliability;
- `EPDDriver::display1b()` and `EPDDriver::clean()` for binary full-refresh
  speed;
- `EPDDriver::display3b()` for native 3-bit grayscale full-refresh speed;
- `vscan_start()`, `hscan_start()`, and `vscan_end()` for shared row timing.

Compare them with the equivalent Rust cleanup, full-scan, partial-scan, and
waveform primitives under `boards/inkplate-tempera/src/`.

This plan does not change the five-minute Ambient Home policy, UI behavior, or
touch classification. The 3-bit work uses a diagnostic fixture until a product
feature explicitly adopts grayscale.

## Rules

- Compare compiled loops, not source formatting or nominal delay values.
- Hold pass count, pulse count, and ordering equal to the official workload in
  every same-workload timing experiment.
- Derive explicit CL-high, CL-low/data-setup, row-latch, and pass-spacing
  constraints in physical time from capture or panel evidence, including their
  tolerances. CPU cycles may implement a constraint but may not justify it.
- Treat waveform quantity and execution rate as separate experiment axes, while
  recognizing that excessive rate can still violate panel settling constraints.
- Record toolchain, CPU frequency, optimization, code placement, feature set,
  and ELF hash for each artifact.
- Change one timing or ordering property per diagnostic build.
- Keep partial, binary full, and 3-bit full timing independent. An experiment
  in one mode must not weaken another mode.
- Treat serial timing and boot health as separate from physical image quality.
- Keep the qualified 12/48 image available as the recovery baseline.
- Do not resume blind 12/9/6 hold-count bisection.

## Phase 1: Freeze the official build

Reproduce the official TEMPERA build at 240 MHz and retain its ELF, map, symbol
table, and disassembly. Record the known baseline metadata: library commit
`839da188`, library version 11.1.2, board package 8.1.0, FQBN
`Inkplate_Boards:esp32:Inkplate4TEMPERA`, and `-O2`.

Annotate one complete row from each relevant path:

- full cleanup;
- binary full framebuffer scan;
- 3-bit full framebuffer scan;
- partial framebuffer scan.

For each row, mark CL high and low phases, data changes, SPH/CKV/LE ordering,
row termination, pass delay, and code/data placement.

Output: one reproducible reference artifact and four short annotated traces.

## Phase 2: Compare the production Rust build

Extract the same traces from the qualified Rust 12/48 release ELF and a 3-bit
fixture build. Compare:

- GPIO writes and memory barriers;
- instructions between CL set and clear;
- work between consecutive pulses;
- data setup and hold around CL edges;
- first and final pulse in a row;
- CKV/LE row termination and the next-row boundary;
- flash versus RAM execution;
- interrupt masking and possible preemption.

For binary and 3-bit full refresh, account separately for:

- the explicit 12-cycle CL hold;
- the 1 microsecond row-latch margin;
- fixed 230 microsecond pass delays;
- panel power-on and shutdown time.

Classify each difference as required ordering, intentional margin, removable
overhead, or unresolved.

Output: one difference table and one recommended throughput experiment.

## Phase 3: Test one deterministic throughput change

Choose the smallest change supported by Phase 2. Likely candidates are:

- a fixed-instruction CL pulse with explicit set-to-clear timing;
- a deterministic CL-low or data-setup interval;
- a smaller row-boundary margin if the reference trace supports it;
- deliberate code placement only if placement itself is causal.

Do not optimize toward a chosen hold count or copy incidental latency. Without
edge-capture hardware, use the official compiled instruction sequence as the
contract, guard the final ELF, and qualify repeated timing and images. State
explicitly that this does not establish an electrical minimum or tolerance.

Keep the official 78-pass binary waveform unchanged during this phase. A
shorter pass sequence changes the electro-optical waveform and must not be
mixed with a source-timing experiment. Apply the same rule to the official
74-pass 3-bit waveform. Test timing changes in one display mode at a time.

## Phase 4: Validate and decide

Validate any candidate against the 12/48 baseline:

- repeat regular Ambient Home full refreshes at the real five-minute boundary;
- alternate high-contrast previous and next images and inspect cleaning,
  ghosting, contrast, bands, and weak rows;
- test a cold and a warm device state;
- run at least 20 partial updates, including physical press/release and page
  changes, after a full refresh;
- confirm timing, ELF identity, and absence of panic, watchdog, or unexpected
  full fallback in serial logs.

For 3-bit mode, also:

- capture a 48-cycle baseline before changing timing;
- render all eight gray levels, ramps, flat fields, and sharp boundaries;
- alternate dark, light, and mid-gray previous images to expose ghosting;
- repeat the refresh at cold and warm device states;
- confirm that leaving grayscale invalidates the binary partial baseline and
  that the next binary refresh recovers correctly.

Promote a candidate only when the identical waveform workload is faster and
physically as clean as both the official implementation and the retained Rust
baseline. Otherwise keep the fastest physically qualified implementation.

If same-pass timing work cannot materially improve the 1.43-1.45 second full
refresh, stop this investigation. Reducing the 67 cleaning passes is then a
separate waveform-quality experiment, not another timing tweak.

## Done when

- Partial refresh has a justified deterministic timing contract.
- Binary full refresh either matches or beats official same-workload throughput,
  or has a measured explanation for its remaining overhead.
- Native 3-bit full refresh has a measured baseline, independent timing, and a
  physically qualified same-workload throughput result.
- ELF checks cover the final timing and ordering assumptions.
- The final result separates waveform, panel-power, and scheduling cost.

## Closeout

Closed on 2026-08-31. The investigation satisfied its completion criteria:

- partial refresh retains an independent, deterministic 48-cycle contract;
- binary full preserves the official 78-pass workload and measures 6.27 percent
  faster than the official comparator;
- clean full Gray4 preserves the official 74-pass workload and measures within
  0.189 percent of the official comparator;
- fixed-pattern physical inspection qualified both final full-refresh paths;
- positive and negative ELF gates enforce code placement, ordered GPIO and row
  boundaries, retained partial timing, and fixed-size framebuffer traversal;
- retained artifacts separate waveform, panel-power, and scheduling duration.

This establishes qualified compiled software contracts, not electrical timing
minima. The broader cold/warm, alternating-image, and extended partial-update
matrix from Phase 4 was not completed and does not keep this assembly
investigation open. It remains visible as manual durability qualification in
the [hardware test matrix](../../reference/hardware-test-matrix.md#panel-waveform-durability-manual).

Reducing binary or Gray4 pass counts, or implementing transition-aware Gray4,
changes update-mode semantics and requires a separate plan.
