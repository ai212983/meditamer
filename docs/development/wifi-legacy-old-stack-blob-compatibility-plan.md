# Wi-Fi Blob Compatibility Phase Plan

## Goal

Proceed past the completed Rust-side old-stack import and target the remaining
old-vs-new blob/internal compatibility boundary directly.

## Current Boundary

Canonical validation already proves all of the following on
`backend_legacy_port`:

- runtime init completes
- `legacy_port_wifi_init stage=done`
- `start=ok`
- `WIFI_MAC` interrupts rise during scan activity
- pre-scan promisc stays zero
- `wifi_rx_cb_count sta=0 ap=0`
- `scan_done_eventpost=0`
- direct explicit scan returns `ap_num=0`
- wrapped scan returns `InternalError(Timeout)`
- queue/semaphore/thread-semaphore counters stay zero

Interpretation:

- Rust-side runtime/bootstrap is not the remaining blocker
- Rust-side old-stack init/control/RX ownership is not the remaining blocker
- the remaining mismatch is in the internal/blob-facing compatibility layer

## Constraints

- Do not change runtime/bootstrap.
- Do not add generic diagnostics or A/B knobs.
- Do not add more Rust-side wrapper/facade/table refactors.
- Prefer one cohesive direct compatibility slice over many new shims.
- Every runtime-affecting step must end with canonical full-flash validation.

## Success Criteria

Treat the next chunk as successful only if one or more of these becomes
non-zero in canonical validation:

- pre-scan promisc totals
- `wifi_rx_cb_count`
- `scan_done_eventpost`
- direct explicit scan `ap_num`
- wrapped scan AP count

If all of them remain unchanged, stop and record that the remaining gap is
below the current vendorable Rust-side boundary.

## Phase Checklist

- [x] Phase 7: map old-vs-new blob/internal compatibility surface
- [x] Phase 8: isolate the minimum old internal/blob contract required for scan
      admission
- [x] Phase 9: implement the first direct blob/internal compatibility slice
- [x] Phase 10: validate whether discovery metrics move
- [x] Phase 11: decide between deeper blob-facing import vs stop

## Phase 7: Compatibility Map

### Goal

Produce a concrete map of the old-vs-new internal interfaces that still matter
for discovery, using source comparison instead of new runtime experiments.

### Steps

- [x] Step 7.1 compare old working `esp-wifi 0.15.1` /
      `esp-wifi-sys 0.7.1` scan-start and RX-admission-adjacent internal paths
      against the current vendored active path
- [x] Step 7.2 enumerate the exact symbols, globals, and internal state
      contracts that differ and could plausibly affect admission
- [x] Step 7.3 reduce that list to the minimum compatibility slice required for
      the first direct import attempt

### Required Artifacts

- one compatibility map note added to this file
- references to the exact implementation files selected for the first direct
  compatibility slice

### Notes

- commit:
- validation: source comparison only
- outcome:
  - compared old working Rust-side install/init path in
    `$CARGO_HOME/registry/src/<registry>/esp-wifi-0.15.1/src/wifi/internal.rs`
    and
    `$CARGO_HOME/registry/src/<registry>/esp-wifi-0.15.1/src/wifi/mod.rs`
    against the current active vendored path in
    `vendor/esp-radio-0.17.0/src/wifi/internal.rs`
    and
    `vendor/esp-radio-0.17.0/src/wifi/legacy_stack/init.rs`
  - confirmed the public `esp_wifi.h` scan/start/init contract is effectively
    unchanged between `esp-wifi-sys 0.7.1` and `esp-wifi-sys 0.8.1`
  - confirmed the internal install/global-table contract is not unchanged:
    current `internal.rs` carries extra coex/debug/xtal callbacks, different
    timer/allocator wrappers, renamed exported globals, and different `G_CONFIG`
    ownership semantics
  - conclusion: the next direct compatibility slice should target the old
    internal install/global-state contract at the blob boundary, not public scan
    wrappers

## Phase 8: Minimum Contract Selection

### Goal

Define the first implementation slice that changes blob/internal expectations
directly without reopening Rust-side runtime churn.

### Candidate Targets

Choose one cohesive unit, not multiple scattered shims. Preferred order:

1. scan-start / admission-adjacent internal state and callback contract
2. RX-admission / delivery-entry contract before `recv_cb_sta/ap`
3. internal event/queue state that must become non-zero for scan completion

### Steps

- [x] Step 8.1 choose one minimum compatibility unit
- [x] Step 8.2 document why that unit is the best next target
- [x] Step 8.3 identify exactly which current files will be replaced or bypassed

### Notes

- commit:
- validation: design closure only
- outcome:
  - selected minimum unit: direct old internal install/global-state contract
    compatible with `esp-wifi-sys 0.7.1`
  - this is the best next target because:
    - the public scan/start headers did not materially change
    - the Rust-side old-stack init/control/RX path is already active
    - the remaining divergence appears in internal global/table ownership and
      blob-facing compatibility expectations
  - first files/substrates to replace or bypass:
    - current active dependency on
      `vendor/esp-radio-0.17.0/src/wifi/internal.rs`
      for blob-facing install/global assumptions
    - current `esp-wifi-sys 0.8.1` internal install/global expectations where
      they differ from the old `0.7.1` working stack
  - first implementation shape:
    - parallel old internal install/global-state module
    - likely paired with vendored old `esp-wifi-sys 0.7.1` substrate or a
      minimal extracted compatibility subset from it

## Phase 9: First Direct Compatibility Slice

### Goal

Import or reproduce the selected old internal/blob-facing behavior as directly
as the current crate graph allows.

### Allowed Work

- importing old internal logic or state layout into a dedicated module
- changing `backend_legacy_port` to bind to that direct compatibility module
- adapting types only at the boundary to make the old logic compile

### Disallowed Work

- new diag knobs
- runtime/bootstrap rewrites
- scheduler/task model edits
- outer scan wrapper redesign

### Steps

- [x] Step 9.1 add the dedicated compatibility module for the chosen slice
- [x] Step 9.2 route only `backend_legacy_port` through it
- [x] Step 9.3 keep current shim-based paths only as compile support until
      validation lands

### Notes

- commit:
- validation:
  `logs/hostctl_flashcapture_backend_legacy_port_20260316_092802_blob_compat_install/capture.log`
- outcome:
  - added a dedicated direct-compatibility module at
    `vendor/esp-radio-0.17.0/src/wifi/blob_compat_internal.rs`
    as a closer import of the old `esp-wifi 0.15.1` internal install/global
    state contract
  - routed only `backend_legacy_port` init/coex selection through that module
    from:
    `vendor/esp-radio-0.17.0/src/wifi/legacy_stack/init.rs`
    and
    `vendor/esp-radio-0.17.0/src/wifi/mod.rs`
  - the slice built successfully, but canonical validation regressed the
    boundary to:
    `legacy_port_wifi_init stage=esp_wifi_init_internal.before`
  - `runtime_init result=ok` and `legacy_port_wifi_init stage=done` were no
    longer reached
  - interpretation: the old internal install/global-state contract is not
    compatible with the current blob generation as a direct import

## Phase 10: Canonical Validation

### Goal

Determine whether the direct blob/internal compatibility slice moved discovery.

### Validation Method

Use only:

- canonical full-flash `hostctl flash-capture`
- slim app build
- `backend_legacy_port` diagnostics

### Required Fields To Record

- backend name
- `runtime_init result`
- `legacy_port_wifi_init stage=done`
- `start=ok`
- pre-scan promisc totals
- direct null scan result
- direct explicit scan AP count
- wrapped scan result
- `wifi_mac_isr_count`
- `wifi_rx_cb_count`
- `scan_done_eventpost`
- queue/semaphore/thread-semaphore counters

### Steps

- [x] Step 10.1 run canonical validation
- [x] Step 10.2 record whether any discovery metric moved off zero
- [x] Step 10.3 classify the new boundary precisely

### Notes

- commit:
- validation:
  `logs/hostctl_flashcapture_backend_legacy_port_20260316_092802_blob_compat_install/capture.log`
- outcome:
  - backend marker remained `backend-legacy-port`
  - boot still reached:
    - `legacy_port runtime_init stage=before_esp_radio_init`
    - `after_esp_radio_init`
    - `setup_timer result=ok`
    - `init_tasks result=ok`
    - `runtime_init stage=before_wifi_new`
    - `legacy_port_wifi_new stage=validate`
    - `legacy_port_wifi_new stage=wifi_init`
  - new boundary:
    - `legacy_port_wifi_init stage=esp_wifi_init_internal.before`
    - then no further progress
  - no discovery metric improved because init no longer completed
  - this is still useful closure: the first direct old internal
    install/global-state import is incompatible with the current blob-facing
    layer before scan even begins

## Phase 11: Decision Gate

### Goal

Decide whether it is still rational to continue source-level compatibility work
inside this repo.

### If Metrics Improve

- [ ] Step 11.1 continue with the next direct blob/internal slice
- [ ] Step 11.2 keep trimming dead shim paths only after repeated success

### If Metrics Stay Flat

- [x] Step 11.3 stop Rust-side/source-level import work for this branch
- [x] Step 11.4 record that the remaining problem is below the current
      vendorable boundary
- [x] Step 11.5 decide between:
  - pairing `backend_legacy_port` more directly with old
    `esp-wifi-sys 0.7.1` expectations, or
  - abandoning the current blob generation for discovery-critical paths

### Notes

- commit:
- validation:
  `logs/hostctl_flashcapture_backend_legacy_port_20260316_092802_blob_compat_install/capture.log`
- outcome:
  - stop condition reached
  - the full Rust-side parallel old-stack import did not move discovery
  - the first direct blob/internal compatibility slice regressed init before
    `esp_wifi_init_internal(...)` completed
  - conclusion: further source-level extraction against the current blob
    generation is not a credible next step
  - recommended next chunk:
    - pair `backend_legacy_port` more directly with old `esp-wifi-sys 0.7.1`
      internal expectations in a truly parallel backend, or
    - stop pursuing the current blob generation for discovery-critical paths

## Recommended Starting Point

Start with the old internal install/global-state contract that sits closest to:

- the globals and tables passed into `esp_wifi_init_internal(...)`
- the internal state that must exist before `wifi_scan_start_process`
- the admission path before `recv_cb_sta/ap` becomes observable

The first implementation chunk should be the smallest direct compatibility unit
that can change one of the validation counters above.

## Stop Conditions

Stop immediately if:

- canonical full-flash validation does not run
- the active backend is not `backend-legacy-port`
- the proposed change is only another Rust-side wrapper/table/facade refactor
- the direct compatibility slice validates cleanly but leaves all discovery
  metrics unchanged

## Next Pending Step

Do not add more source-level compatibility slices against the current blob.

If work continues, the next chunk must be one of:

1. vendor and bind `backend_legacy_port` more directly to old
   `esp-wifi-sys 0.7.1` internal expectations in a truly parallel backend, or
2. stop the current discovery-port line and choose a different Wi-Fi backend
   strategy for stable discovery.
