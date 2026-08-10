# Panel Waveform Reference-Assembly Investigation Plan

- Status: Active
- Last-reviewed: 2026-08-10

## Goal

Determine which compiled timing and ordering differences between the official
Inkplate 4 TEMPERA partial-refresh implementation and the Rust implementation
cause panel corruption, then replace incidental timing with the smallest
explicit, deterministic contract that remains reliable.

The investigation must answer two separate questions:

1. What sequence does the compiled reference implementation actually emit?
2. Which parts of that sequence must the Rust implementation preserve?

The reference implementation is an implementation baseline, not an electrical
specification for the ED038TH2 panel. It cannot establish undocumented minimum
timings by itself.

## Current Evidence

- The relevant reference path is `EPDDriver::partialUpdate()` in
  `../../../Inkplate-Arduino-library/src/boards/Inkplate4TEMPERA/Inkplate4TEMPERADriver.cpp`.
- The official Inkplate 4 TEMPERA board configuration runs the ESP32 at
  240 MHz.
- The compiled reference CL pulse contains ordered GPIO writes:
  `memw -> GPIO set -> memw -> GPIO clear`.
- The current Rust binary also emits `memw` around its GPIO writes, but inserts
  a CCOUNT-based hold between the CL set and clear writes.
- The reference partial-update loop executes from flash-mapped text, while the
  Rust panel scan is deliberately placed in RAM. Code placement can change
  instruction and data-fetch latency even when the source operations appear
  equivalent.
- Hardware tests found a clean result with the 12-cycle hold and corruption
  with the 9-cycle and 6-cycle holds in the current build lineage.
- Changing the CL-high hold also changes the complete source-clock period and
  data-settling window. Those results do not prove that CL-high width alone is
  the failing parameter.
- Generic SPI timing tables do not apply. TEMPERA drives the raw parallel panel
  through D0-D7, CL, LE, SPH, CKV, SPV, OE, and GMOD.
- At six cycles, four controlled synthetic transitions remained visually clean.
- A blank physical tap produced no LVGL render or panel refresh. The identical
  synthetic transition 500 ms later corrupted, despite touch acquisition being
  suspended and the touch controller being powered off before the waveform.
- Six-cycle post-touch and pre-touch framebuffer hashes, dirty geometry, scan
  rows, and transition data matched. The known-clean 12-cycle build tolerates
  both histories, so 12 cycles is margin, not a derived panel requirement.
- The archived two-finger failure is a `LoadProhibited` exception, not a panel
  deadlock. Its retained ELF resolves the faulting PC to
  `lv_event_get_gesture_state` while dereferencing an invalid recognizer
  pointer.

## Investigation Rules

- Treat the compiled reference waveform as the implementation baseline.
- Compare complete compiled loops, not isolated source statements.
- Keep the reference and Rust toolchain, optimization level, CPU frequency,
  linker placement, and feature set recorded with every comparison.
- Change one timing or ordering property per diagnostic build.
- Do not infer a panel timing requirement from a passing delay value.
- Do not resume blind delay-count bisection.
- Keep the known-clean 12-cycle build available as the recovery baseline.
- Preserve the gesture-event guard and touch-release fixes while investigating
  the display waveform.
- Do not copy the entire reference driver. Reproduce only behavior supported by
  disassembly or hardware evidence.

## Phase 0: Establish Touch/No-Touch Equivalence

Before attributing corruption to waveform timing, compare synthetic and
physical input with identical LVGL and panel preconditions.

Use the existing no-op `TOP TEST` button. A diagnostic build shall:

1. Start from the completed startup full refresh.
2. Inject synthetic `Down` and `Up` events through the same
   `Backend::handle_touch` and `refresh_panel` calls as physical input.
3. Return the framebuffer and previous-frame baseline to the original released
   state.
4. Wait for one physical tap on the same button.
5. Compare down and release independently.

For every phase, record:

- input source and phase
- current and previous framebuffer hashes
- changed bytes and pixels
- changed row and byte-column bounds
- LVGL dirty rectangle
- selected refresh strategy and transition/neutral row counts
- panel terminal-hold and recovery state

The current diagnostic is enabled at compile time with
`MEDITAMER_TOUCH_EQUIVALENCE_PROBE=1`. It automatically runs the synthetic pair
after startup and prints `PANEL_EQUIV state=awaiting_physical` before accepting
the physical comparison.

`MEDITAMER_TOUCH_PRIME_THEN_SYNTHETIC_PROBE=1` adds the decisive control: two
synthetic pairs, one blank physical tap with no render, then the same synthetic
pair 500 ms later.

`MEDITAMER_TOUCH_PIPELINE_REPLAY_PROBE=1` replaces that physical prime with a
fixed tap emitted by the core-1 acquisition task through the production touch
pipeline. It excludes controller I2C, GPIO36, and physical contact while
retaining acquisition-core execution and cross-core delivery.

Interpretation:

- Matching render signatures with different physical outcomes establishes a
  touch-dependent runtime, concurrency, cache, or power-sequencing effect.
- Different signatures establish that the earlier tests selected different
  geometry or transition data; waveform timing cannot yet be isolated.
- Matching signatures and outcomes move the investigation to the complete
  compiled waveform comparison in Phase 1.

The multi-touch crash is fixed and tested separately from this equivalence
probe. Gesture callbacks must accept only the exact registered LVGL input
device, and only completed recognizer events enter the UI queue.

Phase 0 result: complete. Touch-triggered UI content, concurrent acquisition,
and controller activity during the scan are ruled out. Real touch leaves a
persistent runtime precondition that exposes the six-cycle waveform margin.
A core-1 pipeline replay completed three untouched six-cycle runs without
corruption, with matching pre/post render signatures. Acquisition-core timing,
cross-core delivery, normalization, classification, and LVGL delivery are not
sufficient; the remaining discriminator is the physical GPIO36/report-read
path before the later waveform.
Phase 1 must compare the complete reference and Rust loops; it must not treat
12 cycles as a source-clock specification.

Exit criteria:

- Synthetic and physical down/release signatures have explicit match verdicts.
- The operator records whether each of the four physical updates is visually
  clean or corrupt.
- No waveform-delay conclusion is drawn from non-equivalent signatures.

## Phase 1: Freeze the Compiled Reference Baseline

Compile the official partial-update example with the same board definition used
for the reference result and retain:

- board package and compiler versions
- fully qualified board name and CPU frequency
- compile flags and optimization level
- ELF file and link map
- symbol table
- disassembly of `partialUpdate`, `vscan_start`, cleanup passes, and any
  non-inlined helpers
- section placement for code and lookup tables

Record the actual instruction sequence for:

- first source pulse in a row
- steady-state inner-loop pulse
- final source pulse in a row
- CKV and LE row termination
- SPH transition around the first source pulse
- delay and control flow between consecutive rows
- delay and control flow between waveform passes

Exit criteria:

- The reference artifacts can be regenerated with a documented command.
- Every GPIO transition relevant to one row is mapped to its compiled
  instruction sequence and section.
- No conclusion depends only on C++ source formatting or nominal delay calls.

## Phase 2: Compare the Complete Rust and Reference Loops

Build the Rust firmware with the same production feature set used by the
hardware test. Extract the equivalent ELF, link map, symbols, section
placements, and disassembly.

Compare the implementations across these dimensions:

| Area | Evidence to compare |
| --- | --- |
| CL high phase | GPIO set, barriers, intervening instructions, GPIO clear |
| CL low phase | LUT lookup, framebuffer load, address arithmetic, loop branch |
| Data timing | When D0-D7 change relative to both CL edges |
| First pulse | SPH state and the first data/CL write |
| Final pulse | Neutral/final data, CL transition, and data clearing |
| Row termination | CKV clear, LE pulse, and next-row boundary |
| Pass boundary | 230 us delay and vertical-scan restart |
| Code placement | Flash versus RAM execution and cache exposure |
| Data placement | LUT/framebuffer memory region and load latency |
| Concurrency | Interrupt mask, task scheduling, and possible preemption |

Produce a side-by-side annotated instruction trace for at least one complete
row. Classify every difference as:

- required ordering difference
- intentional safety margin
- incidental compiler or placement difference
- unrelated bookkeeping
- unresolved and requiring a controlled test

Exit criteria:

- The comparison explains high phase, low phase, LUT/data preparation, row
  termination, and code placement as one waveform pipeline.
- Any proposed causal difference identifies the exact instructions and the
  signal interval they affect.
- The investigation does not describe a nominal delay count as a physical
  timing specification.

## Phase 3: Reproduce Relevant Ordering Deterministically

Replace each relevant incidental property with an explicit implementation
contract. Depending on Phase 2 evidence, this may include:

- explicit memory ordering around GPIO writes
- a fixed instruction sequence for the pulse primitive
- deliberate data setup or hold before the relevant CL edge
- a deterministic CL high or low interval
- explicit LE, CKV, or SPH ordering
- deliberate code and data placement where placement is proven relevant
- an interrupt boundary covering exactly the timing-critical region

Prefer hardware-timed or fixed-instruction behavior over delays whose effective
duration changes with compiler layout. If additional timing evidence is needed,
use the ESP32 RMT receiver as an internal digital pulse capture; no external
oscilloscope or logic analyser is required for that diagnostic.

Each diagnostic build must:

- begin from the same known-clean functional state
- alter only one classified difference
- log its waveform configuration at boot
- retain a recoverable firmware image and capture log
- avoid changing carousel geometry, LVGL state logic, or touch classification
  in the same slice

Exit criteria:

- The Rust implementation has an explicit reason for every timing-critical
  instruction or delay.
- Correctness no longer depends accidentally on inlining, cache behavior, or
  unrelated LUT work.
- The implementation remains compatible with the panel ownership and
  interrupt-safety model.

## Phase 4: Retain Only Necessary Explicit Margin

After the relevant reference behavior is reproduced, add margin only where the
Rust implementation cannot safely or deterministically match it.

For every retained margin, document:

- the signal interval it protects
- the deterministic primitive used to create it
- its derived or measured duration
- the clean and failing comparison builds
- why a smaller or reference-equivalent interval is unsafe
- its cost per row, pass, and partial update

Do not describe the margin as an ED038TH2 requirement unless an authentic panel
specification supplies that requirement.

Validate the candidate with:

1. At least 20 automated partial updates without touch input.
2. Repeated physical press and release transitions on both navigation buttons.
3. Repeated carousel page switches in both directions.
4. Multi-touch gesture tests, including the prior two-finger swipe freeze case.
5. A check that no button remains logically pressed after release.
6. Serial confirmation that no panic, watchdog reset, or unexpected full
   refresh occurred.

Exit criteria:

- No panel corruption occurs in the complete validation sequence.
- Pressed-state feedback remains physically visible and responsive.
- The final margin is justified by evidence rather than delay-count search.
- Refresh-time impact is measured and separated into waveform, panel-power,
  and UI scheduling costs.

## Deliverables

- Reproducible reference and Rust build metadata.
- Side-by-side annotated assembly for the complete inner row loop.
- A difference matrix with causal status for every relevant divergence.
- Diagnostic logs and hardware outcomes for each one-variable slice.
- The final deterministic pulse/row implementation and its placement checks.
- A short conclusion stating what was reproduced, what margin remains, and
  which panel requirements remain unknown.

## Stop Conditions

Stop and reassess before proceeding if:

- the official compiled reference corrupts the same device under the same test
- a diagnostic changes more than one timing dimension
- the generated assembly no longer matches the reviewed sequence
- the panel cannot be returned to a known-clean state
- evidence points to power sequencing, waveform data, or geometry rather than
  the source/gate timing loop
