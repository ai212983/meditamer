# Typed Observation Subscriptions Evidence Index

- Status: Complete — required evidence accepted; the [plan](typed-observation-subscriptions.md) is closed.
- Last-reviewed: 2026-08-28
- Decisions: [ADR-0018](../architecture/0018-typed-observation-subscriptions.md), [ADR-0019](../architecture/0019-provider-owned-periodic-demand-overrides.md)

## How to use this index

This file records results and limits, not phase status or to-dos. Evidence applies to the named
source/image and exercised behavior. Host tests, synthetic faults and physical measurements remain
separate evidence types. Missing coverage does not invalidate a passing result for another case.
Keep new entries to the outcome, evidence pointer and material limitation; detailed run narratives
belong with artifacts. Update capability summaries when evidence changes; keep every action in the plan.

The complete E-0001–E-0051 [historical ledger](../archive/observations/typed-observation-subscriptions-ledger-2026-08-28.md)
and [prior plan](../archive/observations/typed-observation-subscriptions-plan-2026-08-28.md) are frozen,
byte-for-byte snapshots taken before E-0052. Their phase status and next-step instructions are historical.
Archive SHA-256: ledger `a4e3e39e06f2395e13577ecbad6039fa0481b1854f1303c4d9f0064c5bdb1ca4`;
plan `d012930b32905c4a29efeb5767030871af4d9ebda44dfc06f18515aae5bf4908`.
The E-0029 compaction's earlier originals remain at its recorded artifact location.

## Implementation evidence

| Capability | Results retained | Evidence and executable inventory |
| --- | --- | --- |
| Demand, delivery and state | Mixed per-field cadence; all initial policies; fresh-result and health bypass; retained successful timestamps; checked identity/restart | E-0032/E-0039/E-0044; `platform/observation/src/{demand,delivery,state,ids}.rs` tests |
| Bounded admission and scheduler | Shared queued/pending credits, expiry, conversion-time admission, idle behavior and completion-based retry covered | E-0039; `platform/observation/src/ingress/tests.rs`, `runtime/tests.rs` |
| Periodic demand ownership | Latest live demand restored on expiry/cancellation/control; host coverage does not require UI polling | E-0046; `platform/observation/src/periodic.rs` and runtime tests |
| Product adapters | Committed owners, rollback/re-entry, stale results, independent fresh-entry delivery and persistent battery trace covered using product code | E-0031/E-0040/E-0041/E-0047; `products/medinote/src/observations.rs`, `tools/touch_replay/tests/environment_presentation.rs` |
| Provider control and drivers | Correlated acknowledgement, retained resume, partial suspension, failure logging/retention and cancellation paths covered | E-0030/E-0031/E-0040; `tools/touch_replay/tests/{bounded_control,battery_driver,medinote_drivers}.rs` |
| Upload scheduling | Observation providers and the IMU control pipeline share upload I/O priority, preventing strict-priority starvation; fresh device acquisitions and cleanup passed | E-0054; `products/meditamer/src/firmware/scheduling.rs`, `scripts/ci/check_network_owner_source.sh` |
| External SD ownership | Actual FAT/runner allocation rejection and unpolled drop restore external memory; upload cancellation removes temporary state and permits a new successful transfer | E-0055; `products/meditamer/src/firmware/storage/sd_task/runtime_loop/allocation_fixture.rs`, hostctl `sd-upload-recovery` |
| Board/panel recovery | Expander invalidation/reconciliation and GPIO-only abort have host regressions | E-0030/E-0031; `boards/inkplate-tempera/src/{expander_tests.rs,panel_lifecycle/tests.rs}` |

The bounded E-0052 source/test audit identified no additional concrete core/adapter/lifecycle defect.
It did not prove exhaustive correctness, electrical fault behavior or physical display responsiveness.

## Earlier results and corrections

| Evidence | Result and limitation |
| --- | --- |
| E-0001–E-0023 | Initial migration, real readings and injected rejection established. Early lifecycle omissions and misleading startup localization were corrected later; individual results remain in the archive. E-0020 demonstrated battery cleanup rejection/recovery, not an electrical fault. |
| E-0025–E-0028 | Wake/read/out-of-range failure logging, real BQ driver tests and injected serial failures covered. E-0028 normal restoration reached readiness but did not capture healthy battery delivery; E-0043 supplies that evidence. |
| E-0029–E-0031 | Review found stale control correlation, lost resume intent and failed-waveform cleanup defects; E-0030 fixes them, with production-path regressions. E-0031 repairs expander/cache handling and removes blanket upload sampling suppression; E-0054 fixes scheduling starvation and validates upload coexistence on device. |
| E-0032–E-0033 | Product ownership and Medinote sleep acknowledgements implemented; later E-0040/E-0049/E-0050 refine and exercise that lifecycle. |
| E-0034–E-0038 | Board/role memory accounting, redundant SD staging removal, returning boot frames and external SD/UI placement measured. Static memory improvements do not prove all placement safety or runtime peaks. |
| E-0039–E-0042 | Final bounded admission, product demand and battery fixture/metadata wiring implemented. Later device entries establish acquisition, separately from these builds/tests. |

## E-0024 — E-0022 follow-up: startup refresh recovery fixed and one-shot BME688 cleanup failure qualified

Startup full-scan completion/retry fixed. One injected BME688 cleanup failure blocked panel entry;
startup recovered and reached readiness without a serial repaint, then normal firmware was restored.
The original E-0022 startup mechanism remains unknown. Artifacts:
`logs/typed-observation-subscriptions_e0023_20260827/` (artifact ID differs from evidence ID).

## E-0043 — Inkplate battery device qualification and serial readiness

Fresh battery fixture and later ordinary periodic acquisition passed; the later successful sample
was 300003ms after the previous one. Healthy hardware evidence, without active upload/BLE load or
physical faults. Artifacts: `logs/typed-observation-subscriptions_e0043_20260828/`.

## E-0044 — Provider metadata and BME688 fixture

All four projections expose successful-sample identity. Inkplate BME688 fresh fixture and later
ordinary acquisition passed, 300017ms between successful samples; battery regression also passed.
The first shorter capture showed cache only and is retained as insufficient periodic evidence.
Artifacts: `logs/typed-observation-subscriptions_e0044_20260828/`.

## E-0045 — Medinote SHTC3 and ADC fixtures

Both fresh fixtures and ordinary periodic acquisitions passed (60073/60000ms between successful
SHTC3/ADC samples). Cached UI lines were distinguished from new acquisition. Startup SHTC3 timeouts
recovered but their cause was not established. Artifacts: `logs/typed-observation-subscriptions_e0045_20260828/`.

## E-0046 — Bounded periodic demand overrides

All four overrides implemented. Medinote SHTC3 expiry and ADC cancellation/latest-live cadence
restoration passed. Inkplate's uninterrupted test instead observed valid closure during normal panel
suspension; it was not a sensor failure. E-0047 tests that actual lifecycle. Forced UI stall was not
physically exercised. Artifacts: `logs/typed-observation-subscriptions_e0046_20260828/`.

## E-0047 — Correlated Inkplate panel-cycle qualification

Both providers passed exact override closure, current suspend/resume identities, restored live demand
and fresh periodic acquisition. Two initial failures exposed duplicate-cache delivery and a host
round-trip ordering race; both were fixed, regression-tested and passed on the final image.
This proves normal panel recovery, not rejected-panel restoration or electrical faults. Startup
BME688/touch NACKs recovered before fixture admission; their cause remains unknown.
Artifacts: `logs/typed-observation-subscriptions_e0047_20260828/`, final `flash-owner/` image and reports.

## E-0048 — Startup I2C transaction diagnostics and sleep scope

Startup-only transaction context added with unchanged task pools/BLE stack reservation. Three
post-flash reset captures were clean; the planned ten-cycle soak stopped on host connection failures.
No startup NACK was reproduced with diagnostic context, so no causal fix was established. Manual
Medinote wake included operator interaction; E-0049 provides automated timer evidence instead.
Artifacts: `logs/typed-observation-subscriptions_e0048_20260828/`.

## E-0049 — Automated Medinote sleep and cleanup rejection

Environment rejection, battery rejection and normal deep sleep/Timer wake passed through the UI owner.
Synthetic failure followed real cleanup and affected one request. Passive attach fixed the new
workflow's unwanted USB reset; the initial failed attach remains recorded. No active periodic lease,
light-sleep or electrical-fault proof in this entry. Artifacts: `logs/typed-observation-subscriptions_e0049_20260828/`.

## E-0050 — Active periodic fixtures across rejected sleep

Both rejection cases closed both active leases, restored latest 60000ms Home demand and produced
later fresh successful samples before old lease expiry, without host refresh requests or reset.
This extends E-0049 with periodic restoration; deep sleep was not rerun on this image.
Host suites passed 636 tests; linked stack/task pools unchanged. No runtime memory-peak claim.
Artifacts: `logs/typed-observation-subscriptions_e0050_20260828/`.

## Image provenance

All E-0043–E-0050 and E-0054–E-0055 captures identify dirty-checkout sources/ELFs based on
`27b091b9fef1b530c520afa2383dca387bed09e1`; they are not clean-commit qualification.
Each artifact directory and frozen entry retains its exact hashes, intermediate failures and logs.
Latest recorded installed images, associated through flash artifacts rather than device readback:

| Target | Evidence | ELF SHA-256 |
| --- | --- | --- |
| Inkplate, MAC `e8:6b:ea:fb:d5:54` | E-0055 | `4e5ae0a0a2c2ef025a898419eaf5122917d8084b25ca57e227a3f6e27f5f07c6` |
| Medinote, MAC `a4:cb:8f:d0:6a:74` | E-0050 | `dcee03a165349cce10ac860809ce88435475e3deb7024233a98f4ad2bd2c02fb` |

E-0047 normal panel and E-0049 deep-sleep results belong to their respective earlier identified images.
A result is neither promoted to a later image nor invalidated merely because another image was built.

## E-0051 — Subscription scope and proportional validation

The user deferred ADR-0018's general resource-promotion gate for this milestone; its specific
capacity-change checks remain applicable. Bounded storage, safety gates and targeted placement
landing requirements were retained. E-0051 proposed another synthetic rejected-panel
fixture; E-0052 classifies it as deferred composite assurance rather than a known-defect fix.

## E-0052 — Reconcile completion, backlog and evidence

2026-08-28. Three independent bounded audits mapped existing core/adapter tests, target lifecycle
paths and device results. No blanket evidence invalidation or additional concrete subscription
implementation defect was identified. The plan now owns one required acceptance item, upload
coexistence, because that production behavior changed without recorded device validation.
The former phases were replaced with completed capabilities and explicit dispositions; the full
prior documents were frozen with the hashes above. This index contains no execution backlog.

The audit preserves unmeasured hardware cases and known startup, placement and baseline limitations;
it does not classify them as passed. No firmware, test implementation, device or image was changed.
Current host tests passed: observation **90**, Medinote **115**, touch-core **241**, board **51**
(**497 total**), through `scripts/host-test.sh test <suite>`. Logs:
`logs/typed-observation-subscriptions_e0052_20260828/`. No firmware build, hostctl suite or hardware
run was repeated; earlier build/device results retain their original provenance.
Document checks: 64 links passed, one excluded; 22 inbound/local heading links resolve. Both frozen
documents match their pre-edit hashes; whitespace and local-path checks passed.

## E-0053 — Keep only required follow-ups

2026-08-28. Removed optional assurance and incident-reminder lists from the plan. The existing SD/UI
landing prerequisites are now explicit validate-or-revert actions R2/R3, required alongside R1;
they are not optional risks left behind after completion. Unmeasured cases remain evidence limits,
without future-work promises. No firmware, hardware or historical test result changed.
Five-document link check passed (49 links, one excluded); whitespace and frozen-archive hashes passed.

## E-0054 — Upload coexistence and scheduling correction (R1)

2026-08-28. Actual upload exposed expired BME688 requests, then an IMU quiescence timeout: upload
I/O at priority 1 could starve priority-0 tasks. Battery, environment acquisition, IMU acquisition and
IMU pipeline now share upload I/O priority; IMU sampling suppression is unchanged. Host prerequisite
defects were also fixed: acknowledge upload/diagnostic mode before provisioning, and enable discovery
telemetry before the warm-boot Wi-Fi gate. Failed captures remain alongside the final results.

On the final Inkplate BLE image above, BME688 and BQ27441 produced fresh successful samples after
admission while a correlated HTTP upload body remained unfinished. HTTP 201, exact 64-byte SD
readback and cleanup passed without observed reset, panic or failed control transition. The canonical
Wi-Fi gate passed eight discovery rounds and one-/three-upload cycles; optional soak was skipped.
Host suites passed: hostctl **318**, observation **90**, touch-core **241**; hostctl Clippy, the scheduling
source guard and both Inkplate release builds passed. Linked sections/task pools stayed unchanged;
the BLE stack reservation remains **39844 bytes**. Production scheduling unit tests are not wired into
a host harness; the source guard and device run supply this change's validation.

Artifacts: `logs/typed-observation-subscriptions_e0054_20260828/`, especially `flash-final/`,
`coexistence-closure/`, `wifi-regression-verified/`, `source-manifest.json` and `software-summary.json`.
The probe proves acquisition during body ingestion, not sustained concurrent SD writes, throughput
qualification, physical fault handling or runtime memory peaks. Medinote was not changed or reflashed.

## E-0055 — External SD runner validation (R2) and stale flash ceiling

2026-08-28. The actual SD startup factories now return allocation errors before the existing reset/halt
policy. An opt-in device fixture rejected FAT and runner allocations through the null-allocation branch,
then dropped an allocated, unpolled runner. Each case restored **4059304 external free bytes** exactly,
with no runner polling or probe initialization; ordinary startup then reached readiness. This used
diagnostic ELF `827b2d8b2ac02274296beaf4aedb4458b488720737dc31206b53dc915ac5c5c9`.

After restoring the normal BLE image above, hostctl disconnected a correlated unfinished upload,
observed an Abort roundtrip and absent destination/temp files, then completed a distinct upload with
HTTP 201 and exact 64-byte readback on the same runner. No reset/panic/control failure was observed.
The canonical Wi-Fi gate passed eight discovery rounds plus one-/three-upload cycles. Hostctl **320**
and SD/FAT **24** tests, both host linters, targeted source guards and default/BLE builds passed.
SD task pool stays **1456 bytes**; BLE linked sections/stack stay unchanged (**39844-byte** stack).
Default linked data grows **16 bytes**, reducing its stack reservation to **114788**; no capacity changed.

The first flash was rejected before writing: hostctl still used the obsolete A/B slot size. It now
reads `ota_0` capacity from the current CSV, preserving 128 KiB headroom: **3538944-byte** image ceiling.
Boundary regressions passed, and the previously rejected default diagnostic image flashed successfully.
Artifacts: `logs/typed-observation-subscriptions_e0055_20260828/` (`allocation-results.json`,
`flash-production/`, `cancellation-recovery/`, `wifi-regression/`, `software-summary.json`).
Limits: synthetic allocation rejection, not physical PSRAM exhaustion or reset/halt execution; session
cancellation, not dropping in-flight DMA; no UI, Medinote, throughput or runtime-peak qualification.

## E-0056 — External UI ownership: automated checks and visual confirmation passed

Fixed a real initialization-cleanup gap: failures left LVGL's display/input session registered.
A scoped initialization owner now deinitializes LVGL on failure after surface/callback cleanup;
success transfers ownership to the installed Backend. Duplicate initialization and missing input
creation are rejected. The persistent 128 KiB PSRAM arena is retained; draw/callback/I/O state
stays internal, and callback user data encodes a generation-checked route, never the external model address.

On Inkplate `e8:6b:ea:fb:d5:54`, diagnostic ELF
`03bd1423a3408eff0751c26009b60f92c6a55785b83bafbd9a84bfb06928e1c8` passed actual model allocation
rejection, five initialization boundary rejections twice, installed callback delivery, queued-intent
purge, teardown, stale callback rejection and successful reinitialization. All 13 fixture markers
passed, external memory returned to baseline, then ordinary startup reached readiness. Boundary
rejections are synthetic errors through actual initialization; they do not reproduce electrical faults
or every LVGL allocator failure.

Normal BLE ELF `46ba2cc8c959c98022ba5d6e6d836d5ff68c32de421bcccd264451479c6984d8` was restored.
Startup, correlated full repaint and two launcher→diagnostics→home cycles passed without lifecycle,
allocator-integrity, callback or reset faults. On 2026-08-28 the user confirmed that startup, screen
transitions and repaint "looks correct", completing physical rendering acceptance and closing R3.
This is user-observed visual evidence, separate from the automated checks. E-0047 remains applicable
to unchanged panel/provider paths.

The first UI run's zero-byte allocator tolerance failed (8–68-byte spans). The total-size variation
is exactly accounted for by TLSF's four-byte block headers: payload plus headers remains 129596
bytes at every checkpoint. A second two-cycle run passed the already documented E-0006 256-byte
settling bound, with unchanged live-block counts and stable current heaps. No tolerance was changed;
this is bounded UI evidence, not a new resource qualification or a long-run plateau claim.

Tooling now queries live readiness through a passive, locked UART session, checks correlated repaint
completion before cycling, accepts Ambient as the initial cycle surface, and rejects composition/audit
fault flags. Fixed flash-capture's retained-console/post-command ownership conflict; boot capture,
time sync and `STATE SET upload=off` then passed. YAML still owns workflow sequencing.

Default/BLE builds and 385 host tests passed (`ui-shell` 17, `shell` 46, `hostctl` 322); `render`
compiled with no tests. All four host lints and UI-ownership/stack source guards passed. BLE internal
`.data`/`.data.wifi`/`.bss`/stack remain 17024/1872/72332/39844 bytes; default is
17240/540/64052/114764 (24 fewer stack bytes than E-0055). Display/SD pools remain 3064/1456;
initialization and model-construction frames are unchanged. Diagnostic symbols are absent from
normal images. Medinote is unchanged. Artifacts: `logs/typed-observation-subscriptions_e0056_20260828/`
(`flash-fixture-final/`, `flash-production-final/`, `ui-lifecycle-bounded.json`, `validation-summary.json`).
