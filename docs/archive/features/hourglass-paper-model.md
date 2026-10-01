# Paper-Anchored Hourglass Cellular Model Plan

- Status: Done — accepted host visuals, exact duration calibration, target timing, and stack gates
- Last-reviewed: 2026-09-01
- Archived: 2026-09-01
- As-built reference: [Hourglass cellular model](../../reference/hourglass/model.md)

## Current checkpoint

The shared paper rule, its transition tests, and the fixed-down unpaced host preview are implemented.
The paper's 2x2 rule cannot pass through the existing one-cell collision throat because every block
at the neck also contains a wall. The host candidate therefore uses the smallest symmetric opening
that works in all four block positions: three collision cells across the neck. The same geometry is
now selected by the firmware renderer and collision model.

The first fixed-down preview was rejected because its upper free surface kept unrealistically steep,
straight V legs. The host model now adds a bounded local repose pass outside the Devlin-Schuster
transition table: a backed top-surface grain topples when neighboring column depth exceeds the
selected lattice repose limit. Rycroft, Wong, and Bazant motivate treating free-surface relaxation
as a distinct part of granular drainage, but this small stencil is a Medinote rule rather than an
implementation of their Spot Model. The cadence prevents surface relaxation from feeding the throat
faster than the base CA can clear it.

The production adapter rotates the same transition table for four cardinal gravity directions and
deterministically distributes arbitrary angles between neighboring axes. It meters only complete
throat-crossing blocks. The selected scheduler distributes those neighboring directions across the
disjoint blocks of one shared Margolus phase instead of assigning one direction to the entire
generation. This removes the direction/phase lock seen at some angles while preserving one update
per cell per generation. The release host calibration conserves all 4,500 cells and puts the final
cell into the destination bulb on tick 27,000, exactly 15 minutes at 30 Hz.

After host visual approval, the scheduler was enabled in the default Medinote target and flashed on
2026-09-01. Boot identified `paper-cellular`, `gravity_schedule=block-weighted`, 4,500 grains, and
900 seconds. On the 240 MHz ESP32-S3, steady upright physics peaked at 7,146 us over 1,801 ticks. A
fast 90-degree turn exercised intermediate arbitrary angles and raised the cumulative physics peak
to 8,410 us against a 33,333 us period. Physics plus the optional render peaked at 38,472 us. One
missed period was already present in the first report after activation; the counter did not increase
during the turn. The target build, linked memory, and modeled stack checks pass. Physical visual
observation remains separate.

## Goal

Build the best-looking hourglass model that fits Medinote's memory and 30 Hz physics budget. Use
Devlin and Schuster's cellular automaton as the compact, testable base, while keeping visually
necessary product additions explicit and bounded. Paper fidelity is a diagnostic anchor, not the
product goal. Keep Medinote's glass shape, 4,500 visible cells, rotation, and 15-minute duration.

We judged the model with host-generated images and GIFs before changing firmware. Device results
remain a separate validation layer.

The source papers and selection notes are in the
[Hourglass reference folder](../../reference/hourglass/README.md). Figures 1 and 2 of the
[Devlin-Schuster paper](../../reference/hourglass/papers/devlin-schuster-2020-probabilistic-cellular-automata.pdf)
define the base transition rule.

## Why change the model

The previous backend had separate rules for falling speed, rolling, wall release, pressure, and the
free surface. A change in one rule often changes another part of the pile. This makes the center
jump, heart-shaped upper sand, and M/W-shaped lower pile hard to fix with confidence.

The paper offers a smaller model with one material setting:

- a square grid with occupied or empty cells;
- non-overlapping 2x2 block updates;
- four repeating block positions;
- the movement patterns shown in Figure 2; and
- one probability, `p`, for the two stacked-grain topples.

We will start with `p = 0.75`. The paper tested this value and found values above `0.5` visually
pleasing. It is a starting point, not a value the paper declares to be correct for every sand.

## Why the production model is intentionally hybrid

The previous custom CA accumulated interacting pressure, wall-release, rolling, and surface knobs.
Those interactions produced heart-shaped upper sand and M/W-shaped lower piles that were difficult
to diagnose or tune. Replacing that core with the Devlin-Schuster block rule made the transition
set small, deterministic, allocation-free, and inexpensive enough for the ESP32-S3.

The unmodified block rule still produced long, thin, nearly straight V legs in host previews. Its
single toppling probability changes the frequency of one local transition, but it cannot separately
control the free surface's repose shape and its relaxation rate. A full Spot Model would add much
more state and computation than this display needs. The selected compromise is therefore:

- keep the paper's 2x2 transition function intact and exhaustively tested;
- add one bounded local repose pass for the visible free surface;
- keep repose shape and relaxation cadence as separate controls because they affect different
  visible behavior; and
- judge additions by visual results, determinism, conservation, memory, and 30 Hz performance.

This is not a one-paper reproduction. Literature is used to constrain and explain the model, not to
override better measured product behavior.

## Base rule and explicit additions

### Paper model

Use one shared implementation for host previews and firmware. Do not build a separate host model.

Update 2x2 blocks in the paper's four-position cycle:

1. `(x, y)`;
2. `(x + 1, y + 1)`;
3. `(x, y + 1)`;
4. `(x + 1, y)`.

Implement Figure 2 from its before-and-after cell patterns. The figure appears to skip the letter
`(g)`, so tests will use the bit patterns as the source of truth and use letters only as references.
All patterns not shown remain unchanged.

Most moves always happen. The two vertical-stack topples use the same `p`. A deterministic hash of
the seed, generation, and block position supplies the random choice, so the same run produces the
same result on host and device without storing random state per cell.

Glass cells count as occupied obstacles but never move. Reject a block update if it would clear or
move a glass cell. Test this directly.

The Figure 2 transition function is a faithful reimplementation, not a claim of bit-for-bit
reproduction. The complete production model is intentionally not paper-faithful: the paper does not
define Medinote's curved glass, initial packing, pacing, arbitrary-angle rotation, or repose pass.

### Medinote additions

Keep these outside the paper transition function:

- **Glass shape:** use the existing curved mask. Do not add wall forces.
- **Free surface:** use a separate, infrequent lattice repose pass, physically motivated by the
  Rycroft-Wong-Bazant treatment of free surfaces but intentionally much smaller than a Spot Model.
  Move only the backed topmost grain of an over-steep column. Keep angle and cadence separate: angle
  defines the stable shape, while cadence limits how quickly the surface supplies the throat.
- **Starting fill:** keep the deterministic compact upper fill. It is exactly stable on the first
  closed-throat generation; do not add a costly hidden settling phase. Small later edge relaxation
  is expected once the normal four-phase schedule advances.
- **15-minute duration:** first view the unpaced paper model. After it passes, add throat pacing. It
  may delay a whole paper-legal block update that crosses the throat; it must not invent a move or
  apply half a block transition.
- **Rotation:** rotate the paper transition table for cardinal gravity. For shallow and diagonal
  angles, distribute neighboring cardinal rules deterministically from the continuous gravity
  components. Keep one shared four-phase partition and one update per cell per generation. Do not
  add pressure or wall rules.

## Host preview output

The preview contains sand cells and a small time footer only. It does not draw the glass, product UI,
or tracers.

Host and firmware use the same rotated-cell coverage rule. Each occupied lattice site rasterizes its
unit square rather than only forward-mapping its centre pixel; this prevents regular white holes in
dense sand at non-cardinal angles. White sites that remain are actual voids in the CA occupancy.

Every PNG and every GIF frame shows `t=MM:SS` below the sand area. This is elapsed on-device time,
calculated from model ticks and `PHYSICS_HZ`. It is not GIF playback time and not the model's
remaining-time estimate. A host run may finish quickly while the label still shows the time the
device would have reached.

Save images at useful moments: ready, the first running frame, 30 seconds, 100 seconds, mid-run, and
late-run. Use more frames when they help explain a visible problem. Do not add a general comparison
framework; save the current model's baseline images before replacing it and compare only when useful.

The preview accepts a deterministic rotation trace as tick-and-degree pairs, for example
`--rotate 1:15,900:-30,1800:105`. Use it to test shallow tilt, reversal, and fast 90-degree turns
without flashing the device.

## Rotation-scheduling decision

The paper specifies a four-phase block schedule only for one fixed gravity direction. Medinote must
extend that rule because the glass can stop at any angle.

The first production extension assigned one cardinal direction to each whole generation while one
global partition phase kept advancing. That correlated direction with phase: a stream could look
correct at 15 degrees but diagonal at 45 degrees. A host experiment with a separate phase counter
for each cardinal direction moved the artifact instead of removing it: 45 degrees improved while
15 degrees became visibly diagonal. Independent counters were therefore rejected; they do not
provide arbitrary-angle gravity.

The selected `hourglass-block-gravity` scheduler retains one shared global partition. Each
disjoint 2x2 block chooses the vertical or horizontal cardinal transition from the two components of
the full gravity vector. A deterministic hash of seed, generation, and block position supplies the
spatial distribution, so it stores no per-cell random state and remains reproducible. Every cell is
still visited by at most one block and receives at most one paper transition per generation.

Matched flowing previews at -60, -45, -30, -15, 15, 30, 45, and 60 degrees show that the old
constant lattice-direction drift is gone. The 75-degree case confirms the existing near-horizontal
flow pause. Curvature beginning at the rotated throat or wall is acceptable: the glass can redirect
the stream. This is a visual product rule, not a claim that the cellular trajectory is a continuous
mechanics solution.

The custom repose pass is still column-based and runs only when the separately scheduled surface
direction is up or down. Rotating that stencil may make sideways and diagonal settling more
consistent, but it is a separate product experiment and is not required to validate the base stream
direction. The scheduler passed human host-preview approval, release calibration, and target timing;
it is now the Medinote target default. Do not change it solely for paper fidelity.

## Work order

### 1. Shared paper model and fixed-down host preview

- Put the paper rule in its final shared library module.
- Test every 2x2 input pattern, the four block positions, mass conservation, and fixed walls.
- Run a small seed set at `p = 0.25`, `0.50`, `0.75`, and `1.00` on a 61x61 paper-style hourglass.
  Confirm the paper's broad trend: lower `p` gives a taller, slower-settling pile. Do not try to
  reproduce the paper's averaged figure from one seed.
- Run the same core with Medinote geometry, fixed downward gravity, and no 15-minute pacing.
- Generate labeled PNGs and GIFs.

This checkpoint passed visual review before paced integration began.

### 2. Add Medinote behavior on the host

- Generate and test first-frame continuity of the compact starting fill.
- Add throat pacing and confirm that it does not change legal movement away from the throat.
- Check the 15-minute upright run with labeled images and a GIF.
- Add cardinal rotation, then test the simplest shallow/diagonal approach on the host.
- View shallow tilt, diagonal, sideways, fast turns, slow turns, and reversal while sand is moving.

The paced upright path and rotated transition adapter are implemented. The release-only full-run
test covers the expensive 27,000-tick calibration.

### 3. Replace the firmware backend

- Replace the current backend with the already accepted shared model. Git keeps the old version, so
  do not maintain a second device CA backend.
- Remove the old surface, pressure, ballistic-fall, wall-release, gravity-order, and `SandMaterial`
  code in the same change so no orphaned modules remain. Completed.
- Measure host tick time and the target's memory and stack use.
- Build and test the target, then flash it with the repository's normal flash wrapper.
- Check the same visual moments and the 15-minute timing on the physical device.

## Acceptance checklist

- The transition tests match the paper's cell patterns and four-position schedule.
- Sand count is conserved, walls never move, and sand stays inside the glass.
- Ready and the first running frame do not show a bulk jump.
- The upper bulb drains from the throat area toward the outer edges; it does not leave a stable heart
  shape caused by faster movement near the walls.
- The lower pile does not keep a stable M/W outline. A temporary one-sided arm is acceptable.
- Each lattice site takes part in at most one block update per generation.
- Pacing delays only complete throat-crossing transitions and reaches the 15-minute target without
  changing `p`.
- Rotation does not create a visible jump, persistent side bias, center spike, or wall falloff.
- Host physics fits the 30 Hz budget with headroom.
- The target build passes the repository's DRAM and stack checks.
- Device behavior matches the accepted host images closely enough for the display.

## Storage and performance

Use a compact occupancy grid and a fixed glass mask. Keep the model allocation-free and initialize
large storage in place. Read [the DRAM budget](../../reference/dram/dram-budget.md) before choosing the
final target representation.

Profile the simple full-grid update first. Add an active-region optimization only if host and target
measurements show it is needed. An optimization must not change reference hashes for the same seed
and input trace.

## Validation commands

Run the preview-specific tests as well as the normal product and target checks:

```sh
cargo test -p medinote --example hourglass_preview
cargo test -p medinote
cargo clippy -p medinote --all-targets -- -D warnings
cargo fmt -p medinote --check
scripts/ci/check_stack_risk.sh
targets/medinote-waveshare/build.sh --locked
scripts/ci/check_stack_risk.sh --medinote-elf \
  targets/medinote-waveshare/target/xtensa-esp32s3-none-elf/release/medinote-waveshare
```

Use `scripts/device/flash.sh` only after host visual approval. Host previews prove software behavior;
they do not prove target speed or physical display quality.
