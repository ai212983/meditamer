# Hourglass cellular model

This document describes the Hourglass sand model shipped by the Medinote Waveshare target. The
model is a deterministic, allocation-free cellular automaton built from the Devlin-Schuster 2x2
rule plus explicit Medinote geometry, pacing, rotation, rendering, and free-surface behavior.

The objective is the best natural-looking result that conserves sand and fits the ESP32-S3 memory
and 30 Hz physics budget. Paper fidelity is a diagnostic anchor, not the product goal.

## As-built configuration

| Property | Production value |
| --- | --- |
| Visible sand cells | 4,500 |
| Physics cadence | 30 Hz |
| Render cadence | 10 Hz |
| Upright duration | 900 seconds / 27,000 physics ticks |
| CA grid | 119 x 173 cells |
| Interior throat width | 3 cells |
| Paper toppling probability | 750 per mille |
| Repose run | 3 cells |
| Surface-relax interval | 4 generations |
| Arbitrary-angle scheduler | `block-weighted` |

The Medinote target enables `hourglass-block-gravity` by default. Boot identifies the active
combination as:

```text
HOURGLASS_BACKEND name=paper-cellular gravity_schedule=block-weighted grains=4500 duration_s=900
```

The retained position-based solver remains available behind `hourglass-pbd` as an A/B control.

## Paper rule

[Devlin and Schuster](papers/devlin-schuster-2020-probabilistic-cellular-automata.md) defines the
base transition table. The implementation:

- stores sand and fixed walls as compact occupancy bitmaps;
- visits non-overlapping 2x2 blocks in the paper's four-position Margolus cycle;
- implements every Figure 2 before/after pattern directly;
- leaves unlisted patterns unchanged;
- treats wall cells as occupied obstacles and rejects any transition that would move one; and
- uses a deterministic hash of seed, generation, and block position for the two probabilistic
  stacked-grain topples.

The transition function is exhaustively tested across all 16 occupancy patterns. Host previews and
firmware use this same model; there is no separate visual-only simulation.

## Medinote extensions

The paper does not define Medinote's product geometry or behavior. These additions remain outside
the paper transition function:

- **Curved glass:** a cubic Bezier half-width profile defines the collision walls. A one-cell throat
  is sealed for a 2x2 block rule, so the collision opening is the smallest symmetric width that works
  in every phase: three interior cells.
- **Starting fill:** 4,500 cells are packed deterministically from the center outward in the upper
  bulb. The first closed-throat downward generation is stable, avoiding a hidden settling pass and
  its visible start jump.
- **Free-surface repose:** an infrequent column stencil moves one backed top-surface grain when the
  neighboring surface is too steep. Repose run controls stable shape; interval controls relaxation
  speed. The Spot Model papers motivate treating free-surface behavior separately, but this bounded
  stencil is a Medinote rule, not a Spot Model implementation.
- **Throat pacing:** accumulated credit may admit only complete paper-legal transitions across the
  bulb boundary. Pacing never moves half a block or changes the toppling probability.
- **Arbitrary-angle gravity:** the cardinal transition table is reused without adding pressure or
  wall-force rules. The scheduler described below maps the full gravity vector onto it.
- **Rotated-cell rendering:** each occupied site rasterizes its rotated unit square rather than only
  its center pixel. This prevents regular white holes in dense sand at non-cardinal angles; remaining
  white sites are real CA voids.

The result is intentionally hybrid. The former pressure, rolling, wall-release, and surface rules
interacted in ways that produced heart-shaped upper sand and M/W-shaped lower piles. The smaller
paper core plus one explicit repose pass is easier to test and diagnose.

## Arbitrary-angle scheduler

Gravity is transformed into glass-local coordinates. Exact cardinal gravity uses the matching
rotated paper transition table directly. At other angles, every generation retains one shared global
Margolus partition. Each disjoint 2x2 block chooses either the vertical or horizontal neighboring
cardinal transition, weighted by the absolute components of the full local gravity vector.

A deterministic spatial hash selects the direction for each block. This has three important
properties:

- one block partition remains shared across the whole grid;
- every cell receives at most one paper transition per generation; and
- no per-cell random or phase state is stored.

The original whole-generation scheduler correlated direction with partition phase. A stream could
look correct at 15 degrees and diagonal at 45 degrees. Giving each cardinal direction an independent
phase counter moved the artifact: 45 degrees improved while 15 degrees became diagonal. That option
was rejected. Spatial block weighting removed the persistent lattice-direction drift in matched
previews at positive and negative 15, 30, 45, and 60 degrees.

Curvature caused by the rotated throat or walls is expected: the glass may redirect the stream. The
75-degree case remains in the near-horizontal repose band and pauses throat flow.

The custom repose pass is still column-based. It runs only when the separately scheduled surface
direction is up or down; left/right rotated repose is not part of the accepted model.

## Tuning boundaries

The main production constants live in
[`paper_cellular.rs`](../../../platform/ui/visuals/hourglass/src/paper_cellular.rs) and
[`paper_backend.rs`](../../../platform/ui/visuals/hourglass/src/paper_backend.rs):

- `DEFAULT_TOPPLE_PER_MILLE` changes the paper's two probabilistic topples;
- `DEFAULT_REPOSE_RUN_CELLS` changes the accepted free-surface slope;
- `DEFAULT_SURFACE_RELAX_INTERVAL` changes how quickly that surface relaxes;
- `MEDINOTE_GRAINS` changes visible sand amount and requires duration and capacity revalidation;
- the Medinote half-width/height and `medinote_half_width` define collision geometry; and
- `THROAT_PACING_TICKS` and `MAX_BLOCK_CROSSINGS` enforce the calibrated discharge.

Do not tune several of these together without isolated host previews. In particular, toppling
probability cannot independently fix repose shape, and pacing should not compensate for a visual
physics defect.

## Host preview

`hourglass_preview` renders cells plus an elapsed on-device-time footer. It deliberately omits the
glass and product UI. A rotation trace is a comma-separated list of `tick:degrees` pairs:

```sh
cargo run -p medinote --release --example hourglass_preview \
  --features hourglass-block-gravity -- \
  --out logs/hourglass-preview --seconds 120 \
  --png-at 0,30,100,120 --rotate 1:15,900:-30,1800:105
```

Use host images and end-to-end GIFs before flashing. Useful checkpoints are ready, first running
frame, 30 seconds, 100 seconds, mid-run, and completion. Mirror positive and negative angles when
investigating directional bias.

## Calibration and target performance

The release calibration conserves all cells and completes an upright session on public tick 27,000,
exactly 15 minutes at 30 Hz. The measured credit horizon is two ticks shorter because the four-phase
block sweep admits the final complete crossing two ticks later.

The accepted target measurement used the Waveshare ESP32-S3-RLCD-4.2 at 240 MHz:

| Device interval | Physics maximum | Total-work maximum | Missed periods |
| --- | ---: | ---: | ---: |
| Steady upright, 1,801 ticks | 7,146 us | 37,584 us | 1 |
| After a fast 90-degree turn, 2,701 ticks | 8,410 us | 38,472 us | 1 |

The animated turn exercises intermediate arbitrary angles. Its block-weighted physics peak uses
about one quarter of the 33,333 us physics period. The single missed period was already present in
the first post-activation report and did not increase during the turn. Total work includes the
optional 10 Hz render, so it may exceed one physics period while the scheduler catches up without
dropping a physics step.

The linked target passed the modeled stack gate with 83,448 bytes of Hourglass-path margin. See
[Runtime Metrics](../runtime/metrics.md#hourglass-timing) for the measurement fields and
procedure.

## Validation contract

Changes to the model should preserve:

- exact transition-table tests and the four partition phases;
- deterministic results for a fixed seed and command trace;
- sand conservation, fixed walls, and bounded interior occupancy;
- first-frame continuity and the accepted upper/lower pile shapes;
- complete-transition-only throat pacing and the 15-minute release calibration;
- mirrored arbitrary-angle previews without persistent lattice bias;
- host golden fingerprints for upright and rotation traces;
- target build, linked DRAM, and stack gates; and
- on-device physics headroom during a fast turn.

The closed implementation history is retained in the
[paper-model plan](../../archive/features/hourglass-paper-model.md).
