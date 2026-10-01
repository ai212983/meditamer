# Medinote Hourglass App Plan

- Status: Done — closed on the identified Waveshare board with an exact 15-minute committed-firmware
  run, retained deadline/cue metrics, host coverage for both granular backends, and explicit deferred
  follow-ons. See the
  [implementation ledger](medinote-hourglass-app-ledger.md).
- Last-reviewed: 2026-08-24
- Archived: 2026-08-24
- Depends on: phases 1–3 of [Product and target axis completion](product-target-axis-completion.md)
- Architecture: [UI and application structure](../architecture/0007-ui-and-application-structure.md),
  [compiled app catalogue](../architecture/0013-compiled-only-ui-catalogue.md), and
  [platform, board, and product axes](../architecture/0015-two-product-platform-workspace.md)

## Goal

Deliver a compiled Medinote hourglass app for the Waveshare S3 target whose glass can assume any
angle and whose sand responds continuously to gravity, moving walls, and the throat. Full-glass
duration and angle-dependent ETA remain calculable. Product state and presentation remain
host-testable; the target supplies clocks, rotation commands, LVGL scheduling, and panel control.

## Product contract

Session and orientation are independent state axes:

```text
Session:     Ready -> Running <-> Paused -> Complete
Orientation: commanded angle in [0, 360 degrees) + angular velocity
```

- The selected duration is the exact full-glass discharge time while one end remains vertically down.
- V1 maps each device `KEY` press to a 90-degree clockwise turn. The rendered glass moves
  continuously between cardinal angles.
- At a stable horizontal angle, flow settles to zero and timer progress pauses. Turning to either
  vertical orientation lets flow resume in the gravity-correct direction.
- Later keyboard or gamepad buttons provide clockwise and counterclockwise rotation commands that can
  stop the glass at any angle.
- Sand reacts during slow turns, fast turns, reversals, and stationary tilted states.
- Moving walls carry or deflect contacting grains; free grains retain inertia and continue under
  world gravity.
- Particle motion and throat geometry determine flow at every angle; angle has no separate rate table.
- Every orientation change preserves sand and updates remaining time from actual mass crossings and
  measured throughput.
- A delayed or skipped render frame changes animation smoothness, not orientation or transferred mass.
- A stable horizontal position pauses after in-flight grains settle; rotating away resumes naturally.
- Completion produces one visual cue and one optional quiet sound cue.

The fixed V1 geometry uses real wall-clock gravity and takes its duration from a deterministic
upright reference run. Longer future durations must change mass or throat geometry rather than slow
gravity. ETA is conditional on observed flow and the current lower chamber; a settled horizontal
glass reports `Paused` until crossings resume.

All inputs emit timestamped `RotationCommand` values into one product-facing contract. V1 supplies
`RotateBy(+90 degrees)` from `KEY`; keyboard and gamepad adapters later add press/release commands for
continuous clockwise and counterclockwise rotation.

## Ownership

Build the app within the product and target structure established by the dependency plan:

```text
products/medinote/src/
  apps/
    mod.rs
    hourglass/
      mod.rs
      model.rs          # session state and sand-mass ledger
      rotation.rs       # commands, angle, and angular velocity
      backend.rs        # compile-time cellular/PBD A/B selection
      cellular.rs       # production cellular granular solver
      particles.rs      # retained position-based A/B control
      duration.rs       # host-only duration/capacity planning
      tracers.rs        # one-pixel sub-pixel-flow presentation
      presentation.rs   # product-level frame data and labels
  power.rs              # product power intent, not target sleep mechanics
  catalogue.rs

boards/waveshare-rlcd42/src/
  buttons.rs            # timestamped KEY events
  panel.rs              # existing refresh and power controls

targets/medinote-waveshare/src/
  runtime_ui.rs          # shell, input, Embassy cadence, and UI actions
  sleep.rs               # Waveshare-specific Sleep/Deep Sleep mechanics
  ui/hourglass_widget.rs # LVGL projection
```

Register the hourglass as a compiled `Launchable` `AppRoot`. The shell owns navigation and
lifecycle; the app owns its model and local presentation, following the
[provider and surface contract](../architecture/0007-ui-and-application-structure.md#provider-and-instance-contract).

## Rotation, flow, and time model

Store full-glass duration `T`, total mass `N`, per-particle mass and chamber membership, commanded
angle `a`, angular velocity, observed throughput, and monotonic timestamps. For a V1 turn by
`da = pi/2`, let `u = clamp((t - t0) / tau, 0, 1)` and use smoothstep motion:

```text
s(u)       = 3*u^2 - 2*u^3
a(t)       = wrap(a0 + da*s(u))
angular_v  = da*(6*u - 6*u^2)/tau
```

For glass-local point `r`, widget center `c`, rotation matrix `R(a)`, and quarter-turn operator `J`,
the moving boundary is `x = c + R(a)r` with velocity `v_wall = angular_v*J*(x - c)`.
The retained PBD A/B control predicts each particle at fixed step `dt` and iteratively projects these
constraints:

```text
particle contact: length(p_i - p_j) >= radius_i + radius_j
glass contact:    signed_distance(p_i, rotating_glass) >= radius_i
v_next = (p_corrected - x) / dt
```

The production cellular backend instead applies the same local gravity to bounded occupancy-cell
moves and wall-release rules, avoiding pair searches and iterative projections. Use fixed-point turn
units, positions, and mass accumulators; keep sine/cosine lookup data in flash.

In glass-local coordinates, gravity is `g_local(a) = R(-a)*g_world`; together with `v_wall`, this is
the only direct angle input. Flow is an output of particle motion through the rotated throat. For
material parameters `theta`, obtain upright discharge time `T_ref(theta)` from a deterministic
reference solve:

```text
crossed_mass         = sum(mass_i for geometric throat crossings)
source_mass          = mass in the bulb gravity is currently draining
vertical_flow        = abs(g_local.y) / abs(g_world.y), or zero in the repose region
remaining            = T_ref * source_mass / (N * vertical_flow)
observed_rate        = EWMA(crossed_mass / dt_wall)
conditional_eta      = source_mass / observed_rate
```

Choose `theta`, including throat width, grain size, and particle count, from a stable-flow range.
Every geometric crossing transfers mass immediately. The production cellular packet limiter uses
the same `vertical_flow` factor as the displayed ETA, so a 180-degree turn rebases both flow and time
to the sand accumulated in the opposite bulb, while every intermediate angle from 0 through 360
degrees selects direction from the sign of `g_local.y` and slows both consistently from its magnitude.
Report no finite ETA in the repose region. The observed-rate ETA remains a diagnostic for departures
from that calibrated model. `Paused` follows a settled no-flow orientation immediately, or sustained
zero throat crossings at a flow-capable angle. Bulk settling within either bulb neither advances the
timer nor keeps the session `Running`. `KEY` targets the next 90-degree position; later press/release
commands target angular velocity and stop at arbitrary angles.

## Granular model and rendering

- Keep both bounded granular backends for A/B testing: the 384-grain PBD control and the selected
  cellular production backend.
- Production uses 3,072 occupied one-pixel cells. For the fixed 15-minute V1, cells are weighted mass
  packets representing 53,998 logical sub-pixel grains; short-lived one-pixel tracers make throat flow
  visible without adding that many solver records.
- If more grains must fit the fixed pixel area, scale physical grain size and collision radius
  together and rasterize the resulting subpixel coverage. An isolated falling grain must remain
  visible as exactly one pixel even when its diameter is only 0.1 pixel. This is a visibility floor,
  not a larger collision footprint: two or more subpixel grains covering the same output pixel still
  produce that one pixel, without increasing its size or intensity. Grouped grains therefore render
  from the union of covered pixels rather than one forced pixel per particle.
- Keep particles in screen/world coordinates under a constant downward gravity vector.
- Represent the rotating glass as line and curve collision boundaries transformed by continuous angle.
- Include boundary velocity in contact response so moving walls push grains while free grains retain
  momentum.
- In the retained PBD control, resolve particle contacts through a bounded uniform-grid broad phase
  and 3-5 position iterations.
- Tune PBD restitution/friction and cellular mobility/roll/wall-release parameters against settling,
  pile angle, rebound, and flow stability.
- Weight grains as visible mass packets and reuse short-lived throat tracers for long durations.
- Start device tuning near 50 Hz physics and 20 Hz rendering, then retain the fastest cadence that
  meets the identified-device deadline gate. The Phase 5 result is 30 Hz physics and 10 Hz rendering
  on the ESP32-S3 at 240 MHz. Interpolate only presentation.
- Preserve every physics step and skip render frames first; unresolved physics backlog is a timing
  fault rather than simulated progress.
- Render through one LVGL widget and invalidate the hourglass bounding rectangle once per frame.
- Use silhouette, stipple, pile shape, and motion for the monochrome panel.
- Keep the ST7305 in its normal low-power drive while animated, paused, complete, and outside the
  app. Identified-panel validation showed reduced contrast in high-power mode, while regional writes
  remain immediate in either mode; runtime CPU/performance intent must not change the LCD drive
  waveform. The relevant measurements are recorded in
  [ADR-0015](../architecture/0015-two-product-platform-workspace.md#frctrl-is-self-refresh-not-write-latency).

## Delivery

### 1. Product foundation

- Complete the Medinote product/target roots needed by this app.
- Add `apps/hourglass` and one compiled catalogue entry with stable provider, entry, and surface ids.
- Add a Medinote host-test suite to the repository's host-suite inventory.

Exit: the empty app enters and leaves through the shell on host and target builds.

### 2. Rotation and deterministic mass model

- Implement timestamped `RotationCommand` handling, continuous angle, angular velocity, calibrated
  model time, geometric crossing accounting, observed-flow ETA, and mass conservation.
- Cover calibration bounds, pause/resume, each 90-degree V1 transition, arbitrary stationary angles,
  slow/fast rotations, complete circles, reversals, stale events, and irregular render schedules.

Exit: a fixed command trace and fixed physics steps produce the same angle, crossing ledger, and ETA
under every render schedule.

### 3. Physics and presentation

- Implement the fixed-capacity particle store, rotating boundaries, broad phase, contact solver,
  geometric throat crossing, model-time scaling, and physics-backlog accounting.
- Produce deterministic frame data without LVGL or hardware dependencies.
- Add golden hashes for representative progress and rotation traces.

Exit: mass is conserved, wall contacts hold, sand follows world gravity at every glass angle, and
fixed orientation traces reproduce fixed frames.

### 4. Target integration

- Add timestamped `KEY` handling, 90-degree rotation commands, a fixed-step Embassy physics cadence,
  and an independent render cadence in the sole LVGL-owning task.
- Add the hourglass widget, partial invalidation, runtime power-intent transitions, fixed
  high-contrast panel drive policy, and completion cue.

Exit: Home opens the compiled launcher, the launcher enters Hourglass, and Back returns through the
shell-owned surface stack. The fixed V1 duration is 15 minutes; a duration picker is deliberately
deferred.

### 5. Device validation

- Measure linked S3 `.data`, `.bss`, stack remainder, and task-pool changes using the accounting
  discipline in the [DRAM budget](../reference/dram/dram-budget.md).
- Measure physics and render runtime, missed frame deadlines, panel flush payload, and cue latency
  from the target's bounded `HOURGLASS_METRICS` output.
- Validate the upright duration calibration, 90-degree endpoints, horizontal zero-flow pause, and
  intermediate-angle flow without angle-specific tuning.
- Run a complete default session plus repeated `KEY` turns and horizontal holds. Arbitrary command
  traces, render-stall independence, reverse turns, and pause/resume remain host-model checks until a
  later input adapter exposes those commands on-device. The originally proposed one-hour repeated
  session soak remains ongoing hardware-matrix regression coverage rather than a V1 closure gate; two
  complete 15-minute physical sessions and the final run's retained metrics provide the bounded V1
  acceptance evidence.
- Record the resulting target coverage in the
  [hardware test matrix](../reference/hardware-test-matrix.md).

Exit: completion is crossing-derived, upright duration and cue latency have accepted bounds,
rendering meets its cadence, and memory headroom remains measured and explicit.

## Completion criteria

- The target resolves the compiled catalogue entry through Home and the launcher, enters Hourglass,
  and returns through shell navigation.
- Commanded angle, crossing ledger, and remaining time are independent of render cadence.
- Completion changes state and emits its visual cue exactly once per completed flow.
- Each V1 `KEY` press advances exactly 90 degrees through a continuous, gravity-reactive turn.
- Stable 90- and 270-degree orientations have zero throat flow and pause time even while the bulk
  sand continues settling; returning to a flowing angle resumes it.
- The glass tracks and renders arbitrary angle without directional snapping.
- Grains respond continuously to gravity, inertia, moving-wall contact, tilt, and reversal.
- Angle affects flow through rotating geometry, local gravity, particle dynamics, and the production
  packet limiter's shared vertical-gravity factor; there is no separately tuned angle curve.
- Arbitrary mid-session rotations conserve mass and produce an observed-flow conditional ETA.
- The granular model is deterministic for a fixed seed and orientation trace.
- Hourglass requests `HighPerformance` only while `Running`, requests `Normal` while Ready, Paused,
  or Complete, and never requests Sleep or Deep Sleep. The ST7305 stays in its measured
  high-contrast low-power drive mode for every app state.
- Host coverage, target build, source reachability, code-size checks, and documentation checks pass.
