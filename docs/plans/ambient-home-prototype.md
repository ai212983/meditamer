# Ambient Home Prototype Description

- Status: Proposed
- Last-reviewed: 2026-08-19

## Purpose

Define the user-visible behaviour of the first Ambient Home prototype. A later code plan will derive
the implementation.

Ambient Home presents daytime as an arc with a circle that moves from left to right in coarse steps
aligned with local time.

## Surface

The surface contains:

- a white background;
- the complete arc; and
- the time-positioned circle.

The arc is black. The circle has a white fill and black outline and appears above the arc. Together,
they communicate the day's progress at a glance. Nothing else competes with them: the Back button
belongs to the time display, which a tap reveals.

Activating Back returns to the preceding screen. When Ambient Home is already the navigation root,
it remains active.

## Configuration

All dimensions are proportions of the available surface so the same composition adapts across
supported displays.

| Field | Default |
| --- | --- |
| Arc start | Circle centre 8% of the surface width beyond the left edge and 73% down the surface |
| First curvature control | 17.5% across and 12.5% down the surface |
| Second curvature control | 82.5% across and 12.5% down the surface |
| Arc end | Circle centre 8% of the surface width beyond the right edge and 73% down the surface |
| Circle radius | 6% of the surface's shorter dimension |
| Start time | `08:00` local time |
| End time | `20:00` local time |
| Update period | `5 minutes` |
| Tap to show time | Enabled |

The default curve is a symmetric arch. Its endpoints place the complete circle outside the visible
surface at the start and end of the configured day.

Configuration is applied as one complete set. Any invalid value selects the complete defaults. The
start time precedes the end time, the radius and update period are positive, and the update period
fits within the configured time span.

## Time behaviour

At each update, the elapsed fraction of the configured time span determines the same fraction of the
circle's journey from the arc start to the arc end:

- before the start time, the circle rests at the arc start;
- at the start time, movement begins from the arc start;
- during the configured period, the latest update boundary determines its position; and
- at and after the end time, the circle rests at the arc end.

Update boundaries are anchored at the start time. With the defaults, `14:04:59` shows the `14:00`
position, while `14:05:00` shows the `14:05` position. The end time is an additional update boundary
when the regular cadence ends earlier.

The circle moves directly to each new position, producing a sequence of calm, discrete changes. A
clock adjustment moves it to the position for the newly reported time. Each new local day begins at
the pre-start position.

When local time is available, entering Ambient Home immediately shows the current position. When
local time is unavailable, the surface shows the arc alone; the circle appears at the current
position once time becomes available.

## Time-on-tap option

When enabled, a surface tap replaces the arc and circle with the current local time, centred in
24-hour `HH:MM` format and large enough to read across a room. The Back button appears with it,
centred near the bottom edge, and stays active for as long as the time is shown. After 10 seconds,
the arc and circle return at the position for the then-current time, and the Back button leaves with
the time.

When the option is disabled the time display is unreachable, so the Back button stays on the ambient
view instead; the surface is never left without a way back.

A further tap on the time display refreshes the shown time and restarts the 10-second period. A tap
with unavailable local time retains the ambient view. When the option is disabled, surface taps
retain the ambient view.

## Rendering

Every presentation of the arc and circle uses a full-screen update. This includes surface entry,
scheduled circle movement, clock adjustment, a new local day, configuration changes, time becoming
available, and return from the time display.

## Acceptance

The prototype is complete when:

- the default surface shows the specified symmetric arc and circle, and no Back button;
- each configuration value changes its defined aspect of the experience;
- the circle follows the configured start, update, and end behaviour;
- every arc-and-circle render uses a full-screen update and leaves one clean circle at the current
  position;
- unavailable time transitions cleanly to the current position when time becomes available;
- an eligible tap shows the current time, at a size legible across a room, together with the Back
  button for 10 seconds, and then returns to the current arc position;
- repeated taps restart the time-display period;
- Back returns to the preceding screen, and is reachable whether or not tap-to-show-time is
  enabled; and
- the experience remains legible and calm across supported display sizes.

Any change to user-visible behaviour updates this description before the code plan is derived.
