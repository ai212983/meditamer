# Typed Observation Subscriptions Plan

- Status: Active — subscription fault/recovery and coexistence validation remain open; general resource qualification deferred (E-0051).
- Last-reviewed: 2026-08-28
- Decision: [ADR-0018](../architecture/0018-typed-observation-subscriptions.md) and [ADR-0019](../architecture/0019-provider-owned-periodic-demand-overrides.md) — Accepted
- Evidence: [Implementation ledger](typed-observation-subscriptions-ledger.md), E-0050 active-fixture restoration and E-0049 Medinote sleep/rejection passed. E-0048 startup diagnostics leave the cause open; E-0046/E-0047 periodic/panel gates passed.

## Objective and current state

Environmental and battery tasks use `platform/observation` with product-owned subscription and presentation adapters. The [UI coordinator proposal](shared-ui-coordination.md) may adopt them later.

| Target / provider | Recorded progress | Remaining work |
| --- | --- | --- |
| Medinote / SHTC3 | Fixture/periodic expiry, deep sleep and rejection/restoration passed (E-0045/E-0046/E-0049/E-0050) | Startup timeout, physical faults and loaded recovery |
| Medinote / ADC | Fixture/cadence, deep sleep and rejection/restoration passed (E-0045/E-0046/E-0049/E-0050) | Physical ADC faults, light sleep and loaded recovery |
| Meditamer / BME688 | Fresh fixture/periodic delivery and normal panel recovery passed (E-0044/E-0047) | Startup I2C errors, panel faults and coexistence |
| Meditamer / BQ27441 | Fresh fixture/periodic trace and normal panel recovery passed (E-0043/E-0047) | Expander/panel rejection recovery and physical coexistence qualification |

E-0030 fixes E-0029 R1–R3: all five panel clients use correlated, retained desired state;
resume survives caller timeout; failed waveform closure isolates GPIOs and schedules bounded cleanup.
Production-protocol/finalizer regressions and target builds pass. E-0039–E-0041 budget bounded storage
and wire BME688/Medinote entry admission plus persistent battery demand; physical fault recovery remains open.
E-0028's restored image reached runtime-ready; its capture missed battery delivery. E-0022's original
startup cause remains unknown and is not a reason to repeat the same injection without a new hypothesis.

## Scope and validation effort

Finish subscription behavior, real acquisition and targeted coexistence checks. Run relevant tests/builds
per change; compare affected-board memory when storage, task layout or call paths change. Keep existing
safety gates and hardware checks for placement/allocation/ownership changes in proportion to their risk.

Whole-firmware stack/heap high-water, connected-BLE capacity and sustained workload qualification are
deferred until a representative milestone or a concrete failure/headroom regression requires them.
They are not subscription completion gates, and this amendment creates no separate resource plan.
E-0037/E-0038 SD/UI placement checks remain outstanding before landing; measurements stay in the ledger/RAM reference.

## Ownership and contract to preserve

- `platform/observation` owns pure typed subscription, freshness, delivery, and scheduling logic.
  Board drivers own conversion timing, units, hardware operations, and cleanup. Targets own tasks,
  storage, arbitration, deadlines, and retry limits; products own consumer lifetime and presentation.
- Maintain one subscription authority per product UI owner for surfaces and persistent consumers.
  Key subscriptions by provider, owner identity, owner generation, and slot. Replacement, removal,
  expiry, committed entry, rollback, recovery, and retained-shell wake must update that authority.
- Aggregate requested fields and the strictest maximum age per field. Combined conversions may cache
  extra fields; only requested fields schedule acquisition. Drivers retain hardware conversion timing.
- Schedule delivery separately from acquisition. Preserve successful per-field timestamps across
  failure; publish identity, generation, revision, last-attempt time, snapshot, and typed health.
  Advance revision per completed attempt and generation on restart; report counter exhaustion.
- Support use-cache, refresh-if-stale, and cache-then-refresh. Cache-then-refresh requests a fresh
  observation even when cache is fresh. Entry and explicit-request results each bypass the delivery
  interval once; cached entry must preserve the fresh-result bypass. Publish health transitions promptly.
- Observe-now requests have bounded admission and expiry. A conversion may satisfy compatible requests
  admitted before it completes. Removed owners stop periodic demand and delivery; admitted one-shots
  may finish into cache until expiry. Deliver only to the current owner/subscription.
- Read each provider state once per UI iteration and merge all ready presentation changes into one
  refresh. Keep the existing units, battery mappings, and single product self-heating correction.
- Log concrete acquisition failures once per failed attempt. Keep idle/success paths silent and bound
  repeated diagnostics through retry backoff. A failed attempt retains the last valid sample.

## Remaining work, in order

1. Extend the existing Inkplate panel-cycle fixture with one-request cleanup rejection for BME688 and
   BQ27441, one case each with an active periodic override. Keep real cleanup and the UI/panel owner;
   correlate the rejected acknowledgement, prove the rejected operation starts no waveform, and verify closure,
   newer resume IDs, latest live demand and fresh periodic acquisition without host refresh requests.
   A subsequent normal repaint must pass. Synthetic acknowledgement rejection is not a physical fault.
2. Use E-0048 traces when the intermittent startup NACK recurs; fix only an established cause. Continue
   targeted driver-fault, UI-stall expiry and shared-bus/upload checks. Inkplate sleep is future work.

### Phase 0 — Bounded storage and change-specific memory checks

- Use the [measured storage budget](../reference/dram/dram-budget.md#periodic-demand-storage-e-0046):
  two subscription slots and two total outstanding requests per provider (live plus fixture), one
  provider-owned demand controller, and one additional two-field provider. Full/closed/invalid/expired admission rejects
  explicitly; replacement frees its old slot. With two one-shot fixture sessions, the Inkplate model is
  2820 bytes (1860 internal + 960 external); Medinote's model is 2840 internal.
  These include existing objects, exclude drivers/futures/task overhead, and are not linked deltas.
- Declare per-target safety bounds for conversion, transactions, cleanup, whole-control operations,
  expiry and retry. Test full/closed/expired admission and recovery. Record acquisition/control timing
  for the exercised case; do not turn an observed latency into a future-proof scheduling margin.
- For memory-affecting changes, compare before/after linked sections, task pools and significant
  construction/poll frames on affected targets/features. Identify source, toolchain and ELF; include fixture storage.
- Preserve the unchanged 33900-byte Inkplate BLE stack floor. E-0048 reserves 39844, with 5944 above
  the floor. Medinote reserves 53696 with central/security/pairing intact; fixture/session placement
  is measured separately. Rebudget future fixture expansion before acceptance.
- Preserve E-0036's returning boot/spawn boundaries; inspect affected frames when those paths change.
- Budget internal data/instruction RAM, CPU stack, task pools, heap and external PSRAM separately
  for each board. Stack reservation is not runtime headroom; internal heap relocation spends another
  internal budget. Do not treat S3's unused region or uninitialized PSRAM as available storage.
- Investigate memory regressions; do not trim capacities or lower the stack floor to pass.

### Phase 1 — Complete the core contract

- Treat `max_age` as a freshness threshold, not a promised delivery deadline. The scheduler uses
  last-success time plus maximum age; conversion, arbitration and executor delay can produce stale
  intervals. Preserve actual timestamps and last good values so consumers can distinguish freshness.
- Defer predictive early scheduling. Keep datasheet waits in drivers; measure acquisition and
  demand-to-result latency without turning observations into fixed scheduling margins. A future hard
  deadline needs an explicit contract, admission policy and reported misses, not a guessed lead time.
- Preserve E-0039 completion-aware admission, expiry-driven capacity release and completion-based retry
  deadlines on reactive wakes. Medinote tracks successful-sample revision separately from attempt revision,
  including same-millisecond ADC completion and a success followed by failure before UI polling.
  Fresh cache and failure-only revisions cannot spend the entry-result bypass.
- Preserve E-0044 provider identity, last-attempt and successful-sample identity in all four
  projections. Failures retain sample identity; BME688 entry uses revision, including equal timestamps.

### Phase 2 — Qualify control recovery and complete production fixtures

- Preserve E-0030's request correlation, retained resume/reset intent, fail-closed identity exhaustion,
  and control priority. Running acknowledges touch reset enqueue, not pipeline consumption.
- Preserve GPIO-only abort after failed waveform closure, the 2-second shutdown deadline including
  bus/expander admission, and cleanup retries at least 1000ms after completion without another refresh.
- Qualify delayed/hung participants, repeated cycles, partial suspension, waveform-close failure,
  timeout cancellation and eventual cleanup on device. Host tests cover the shared production control
  and finalizer; actual task scheduling, GPIO isolation and rail shutdown still need measurement.
- Keep the current single panel-operation owner. Five sequential 2-second waits can consume 10 seconds
  per suspend/resume phase; executor starvation and blocked driver/output work are not cancelled by them.
- Preserve all four providers' live-demand controller, shared credited ingress, state `Watch`, and authority
  wiring. Compatible in-flight requests may use completion; empty demand idles,
  expiry frees credits, and control wins independently of queue capacity. Preserve these tested rules.
- Through the lifecycle owner, close admission, discard queued one-shots, finish/cancel acquisition,
  clean up, and acknowledge quiescence within the declared budget. Preserve periodic demand during
  suspension; resume re-evaluates freshness. Verify scheduling and cleanup under load.
- E-0046 applies periodic overrides between acquisitions; expiry, cancellation and control/stop
  restore the latest live demand without UI polling. Parked providers reject new periodic fixtures;
  one-shot and periodic sessions are mutually exclusive. See the [run guide](../guides/troubleshooting.md#headless-battery-observation).
  E-0047 passes the separate panel-cycle gate for both Inkplate providers; qualify startup/panel-fault recovery;
  acquisition/delivery and control-deadline evidence remain open for fault/contended paths.

### Phase 3 — Complete Medinote

- Preserve E-0031 SHTC3 cleanup propagation (250ms acquisition plus 100ms cleanup) and ADC reset
  on timeout/drop (50ms conversion budget). Qualify the physical behavior and deadlines under load.
- Preserve E-0032's product Home adapter, checked owner renewal, stale-key rejection and health-only
  delivery; Watch access and LVGL application stay in target wiring. Host lifecycle cases do not
  establish a target rollback path or physical sleep correctness.
- Preserve E-0040's two-provider sleep gate: both ingresses close before awaiting cleanup; both current
  acknowledgements share one 2-second deadline. Timeout/drop retains intent, resume recovers, and new
  suspension retries cleanup. E-0049 scripts this gate through the UI owner; synthetic rejection follows real cleanup.
- Preserve one-minute Home freshness/delivery and bounded fresh-entry admission for SHTC3 and ADC.
  Remove demand before sleep; publish both resumes before renewing ownership on wake or rejected sleep.
  Re-entry and suspension-epoch retries retain the original expiry; ordinary removal leaves admitted
  requests free to finish into cache. E-0050 passes both active-lease closures and fresh periodic recovery; ADC faults/load remain open.
- Verify re-entry, rollback, sleep rejection/recovery, both providers, and top-level UI responsiveness
  with adapter fixtures and scripted device consumers; light sleep, physical faults and contended recovery remain open.

### Phase 4 — Complete Inkplate environment and presentation

- Keep initial BME688 setup before touch startup, then transfer to the recoverable provider driver.
- Preserve E-0039's checked authority identity, demand withdrawal and bounded entry refresh. Removal
  stops periodic sampling and delivery; admitted one-shots may finish into cache. Five-minute
  freshness/delivery and entry validity remain; validity is not a promised result latency. A missed
  suspension epoch renews still-live entry admission without extending its original expiry.
- Keep acquisition and cache delivery independent of upload mode. The former blanket suppression
  guarded display-owned reads; no remaining blanket constraint was found in source/archive review.
  Retain shared-bus ownership and existing panel-repaint restrictions. Qualify upload/sensor coexistence;
  introduce product admission limits only if an identified resource constraint requires them.
- Preserve E-0031 cached observation application and the final timer/clock refresh decision. Separate
  touch press/release frames retain their existing behavior; qualify physical batching and responsiveness.
- Preserve E-0024 startup recovery: retry before readiness, at least 1000ms after failure completion,
  with an existing backend and permitted full repaint. Any successful full scan completes startup;
  pending environmental updates must not bypass backoff.
- Qualify repaired panel controls, initialization recovery, consumer removal during conversion, and
  panel/upload contention on identified firmware. Keep injected faults distinct from real bus evidence.

### Phase 5 — Complete Inkplate battery and shared expander recovery

- Preserve the generic real `BqDriver` and its five fault/retention/recovery and idle/backoff-silence
  tests (E-0028/E-0031).
- Preserve E-0043 healthy fixture/periodic `BATTERY_DELIVER` evidence. E-0028 proves injected wake-failure
  diagnostics only; physical wake/read/range faults and subsequent recovery remain unqualified.
- Preserve E-0031 PCAL6416A invalidation before mutating awaits and reconciliation after failure/drop.
  Qualify physical partial writes/reset, panel/frontlight/wake coexistence and the narrow `FG_GPOUT` path.
- Preserve E-0041's UI-owned persistent trace subscription, startup request and five-minute cadence.
  The provider publishes queryable state; navigation/recovery never withdraws or renews the trace owner.
  Keep shared-expander I/O independent of UI polling, and retain the last valid level on failure.
- Exercise expander faults, repeated wake, foreground fixture removal, and fuel-gauge/touch/BME688/
  RTC/panel/frontlight coexistence under the completed panel and upload policy.

### Phase 6 — Validate subscriptions on both targets

- Run applicable software/safety gates and fix introduced regressions. Report existing pinned-linker,
  documentation-link and code-analysis gaps separately, including `process_cycle` 9 arguments and the
  S3 `runtime_ui.rs` parser gap. Hostctl transport-test/clock-query issues remain separate; no baseline pass is implied.
- Verify bounded admission, actual acquisition, retained values, control deadlines and fresh recovery
  under the named lifecycle/contention cases. Apply Phase 0 memory checks when relevant.
- Use canonical flash wrappers and one serial owner. Capture reset through runtime-ready and beyond
  five minutes when asserting Inkplate periodic delivery; restore normal firmware after injections.
- Record exact images, measurements, automated results, and evidence type in the ledger and
  [hardware matrix](../reference/hardware-test-matrix.md). Preserve the updated
  [coordinator handoff](shared-ui-coordination.md#typed-observation-provider-handoff) and existing authority.

## Acceptance and validation

Required acceptance is automated and independent of selected screens or menu order. Host adapters use
fixture lifecycles and observations. One-shot fixtures retain live demand and may share compatible
in-flight conversions. Periodic-demand fixtures enter through that authority, settle existing
acquisition before applying an override, and restore latest live demand on every exit, including expiry
while the UI is stalled. Script panel/upload through their owners. E-0049 supplies Medinote's UI-owner
sleep adapter: E-0050 also proves active-lease closure/restoration across rejection. Physical faults/load remain open.

| Layer | Required cases |
| --- | --- |
| Core | Mixed one/five-minute consumers; combined/partial fields; all initial policies; entry/fresh bypass; health-only change; replacement/removal/expiry; stale owners; restart/exhaustion; capacity/deadline bounds |
| Runtime/control | Idle/rate limits; request during conversion; coalescing/expiry; failure/backoff/recovery; queued/active suspension; cleanup rejection; correlated acknowledgements; retained resume intent; waveform-close failure |
| UI adapters | Committed entry/rollback/re-entry; rebuilt health; retained-shell wake; provider removal/late results; formatting; batched refresh; slow sibling; persistent trace demand |
| Drivers | Missing sensor/init retry; timeout/cancellation; repeated faults; battery wake/read/range diagnostics and retained levels; failed/partial expander writes and reconciliation |
| Startup | No-dirty retry; persistent-failure backoff; normal/service full-scan completion; upload suppression; one-shot cleanup failure followed by runtime-ready |
| Devices | Real startup/periodic readings; Medinote sleep/wake; fixture removal/restoration; rejected-panel recovery; control deadlines; shared-bus/upload contention; consumer responsiveness |

Run focused tests/lint through `scripts/host-test.sh`; extend existing suites. Qualify sensor/upload
coexistence with the [Wi-Fi regression gate](../guides/wifi-regression-gate.md). Final software gates:

```bash
CARGO_INCREMENTAL=0 scripts/ci/check_software_baseline.sh all
targets/medinote-waveshare/build.sh --locked
scripts/ci/check_markdown_links.sh --all
```

Mark Done when all four providers use the complete contract, production fixtures pass, and identified
firmware meets subscription responsiveness, recovery, acquisition, bounded-storage and relevant memory-regression checks.
