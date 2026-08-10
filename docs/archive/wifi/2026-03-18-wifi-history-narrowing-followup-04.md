# 2026-03-18 Wi-Fi History-Narrowing Follow-Up 04

## Scope

This follow-up records the first direct causality test of the
`scan_build_chan_list` working-buffer hypothesis.

Primary predecessors:

- `docs/development/2026-03-18-wifi-history-narrowing-followup-03.md`
- `docs/development/2026-03-18-wifi-history-narrowing-followup-02.md`

## Experiment

A diagnostics-only app build rewrote only `scan_build_chan_list arg2` when the
baseline pointer pattern matched the failing app path:

- if original `arg2 == sta_ptr`
- replace the actual call argument with `chm_ptr + 0x50`
- leave all other helper arguments untouched

Artifact:

- app, targeted arg rewrite:
  - `logs/flash_capture_20260318_helper_wrap_app_force_chm_arg2/capture.log`

## What Happened

The rewrite did take effect.

At both `idf_explicit_compare_postcall` and `idf_explicit_compare_prefirst`:

- original app helper args
  - `arg2=0x3ffbe8cc`
  - `arg3=0x3ffc966c`
- actual helper call arg
  - `call_arg2=0x3ffc9170`
- current pointer identities
  - `sta_ptr=0x3ffbe8cc`
  - `chm_ptr=0x3ffc9120`
  - `g_wifi_nvs_ptr=0x3ffc966c`

So the experiment successfully changed the helper call from:

- `arg2 == sta_ptr`

to:

- `call_arg2 == chm_ptr + 0x50`

The rewritten working buffer also matched the comparator-style bytes:

- `pre_arg2=01:00:01:00:01:00:6c:09`
- `post_arg2=01:00:01:00:01:00:6c:09`

## Outcome

Despite the successful rewrite, the app stayed in the same failure family:

- `idf_explicit_compare=ok`
- `scan_rc=0`
- `scannum=0x0000`
- `head_ptr=0x0`
- `ap_num=0`
- `records_returned=0`

The result list remained empty before retrieval and after retrieval.

## Meaning

This is a strong negative causality result.

The `scan_build_chan_list arg2` working-buffer mismatch is real, and the app can
be forced onto the comparator-style buffer at that exact helper boundary, but
that change alone is not sufficient to restore AP result materialization.

That narrows the target again:

- the failing discriminator is not just the `scan_build_chan_list` working
  buffer origin
- the decisive split is either:
  - later in channel-plan consumption after `scan_build_chan_list`, or
  - in another coupled field/state transition that must change together with the
    buffer origin

## Closed Branch

The following branch is now closed as a sufficient standalone cause:

- `scan_build_chan_list arg2 == sta_ptr` instead of `chm_ptr + 0x50`

It remains a real correlated difference, but not a sufficient fix by itself.

## Best Next Step

Highest-value next step:

1. compare the next consumer after `scan_build_chan_list` on app vs comparator
2. focus on the state that survives this helper and is later consumed by
   `scan_start` / channel progression
3. specifically inspect whether the app still diverges on:
   - the channel bitmap/list contents after the helper
   - the final scan flags passed into `scan_start`
   - timer/channel-step state that uses the built list

## Stop Conditions

Stop this line and regroup if a next probe:

1. only re-observes the rewritten helper bytes without checking a later consumer
2. reopens already closed outer scan-start seams
3. introduces another invasive wrapper that changes control flow rather than
   observing or minimally rewriting one field
