# 2026-03-17 Wi-Fi Pre-Eventpost Result-Link Investigation Plan

## Goal

Find the first point in the app/runtime Wi-Fi path where scan results fail to be
linked before vendor `event_post(ScanDone)`.

This plan exists to keep the investigation on the currently live boundary and to
avoid reopening branches already closed by earlier March 6-9 work.

## Current Boundary

On the conditioned board, the current app diagnostic build now shows:

- explicit direct scan starts successfully: `scan_rc=0`
- `ScanDone` is reached
- the result list is already empty inside vendor `event_post(ScanDone)`
- the list stays empty through:
  - app-layer `ScanDone`
  - `esp_wifi_scan_get_ap_num()`
  - `esp_wifi_scan_get_ap_records()`

Current captures:

- app with eventpost-level scan-list probes:
  - `logs/flash_capture_20260317_scan_list_eventpost_app/capture.log`
- same-board working comparator:
  - `logs/flash_capture_20260317_scan_list_comparator/capture.log`
- app with prelink summary probes:
  - `logs/flash_capture_20260317_prelink_summary_app/capture.log`
- comparator with matching prelink summary probes:
  - `logs/flash_capture_20260317_prelink_summary_comparator/capture.log`
- app with full `esp_radio` init/new trace:
  - `logs/flash_capture_20260317_init_trace_app/capture.log`

## Proven Facts

### Same-board split still exists

- On the same conditioned board, the current app path reaches `scan_rc=0` but
  returns `ap_num=0`.
- On that same board, the standalone comparator still returns nonzero APs.

### Retrieval is not the primary failure point

- In the app path, the list is already empty before `esp_wifi_scan_get_ap_num()`
  is called.
- In the comparator path, the list is populated before `get_ap_num()` and is
  only cleared after `get_ap_records()`.

### History already established this failure family

Relevant records:

- `docs/development/upload-throughput-history/part-18.md`
- `docs/development/upload-throughput-history/part-17.md`
- `docs/development/upload-throughput-history/part-16.md`
- `docs/development/wifi-upload-decision-ledger.md`

Most important earlier result:

- `part-18.md` already recorded that the BSS/result list was empty at raw
  `ScanDone`, not only later at `get_ap_num()`.

## External Signal

- [espressif/esp-idf#17664 comment 3935694161](https://github.com/espressif/esp-idf/issues/17664#issuecomment-3935694161):
  reverse-engineered evidence of two blob-side Wi-Fi state paths, one updating
  while another consumer-visible path stays stale; not proof for this bug, but
  it supports the current model that our app/comparator split can live inside
  blob-side state materialization rather than only at the API/wrapper layer

## Investigation Exclusions

Do not treat these as the next primary line unless a new signal points back to
them:

- early boot / reset / PSRAM bring-up
- generic RF blackout below all firmware
- scan-wrapper argument shaping
- direct-vs-wrapped scan-call choice
- queue implementation swaps
- wait-queue wake behavior
- blob-tick timing variants
- simplified PHY wrapper restoration
- Rust-side legacy facade / event / global-table shim rewrites already logged in
  the decision ledger

## Working Hypotheses

The next unresolved boundary is likely one of:

1. RX/beacon admission fails before result-linking begins
2. candidate rejection happens before fixed-pool BSS admission
3. scan completion reaches `ScanDone` without ever materializing the result
   list
4. list-link state is created transiently and dropped before vendor
   `event_post(ScanDone)`

## Phase 1: Freeze The Current Boundary

Status: complete

Artifacts:

- `logs/flash_capture_20260317_scan_list_app/capture.log`
- `logs/flash_capture_20260317_scan_list_comparator/capture.log`
- `logs/flash_capture_20260317_scan_list_eventpost_app/capture.log`

Result:

- app list is already empty at vendor `event_post(ScanDone)`
- comparator list is still populated before retrieval

## Phase 2: Use History As Novelty Gate

Status: complete

Objective:

- verify whether the current failure shape was already isolated in older work

Result:

- yes; earlier March 6-9 work already showed:
  - empty raw `ScanDone` list
  - `scannum=0`
  - `head_ptr=0`
  - blank fixed `g_cnxMgr` pool
  - untouched `g_scan` history rows

Consequence:

- the next work should target earlier scan admission/linking, not retrieval or
  already-rejected wrapper layers

## Phase 3: Pre-Eventpost Result-Link Boundary

Status: active

Objective:

- move one step earlier than vendor `event_post(ScanDone)` and determine whether
  result nodes ever become visible before that point

Primary targets:

1. RX/beacon admission path in the app/runtime substrate
2. candidate admission / reject path before fixed-pool BSS allocation
3. first point where `scannum` should become nonzero
4. first point where `g_ic + 0x130/+0x134` should hold a linked list

Current preferred direction:

- instrument the earliest pre-`event_post(ScanDone)` path that can still expose
  whether candidate/result nodes are ever admitted

Latest result:

- on the conditioned board, the app and comparator still match on all of these
  at the pre-retrieval point:
  - `rx_sta=0 rx_ap=0`
  - `history_count=0`
  - `history_nonzero_rows=0`
  - `cnx_nonzero_slots=0`
- the app also shows real allocator/free activity at the same point:
  - `malloc_internal_count=26`
  - `wifi_malloc_count=13`
  - `wifi_calloc_count=11`
  - `free_count=35`
- despite that, only the comparator has a populated result list:
  - comparator: `scannum=0x000a`, non-null `head_ptr`
  - app: `scannum=0x0000`, `head_ptr=0`
- the lower-perturbation send/recv caller probe showed:
  - app send-side recent items come from `pp_post` (`caller_ptr=0x8008e028`)
  - app recv-side recent items are consumed in `ppTask+0x2d`
    (`caller_ptr=0x80084ffd`)
- one apparent discriminator is retired: the app `postcall` `0x06`
  control-process set was stale diag-induced getter traffic, not fresh
  scan-completion work
- a clean app checkpoint before those getter calls now exists:
  - `logs/flash_capture_20260317_after_start_pre_driver_state_app/capture.log`
  - at that point the app naturally shows only:
    - `wifi_set_mode_process`
    - `wifi_start_process`
    - `wifi_ipc_process`
- the full init trace has now answered that question directly:
  - `logs/flash_capture_20260317_init_trace_app/capture.log`
  - current app `esp-radio` init executes `esp_wifi_set_tx_done_cb`,
    both `esp_wifi_internal_reg_rxcb`s, `set_country`, and `set_power_saving`
- that retires the missing-full-init hypothesis for the current app path
- a targeted process-counter probe has now narrowed the queue side further:
  - `logs/flash_capture_20260317_process_counts_app/capture.log`
  - in the explicit-compare window, the app does enqueue and consume
    `wifi_scan_start_process` exactly once
  - in that same window, the app does not hit:
    - `wifi_get_ap_list_process`
    - `wifi_clear_ap_list_process`
    - `wifi_set_promis_process`
  - so the app is not obviously clearing the AP list in that explicit-compare
    window; the list is simply never materialized before `ScanDone`
- a static app-vs-comparator blob comparison narrowed the next layer:
  - both images still route the path through `check_bss_queue`,
    `cnx_bss_alloc`, `cnx_update_bss`, and `cnx_update_bss_more`
  - key function sizes still differ materially between app and comparator,
    which keeps the live target below outer runtime/init and inside blob-side
    result materialization

Interpretation:

- the old history discriminators remain useful as exclusions, but on this board
  they no longer separate the working comparator from the failing app
- the live divergence is now narrower:
  - result-list link materializes in the comparator
  - result-list link never materializes in the app
  - and that happens without `g_scan` history or fixed `g_cnxMgr` pool usage in
    either path at this point
- the classic early-reject family is retired:
  - app and comparator both keep all four reject flags at `0`
  - artifacts:
    `logs/flash_capture_20260317_reject_flags_app/capture.log`,
    `logs/flash_capture_20260317_reject_flags_comparator/capture.log`
- the allocator "seeded slot" hypothesis is also retired:
  - app and comparator both show `cnx_seeded_slots=0`
  - artifacts:
    `logs/flash_capture_20260317_seeded_slots_app/capture.log`,
    `logs/flash_capture_20260317_seeded_slots_comparator/capture.log`
- the writer-site lookup now shows both binaries touch `scannum` and
  `g_ic+0x130` from the same family of blob functions:
  - `ieee80211_sta_scan`
  - `clear_bss_queue`
  - `wifi_get_ap_list_process`
  - `wifi_get_ap_record_process`
- the more invasive legacy-port runtime diagnostics are useful only as a
  side-channel; they perturb scan-start enough to move the app back to the
  earlier admission-failure form
- the remaining target is therefore no longer "did init happen?" but
  "what happens after successful init and successful scan admission that keeps
  `scannum/head_ptr` at zero on the app path"
- and it is no longer the higher-level queue dispatch either:
  - `wifi_scan_start_process` does run
  - the list is still empty at vendor `event_post(ScanDone)`
  - no explicit-compare evidence currently points to `wifi_clear_ap_list_process`
    as the primary cause
- two more live hypotheses are now retired:
  - corrected `g_misc_nvs` target probes match in app and comparator; the
    dereferenced target words are `0/0/0` in both
  - init-stage queue process counts before diag reset prove the app does execute
    `wifi_set_rxcb_process` twice, `wifi_register_mgmt_frame` once,
    `wifi_set_country` once, and `wifi_set_ps_process` once
- a new explicit-compare A/B also retired the simple hidden-network
  interpretation:
  - `logs/flash_capture_20260317_show_hidden_true_app/capture.log`
  - `logs/flash_capture_20260317_show_hidden_false_app/capture.log`
  - changing only `show_hidden` from `1` to `0` leaves the failing app shape
    unchanged: `scan_rc=0`, `scannum=0`, `head_ptr=0`, `ap_num=0`
- a static app-vs-comparator compare also narrowed the post-hidden path:
  - the first silent reject block after `scan_check_hidden` is structurally the
    same in both blobs through the first `memcmp` / `candidate+0x92` checks
- the remaining target is now the silent candidate-side reject path inside
  `scan_profile_check` / `scan_parse_beacon`, now more likely in differing
  input state or a later parse stage than in the first post-hidden block

## Concrete Next Step

Instrument the app/runtime path earlier than vendor `event_post(ScanDone)`,
favoring:

1. the first silent zero-return branch inside `scan_profile_check`
2. the candidate-side input tuple that feeds that branch, especially:
   - candidate offset `+0x06`
   - candidate offset `+0x80`
   - candidate offset `+0x92`
   - the post-`scan_check_hidden` path
   - the follow-on `memcmp` path before `cnx_bss_alloc`
3. the first app/comparator divergence after successful explicit scan start
   (`scan_rc=0`)
4. the blob-side candidate admission path after `wifi_scan_start_process`,
   specifically `scan_profile_check` and `scan_parse_beacon`

The immediate success criterion for the next slice is:

- identify the first earlier checkpoint where the app diverges from the
  comparator on whether `scannum/head_ptr` ever become nonzero
- and determine whether the failing app path clears or never links result nodes
  after successful init and successful scan admission
- and keep `check_bss_queue`, `g_misc_nvs`, and missing init-stage process
  registration retired as differentiators
- and keep the runtime `show_hidden` flag retired as a direct discriminator for
  this failing window
- and keep the `wifi_log`-emitting `scan_profile_check` branches retired

## Stop Conditions

Pause and re-scope if either of these becomes true:

- a new capture shows the app list is populated before vendor `event_post` and
  the boundary moves later
- or the earliest reachable pre-`event_post` path still shows no candidate/list
  activity, pushing the target further back into RX/beacon ingress
