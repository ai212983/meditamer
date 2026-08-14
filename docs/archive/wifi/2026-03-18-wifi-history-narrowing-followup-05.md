# 2026-03-18 Wi-Fi History-Narrowing Follow-Up 05

## Scope

This follow-up records the next consumer check after the
`scan_build_chan_list` causality test.

Primary predecessors:

- `docs/development/2026-03-18-wifi-history-narrowing-followup-04.md`
- `docs/development/2026-03-18-wifi-history-narrowing-followup-03.md`
- `docs/development/2026-03-18-wifi-history-narrowing-followup-02.md`

## Experiment

Instrument the `scan_start` wrapper itself and capture the first 16 bytes of the
RAM buffer passed as `arg3`, both before and after the call.

Artifacts:

- app:
  - `logs/flash_capture_20260318_scan_start_arg3_app/capture.log`
- comparator:
  - `logs/flash_capture_20260318_scan_start_arg3_comparator/capture.log`

## Result

The `scan_start arg3` buffer is not the next discriminator.

At the explicit-scan boundary, both images pass the same top-level `scan_start`
flags:

- `arg0=0x10f`
- `arg1=0x03`
- `arg2=0x00`

The remaining visible difference is the RAM pointer value in `arg3`, but the
first 16 bytes at that pointer are structurally the same on both sides.

App:

- `scan_start ... arg3=0x3ffc91a8`
- `pre=00:00:00:00:a8:91:fc:3f:00:00:00:00:00:00:00:00`
- `post=00:00:00:00:a8:91:fc:3f:00:00:00:00:00:00:00:00`

Comparator:

- `scan_start ... arg3=0x3ffc5390`
- `pre=00:00:00:00:90:53:fc:3f:00:00:00:00:00:00:00:00`
- `post=00:00:00:00:90:53:fc:3f:00:00:00:00:00:00:00:00`

So both buffers show the same visible shape:

- zero first word
- second word points back to the buffer itself
- remaining visible words zero
- unchanged across the call

## Meaning

This closes another low-perturbation runtime seam:

- not the `scan_build_chan_list` working buffer alone
- not `app_scan_params`
- not the first visible 16 bytes of the `scan_start arg3` buffer

The active target is now deeper than the observable `scan_start` inputs.

## Best Next Step

If investigation continues, the next useful step is more invasive:

1. probe the local continuation below `scan_start`
2. or instrument the first timer/channel progression callback it schedules
3. or patch/breakpoint the internal path that should advance from accepted
   `scan_start` into beacon parsing/materialization

More outer snapshots at the same seam are unlikely to yield new information.
