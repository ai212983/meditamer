# Wi-Fi Legacy Old-Stack Implementation Plan

## Goal

Implement a parallel old-stack Wi-Fi backend path behind `backend_legacy_port`
 to restore stable Wi-Fi discovery without changing runtime/bootstrap again.

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

## Phase Checklist

- [x] Phase 1: establish `wifi/legacy_stack/` module tree
- [ ] Phase 2: move old install/init ownership into `legacy_stack`
- [ ] Phase 3: move old start/stop/scan ownership into `legacy_stack`
- [ ] Phase 4: move old RX delivery ownership into `legacy_stack`
- [ ] Phase 5: cut `backend_legacy_port` over to the parallel old stack
- [ ] Phase 6: decide whether the Rust-side old-stack import was sufficient

## Steps

### Phase 1

- [x] Step 1.1 create `vendor/esp-radio-0.17.0/src/wifi/legacy_stack/`
- [x] Step 1.2 add `install.rs`, `init.rs`, `control.rs`, `rx.rs`, `mod.rs`
- [x] Step 1.3 mark this tree as the authoritative implementation target for the
      next wholesale slice

Notes:
- commit: pending
- validation: tree creation only; compile validation deferred to Phase 2
- outcome: parallel old-stack module tree exists and is now the authoritative
  destination for the next wholesale legacy import

### Phase 2

- [ ] Step 2.1 move old OSI/global table ownership into `legacy_stack/install.rs`
- [ ] Step 2.2 move old `wifi_new` / `wifi_init` into `legacy_stack/init.rs`
- [ ] Step 2.3 route only `backend_legacy_port` init entrypoints to the new tree

Notes:
- commit:
- validation:
- outcome:

### Phase 3

- [ ] Step 3.1 move old start/stop/scan into `legacy_stack/control.rs`
- [ ] Step 3.2 keep broad blocking scan and result retrieval literal to the old
      stack
- [ ] Step 3.3 remove active `backend_legacy_port` dependence on the stitched
      control/admission path

Notes:
- commit:
- validation:
- outcome:

### Phase 4

- [ ] Step 4.1 move old RX queue/packet/callback/device behavior into
      `legacy_stack/rx.rs`
- [ ] Step 4.2 make active legacy RX behavior come only from that module
- [ ] Step 4.3 keep type adaptation at the boundary only

Notes:
- commit:
- validation:
- outcome:

### Phase 5

- [ ] Step 5.1 make `backend_legacy_port` use the parallel old stack for
      `wifi_new`, start/stop/scan, and RX callbacks/tokens
- [ ] Step 5.2 keep runtime/bootstrap unchanged
- [ ] Step 5.3 keep shim modules only as compile support until validation lands

Notes:
- commit:
- validation:
- outcome:

### Phase 6

- [ ] Step 6.1 if metrics improve, branch to stabilization follow-up work
- [ ] Step 6.2 if metrics remain unchanged, record that the Rust-side parallel
      import did not move the boundary
- [ ] Step 6.3 stop Rust-side wrapper/table/facade refactors after that point

Notes:
- commit:
- validation:
- outcome:

## Stop Conditions

Stop this chunk immediately if:

- canonical full-flash validation does not run
- the active backend is not `backend-legacy-port`
- the parallel old-stack cutover compiles but leaves all discovery metrics
  unchanged

## Next Pending Step

Phase 2, Step 2.1: move old OSI/global table ownership into
`wifi/legacy_stack/install.rs`.
