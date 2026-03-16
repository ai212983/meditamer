# Wi-Fi Legacy Old-Stack Implementation Plan

## Goal

Implement a parallel old-stack Wi-Fi backend path behind `backend_legacy_port`
to restore stable Wi-Fi discovery without changing runtime/bootstrap again.

This document now covers two completed conclusions:

1. the Rust-side parallel old-stack import is complete
2. the next work must target old-vs-new blob/internal compatibility directly

## Current Validated Boundary

- `backend_legacy_port` is the active backend in firmware validation
- runtime init completes
- `legacy_port_wifi_init stage=done`
- `start=ok`
- `WIFI_MAC` interrupts rise during scan activity
- pre-scan promisc stays zero on channels `8/1/6/11`
- `wifi_rx_cb_count sta=0 ap=0`
- raw `ScanDone` state stays empty
- direct explicit scan returns `ap_num=0`
- wrapped scan ends with `InternalError(Timeout)`
- scan-time queue/semaphore/thread-semaphore counters stay zero

Interpretation:

- Rust-side legacy runtime/bootstrap ownership is no longer the main blocker
- Rust-side old-stack init/control/RX ownership is no longer the main blocker
- the remaining mismatch is below the Rust Wi-Fi seams and above packet
  admission, in the old-vs-new blob/internal compatibility layer

## Phase Checklist

- [x] Phase 1: establish `wifi/legacy_stack/` module tree
- [x] Phase 2: move old install/init ownership into `legacy_stack`
- [x] Phase 3: move old start/stop/scan ownership into `legacy_stack`
- [x] Phase 4: move old RX delivery ownership into `legacy_stack`
- [x] Phase 5: cut `backend_legacy_port` over to the parallel old stack
- [x] Phase 6: decide whether the Rust-side old-stack import was sufficient
- [x] Phase 7: map old-vs-new blob/internal compatibility surface
- [x] Phase 8: isolate the minimum old internal/blob contract required for scan
      admission
- [x] Phase 9: implement the first direct blob/internal compatibility slice
- [x] Phase 10: validate whether discovery metrics move
- [x] Phase 11: decide between deeper blob-facing import vs stop

## Steps

### Phase 1

- [x] Step 1.1 create `vendor/esp-radio-0.17.0/src/wifi/legacy_stack/`
- [x] Step 1.2 add `install.rs`, `init.rs`, `control.rs`, `rx.rs`, `mod.rs`
- [x] Step 1.3 mark this tree as the authoritative implementation target for the
      next wholesale slice

Notes:
- commit: `5a08b55`
- validation: tree creation only; compile validation deferred to Phase 2
- outcome: parallel old-stack module tree exists and is now the authoritative
  destination for the next wholesale legacy import

### Phase 2

- [x] Step 2.1 move old OSI/global table ownership into `legacy_stack/install.rs`
- [x] Step 2.2 move old `wifi_new` / `wifi_init` into `legacy_stack/init.rs`
- [x] Step 2.3 route only `backend_legacy_port` init entrypoints to the new tree

Notes:
- commit: `bb9d27e`
- validation: build-only, `CARGO_FEATURES=wifi-debug-slim-app scripts/build/build.sh debug`
- outcome: `backend_legacy_port` init ownership now routes through
  `legacy_stack/install.rs` and `legacy_stack/init.rs` while the current backend
  remains unchanged

### Phase 3

- [x] Step 3.1 move old start/stop/scan into `legacy_stack/control.rs`
- [x] Step 3.2 keep broad blocking scan and result retrieval literal to the old
      stack
- [x] Step 3.3 remove active `backend_legacy_port` dependence on the stitched
      control/admission path

Notes:
- commit: `bb9d27e`
- validation:
  `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/logs/hostctl_flashcapture_backend_legacy_port_20260316_phase3_control_cutover/capture.log`
- outcome: the old-stack control path is active for `backend_legacy_port`, but
  discovery metrics remain unchanged:
  `scan_rc=12300`, `ap_num=0`, wrapped scan `InternalError(Timeout)`,
  `wifi_rx_cb_count sta=0 ap=0`, `scan_done_eventpost=0`

### Phase 4

- [x] Step 4.1 move old RX queue/packet/callback/device behavior into
      `legacy_stack/rx.rs`
- [x] Step 4.2 make active legacy RX behavior come only from that module
- [x] Step 4.3 keep type adaptation at the boundary only

Notes:
- commit: `5a08b55`
- validation:
  `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/logs/hostctl_flashcapture_backend_legacy_port_20260316_phase4_rx_cutover/capture.log`
- outcome: active RX queue, token, callback, and packet-buffer ownership moved
  into `legacy_stack/rx.rs`; discovery metrics remained unchanged with
  `wifi_rx_cb_count sta=0 ap=0` and `scan_done_eventpost=0`

### Phase 5

- [x] Step 5.1 make `backend_legacy_port` use the parallel old stack for
      `wifi_new`, start/stop/scan, and RX callbacks/tokens
- [x] Step 5.2 keep runtime/bootstrap unchanged
- [x] Step 5.3 keep shim modules only as compile support until validation lands

Notes:
- commit: `5a08b55`
- validation:
  `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/logs/hostctl_flashcapture_backend_legacy_port_20260316_phase4_rx_cutover/capture.log`
- outcome: `backend_legacy_port` now uses the parallel old-stack path for
  init, start/stop/scan, and RX callbacks/tokens while runtime/bootstrap stayed
  unchanged; success criteria were not met because all discovery metrics
  remained flat

### Phase 6

- [ ] Step 6.1 if metrics improve, branch to stabilization follow-up work
- [x] Step 6.2 if metrics remain unchanged, record that the Rust-side parallel
      import did not move the boundary
- [x] Step 6.3 stop Rust-side wrapper/table/facade refactors after that point

Notes:
- commit: pending
- validation:
  `/Users/dimitri/Documents/Code/personal/Inkplate/meditamer/logs/hostctl_flashcapture_backend_legacy_port_20260316_phase4_rx_cutover/capture.log`
- outcome: the parallel old-stack Rust-side import did not move the validated
  boundary; the next phase must target old-vs-new blob/internal compatibility
  directly

## Next-Phase Scope

The next chunk is no longer a Rust-side old-stack import. It is a direct
blob/internal compatibility phase.

Detailed execution plan:
- [Wi-Fi Blob Compatibility Phase Plan](./wifi-legacy-old-stack-blob-compatibility-plan.md)

Rules that still apply here:

- do not change runtime/bootstrap
- do not add generic diagnostics or A/B knobs
- do not add more Rust-side wrapper/facade/table refactors
- every runtime-affecting step must end with canonical full-flash validation

Success threshold for the next chunk:

- any of these becomes non-zero in canonical validation:
  - pre-scan promisc totals
  - `wifi_rx_cb_count`
  - `scan_done_eventpost`
  - direct explicit scan `ap_num`
  - wrapped scan AP count

If all stay unchanged, stop and record that the remaining gap is below the
current vendorable Rust-side boundary.

## Stop Conditions

Stop the next chunk immediately if:

- canonical full-flash validation does not run
- the active backend is not `backend-legacy-port`
- a proposed change is only another Rust-side wrapper/table/facade refactor
- the direct blob/internal compatibility slice validates cleanly but leaves all
  discovery metrics unchanged

## Next Pending Step

Start a new implementation chunk only if we are willing to pair
`backend_legacy_port` more directly with old `esp-wifi-sys 0.7.1` internal
expectations instead of continuing compatibility extraction against the current
blob generation.

The detailed closure and next-step recommendation are now recorded in
[Wi-Fi Blob Compatibility Phase Plan](./wifi-legacy-old-stack-blob-compatibility-plan.md).
