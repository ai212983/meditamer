# 2026-03-17 Wi-Fi March 5 vs March 6 Regression Investigation Plan

## Goal
Identify the smallest March 5 -> March 6 change set that flips main-app Wi-Fi discovery from reproducibly green to reproducibly dark on current hardware.

## Current Facts
- `meditamer_march_5` builds, flashes, and passes bounded discovery today.
- `meditamer_march_6` builds, flashes, and fails bounded discovery today.
- Both snapshots use the same core Wi-Fi/runtime crate generation:
  - `esp-hal 1.0.0`
  - `esp-rtos 0.2.0`
  - `esp-radio 0.17.0`
  - `esp-wifi-sys 0.8.1`
- `.env.local`, `sdkconfig.defaults`, and `config/` do not explain the regression.
- The strongest current suspect is app-side Wi-Fi control-flow added in March 6, especially pre-connect zero-discovery handling in `src/firmware/storage/upload/wifi/connect/prepare/prepare_scan.rs`.

## `prepare_scan.rs` Version Breakdown
### March 5 baseline

`src/firmware/storage/upload/wifi/connect/prepare/prepare_scan.rs` in the
March 5 snapshot is simple:

- run `scan_target_candidates(...)`
- handle `hit_nomem`
- if candidates exist, pick one and continue
- if no candidates exist, fall through and return `false`

It does not add:

- a zero-discovery recovery state machine
- scan-entry diagnostic hooks before the real scan
- post-scan hard-recover bookkeeping

### Original March 6 file

The March 6 file adds five meaningful behaviors over March 5:

1. `handle_preconnect_zero_discovery(...)`
   - new async recovery path for zero-result scans
   - clears hints/candidates/config state
   - escalates into retry, driver restart, and terminal fail
2. scan-entry hooks before the real scan
   - `maybe_run_scan_entry_idf_compare_diag(ssid)`
   - `maybe_run_scan_entry_promisc_diag().await`
   - `maybe_log_scan_entry_driver_state()`
   - plus the large `scan_entry_readiness ...` log
3. post-scan bookkeeping
   - `state.note_hard_recover_scan_completion(...)`
4. changed `scan_nomem` watchdog start
   - switches from direct timestamp assignment to
     `state.start_hard_recover_watchdog("scan_nomem")`
5. changed zero-result handling
   - zero-result scans no longer just fall through
   - they enter `handle_preconnect_zero_discovery(...)` and return early

### Current narrowed March 6 working tree

The currently narrowed March 6 file in reduction keeps:

- `handle_preconnect_zero_discovery(...)`
- the `scan_entry_readiness ...` log
- `state.note_hard_recover_scan_completion(...)`

It currently removes again:

- `maybe_run_scan_entry_idf_compare_diag(ssid)`
- `maybe_run_scan_entry_promisc_diag().await`
- `maybe_log_scan_entry_driver_state()`

It also keeps the March 5-style `scan_nomem` watchdog assignment instead of the
full March 6 helper call.

## Validation Rule
Use the same bounded capture path after each runtime-affecting step:
- build the `meditamer_march_6` snapshot
- flash it onto the currently attached device
- run `scripts/tests/hw/test_wifi_discovery_debug.sh`
- record whether discovery is green or dark

A step only counts as successful if the runtime outcome is captured in a log.

## Success Criteria
The investigation succeeds when one of the following happens:
- reverting one bounded slice restores non-zero discovery on March 6, or
- a minimal set of March 6 changes is isolated as required to trigger zero-discovery.

## Stop Conditions
Stop this plan if any of the following becomes true:
- reverting the entire March 6 app-side Wi-Fi control-flow layer still does not restore discovery
- the reduction requires changing crate versions or vendored runtime crates
- validation becomes inconsistent across repeated runs on the same device

## Phase 1: Freeze the Repro Boundary
### Step 1.1
Record the known-good March 5 capture artifact.

### Step 1.2
Record the known-bad March 6 capture artifact.

### Step 1.3
Use those two artifacts as the only baseline for this reduction plan.

## Phase 2: Reduce March 6 by Highest-Value App-Side Wi-Fi Slice
### Step 2.1
Revert March 6 `prepare_scan.rs` to the March 5 version only.

Rationale:
- this file introduces the most important new non-gated zero-discovery control-flow
- it directly matches the observed March 6 runtime symptoms

### Step 2.2
Build, flash, and run bounded discovery capture.

### Step 2.3
Classify the result:
- if discovery becomes green, stop and record `prepare_scan.rs` as the first bad slice
- if discovery remains dark, keep the revert and continue to Phase 3

Phase 2 status on 2026-03-17:
- initial `prepare_scan.rs` revert produced a green run
- bounded replay of every original March 6 `prepare_scan.rs` hunk also stayed green
- `prepare_scan.rs` is byte-for-byte restored to the original March 6 file and no longer explains the regression by itself
- Phase 3 became the next reduction step

Phase 2 update later on 2026-03-17 with the pinned dark device:
- full `prepare_scan.rs` revert is green again
- `zero-discovery branch` alone is green
- `bookkeeping/watchdog` alone is green
- `scan-entry helper calls` alone are green
- `promisc_diag + note_hard_recover_scan_completion` is dark under a one-round gate
- finer-grained sub-file results below full-file revert are not yet stable enough
  to call final root cause

## Phase 3: Reduce March 6 Start/Recovery Behavior
### Step 3.1
Revert `src/firmware/storage/upload/wifi/connect/prepare/prepare_start.rs` to the March 5 version.

### Step 3.2
Build, flash, and run bounded discovery capture.

### Step 3.3
Classify the result:
- if green, record `prepare_start.rs` as required to trigger the regression
- if dark, keep the revert and continue

Phase 3 status on 2026-03-17:
- reverting `prepare_start.rs` also stayed green
- the original March 6 snapshot had already turned green again before the Phase 3 reduction
- therefore the March 6 zero-discovery baseline is no longer reproducible under the current workflow and device state
- further reduction is blocked until the dark March 6 baseline is made reproducible again

## Phase 4: Reduce March 6 Connected-State Recovery Changes
### Step 4.1
Revert these files to March 5 versions:
- `src/firmware/storage/upload/wifi/connect/success.rs`
- `src/firmware/storage/upload/wifi/connect/success/success_progress.rs`
- `src/firmware/storage/upload/wifi/connect/recovery.rs`
- `src/firmware/storage/upload/wifi/connect/error/error_recovery/discovery.rs`

### Step 4.2
Build, flash, and run bounded discovery capture.

### Step 4.3
Classify the result:
- if green, record the minimal required subset by replaying these files one at a time
- if dark, continue

## Phase 5: Remove March 6 Direct Wi-Fi Diagnostic/Raw-IDF Hooks
### Step 5.1
Revert these files or modules to March 5 state:
- `src/firmware/storage/upload/wifi/driver.rs`
- `src/firmware/storage/upload/wifi/scan.rs`
- `src/firmware/storage/upload/wifi/connect/mod.rs`
- `src/firmware/storage/upload/wifi/connect/driver_state.rs`
- `src/firmware/storage/upload/wifi/connect/idf_scan_compare.rs`
- `src/firmware/storage/upload/wifi/connect/promisc_diag.rs`

### Step 5.2
Remove the direct `esp-wifi-sys` dependency from `Cargo.toml` only if it is no longer needed after the source rollback.

### Step 5.3
Build, flash, and run bounded discovery capture.

### Step 5.4
Classify the result:
- if green, replay the reverted March 6 files back one bounded slice at a time until the first bad slice is found
- if still dark, continue to Phase 6

## Phase 6: Confirm Whether the Regression Is Fully in App Wi-Fi Logic
### Step 6.1
Compare the reduced March 6 tree against March 5 again, limited to:
- `src/firmware/storage/upload/wifi/`
- `scripts/lib/run_hostctl.sh`
- `tools/hostctl/src/workflows_wifi_*`

### Step 6.2
If the firmware is still dark after all app-side Wi-Fi reductions, record that the remaining boundary is outside the reduced app-side Wi-Fi flow.

### Step 6.3
Do not jump to crate-version or vendor-crate changes until this app-side reduction is exhausted.

## Repro Gate
Before continuing with any more March 5 -> March 6 file reductions:
- flash the pristine March 6 snapshot
- rerun bounded discovery capture
- require the dark March 6 baseline to reproduce again

If March 6 stays green, stop file-level reduction and investigate what changed in the validation environment or runtime preconditions instead.

## Execution Notes
- Prefer repo-local snapshot worktrees over editing the main current repo.
- Keep each reduction slice in its own commit.
- Record the log path after every runtime-affecting step.
- Do not mix multiple reductions into one validation step.

## Next Step
Re-establish the dark March 6 baseline:
- flash the pristine `meditamer_march_6` snapshot
- rerun bounded discovery capture
- only continue file-level reduction if March 6 is dark again
