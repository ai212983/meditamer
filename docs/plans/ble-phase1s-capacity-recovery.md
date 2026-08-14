# Phase 1S Capacity Recovery Plan

- Status: Capacity passed on the formal gate; Wi-Fi regression gate's SD blocker resolved (CAP-0011/CAP-0012). `acceptance_3_cycle`'s internal-memory-floor failure (CAP-0013) is root-caused (CAP-0014) as expected order-statistics behavior of a monotonic since-boot diagnostic register under a longer, continuous-session workload — not a leak, and not evidence the coordinator's real BLE-admission gate is at risk (CAP-0009 already clears that gate 20/20 with ~3x margin). No safe first-party fix was found; whether the test's own floor check is measuring the right thing for this workload is referred to the user per ADR-0011's human-acceptance requirement. The gate does not yet pass clean.
- Last-reviewed: 2026-08-14 (CAP-0014)
- Started: 2026-08-14
- Parent: [BLE foundation and upload transport plan](ble-foundation-plan.md)
- Evidence: [Phase 1S capacity recovery ledger](ble-phase1s-capacity-recovery-ledger.md)
- Parent evidence: [BLE implementation ledger](ble-foundation-ledger.md)
- Decision boundary: [ADR 0011: bounded BLE service foundation](../architecture/0011-bounded-ble-service-foundation.md)

## Goal

Close the Phase 1S internal-memory gate while retaining both Wi-Fi and BLE.

The preferred result is a repeatable increase in applicable internal free memory. A revised acceptance
floor is a separate valid outcome when supported by an explicit workload and failure model, an ADR
update, and human acceptance.

The [parent plan](ble-foundation-plan.md) owns BLE architecture and promotion. The
[capacity ledger](ble-phase1s-capacity-recovery-ledger.md) owns this plan's status and evidence; the
[parent ledger](ble-foundation-ledger.md) retains broader BLE phase evidence. This plan covers only
the remaining capacity work and its validation.

## Current position

- Commit `77569d33d575a5ebb70a2edc45047ab351d7ce5c` is the imported source baseline.
- The BLE image is 1,739,296 bytes against the 1,900,544-byte ceiling. Flash capacity is healthy.
- The binding Wi-Fi run reached 15,156 bytes of applicable internal free memory against the current
  16,384-byte floor, leaving a 1,228-byte shortfall against that headline number. The coordinator's
  actual admission gate is stricter: `REQUIRED_OFF_FREE_BYTES` in
  [`src/firmware/net/runtime.rs`](../../src/firmware/net/runtime.rs) requires 16,384 + 4,112 (the
  BLE controller's own runtime allocation) = 20,496 bytes free plus a 4,112-byte contiguous block
  above the reserve — a real shortfall of 5,340 bytes against 15,156, not 1,228. See
  [CAP-0002](ble-phase1s-capacity-recovery-ledger.md#cap-0002--capacity-model-protected-floor-ownerlifetime-map-avoidable-overlaps).
- The engineering target is 21,520 bytes: the code's real 20,496-byte gate plus the accepted
  1,024-byte run-to-run drift allowance. Reaching it requires 6,364 repeatable bytes over the observed
  minimum (not the 17,408/2,252 figures this section previously stated, which described only the
  ADR's headline aggregate floor).
- The current link map leaves 104 bytes free in `.dram2_uninit`, so extending the existing internal
  heap is not a practical recovery path. The [DRAM budget](../reference/dram/dram-budget.md) records
  the region ownership.
- A single manual radio-handoff cycle on the exact current commit, with Wi-Fi associated but idle (no
  HTTP/SD traffic in flight), passed the coordinator's floor check at 59,608 bytes free / 31,672-byte
  largest block — about 3x the corrected 20,496-byte target. The historical 15,156-byte low-water was
  not reproduced under this light load; it plausibly needs genuine concurrent upload/SD activity at
  acquire time. See [CAP-0003](ble-phase1s-capacity-recovery-ledger.md#cap-0003--first-device-read-on-the-exact-current-commit-one-manual-radio-handoff-cycle).
- **Resolved.** Investigating under load surfaced a real but unrelated bug (F-0008: a stale
  credentials snapshot reused on radio-handoff retry, causing a bounded ~130s Wi-Fi outage — not a
  hang — after an aborted mid-write handoff). That bug is fixed and device-verified (commit
  `9606e152e816215449486f286cb400bc52d08bab`). The formal 20-cycle gate
  (`hostctl test ble-phase1s --cycles 20`) then passed clean on that exact commit with **zero
  capacity-specific code changes**: off-state 59,608/31,672 bytes (identical across all 20 cycles),
  serving low-water 19,896 bytes (clears the 16,384-byte floor by 3,512 bytes), zero drift, zero UART
  overflow. The historical 15,156-byte failure does not reproduce on this source tree. No CAP-0002
  recovery candidate was needed. See
  [CAP-0009](ble-phase1s-capacity-recovery-ledger.md#cap-0009--formal-20-cycle-gate-passed-clean-no-capacity-candidate-needed).
  Remaining before this plan's Acceptance section is fully satisfied: the separate
  [Wi-Fi regression gate](../guides/wifi-regression-gate.md) on this same exact artifact.

Applicable internal memory means memory with the capabilities required by its owner. Aggregate free
bytes and largest-contiguous-block measurements are both relevant.

## Relevant documents

Governing context:

- [BLE foundation and upload transport plan](ble-foundation-plan.md)
- [Phase 1S capacity recovery ledger](ble-phase1s-capacity-recovery-ledger.md)
- [BLE implementation ledger](ble-foundation-ledger.md)
- [ADR 0011: bounded BLE service foundation](../architecture/0011-bounded-ble-service-foundation.md)
- [Asset upload transport plan](asset-upload-transport.md)

Memory and validation:

- [DRAM budget](../reference/dram/dram-budget.md)
- [ROM and stack budget](../reference/dram/dram-budget-rom-stack.md)
- [Wi-Fi regression gate](../guides/wifi-regression-gate.md)
- [Build and flash guide](../guides/build-and-flash.md)
- [Hardware test matrix](../reference/hardware-test-matrix.md)
- [Service-mode diagnostics](../guides/service-modes.md)

Prior experiment evidence:

- [Wi-Fi/upload decision ledger](../archive/wifi/wifi-upload-decision-ledger.md)
- [Blackout diagnostic knobs](../archive/wifi/blackout-diagnostic-knobs.md)
- [Upload throughput history, part 30](../archive/upload/upload-throughput-history/part-30.md)

The archived decision ledger and upload history are the preflight for new Wi-Fi/upload experiments.
An exact mechanism and value already tested there should be repeated only when new source evidence or
an explicit reconfirmation need changes the premise.

## Constraints

- Wi-Fi and BLE both remain in the product.
- The current 16,384-byte floor remains authoritative unless the ADR and acceptance decision change.
- PSRAM relocation applies only to owners whose complete access lifetime is safe outside
  Internal/DMA/cache-off constraints.
- Capacity claims use an exact clean artifact and report both aggregate free memory and the largest
  contiguous block.

## Step 1: establish the capacity model

Use existing source, allocation provenance, link maps, and device reports to produce:

1. A short explanation of what workload and failure modes the 16,384-byte floor protects.
2. An owner/lifetime map for the 15,156-byte low-water event, correlated with station RX,
   DHCP/listener state, upload connection setup, buffers, SD work, and release.
3. A list of avoidable overlaps with estimated recoverable bytes and confidence level.

This step should identify whether the binding requirement is aggregate capacity, contiguity, a
capability-specific allocation, or a combination of them.

## Step 2: choose one recovery candidate

Prefer candidates in this order:

1. Shorten or resequence a first-party transient lifetime.
2. Relocate proven-eligible state to PSRAM.
3. Introduce a new bounded vendor RX ownership mechanism supported by source evidence.
4. Prepare a floor-revision proposal when the capacity model shows that the current reserve exceeds
   the protected workload.

Select one candidate for the first build. Record its owner, expected recovery, expected timeline
change, and the observation that would disprove it.

## Step 3: implement and validate

For the selected candidate:

1. Make one narrow, auditable change and build from a clean exact HEAD.
2. Run locked dependency resolution, `scripts/ci/check_ble_controller_patch.sh`,
   `scripts/ci/check_network_owner_source.sh`, and `scripts/ci/check_software_baseline.sh`.
3. Record the commit, toolchain, features, ELF/BIN hashes, image size, and section sizes.
4. Run the [Wi-Fi regression gate](../guides/wifi-regression-gate.md) on the exact artifact.
5. Run 20 Phase 1S handoff cycles and record allocator, stack, UART, lease, and restoration evidence.
6. Append the result to the [capacity ledger](ble-phase1s-capacity-recovery-ledger.md), including
   failed hypotheses.

## Acceptance

Phase 1S capacity passes when the exact artifact demonstrates:

- applicable internal free memory at or above the approved floor, with the 17,408-byte engineering
  target reported separately;
- run-to-run low-water drift no greater than 1,024 bytes;
- sufficient largest-block capacity for the allocations identified in Step 1;
- CPU0 stack of at least 8,192 bytes, touch stack of at least 1,024 bytes, and zero UART overflow
  delta;
- successful Wi-Fi regression and 20-cycle handoff gates, including Wi-Fi restoration; and
- no resets, panics, allocator mismatches, ownership leaks, or update-precedence violations.

Passing evidence is linked from the [capacity ledger](ble-phase1s-capacity-recovery-ledger.md), after
which the parent plan can advance Phase 1S and reconsider Phase 2. A floor revision follows the same
device validation after its ADR decision is accepted.

## Immediate next step

Capacity work is done at source/formal-gate scope: Steps 1–3 completed without needing a recovery
candidate (CAP-0001–CAP-0009). Running the [Wi-Fi regression gate](../guides/wifi-regression-gate.md)
on that artifact (CAP-0010) surfaced a separate, pre-existing bug — aborted mid-write SD uploads
permanently poisoned their directory's shared temp file (CAP-0005) — which blocked the gate's
acceptance stage on this physical card. That bug is now root-caused, fixed, host-test-covered, and
hardware-verified, including recovery of the poisoned card (CAP-0011/CAP-0012), and the gate's
`discovery_debug`/`acceptance_1_cycle` stages pass clean on the fixed artifact.

**The plan's acceptance bar is not yet met.** Rerunning the gate on the fixed artifact
(commit `e0213a76cb80be5b0821e53720b9eafdf24f714f`) fails at `acceptance_3_cycle` instead, reproducibly
(3/3 across CAP-0013 and CAP-0014), on an internal-memory-floor violation (`min_internal_free_bytes`
13,456–15,116 against the 16,384 floor) unrelated to CAP-0005/CAP-0011 — see CAP-0013.

**CAP-0014 (2026-08-14) root-caused this.** `min_internal_free_bytes` is a monotonic register that only
ever records a new *lower* value from boot until reboot (confirmed by source review). `ble-phase1s`'s
20 cycles each force a full Wi-Fi controller/stack teardown-and-recreate (a side effect of the BLE-open/
close radio handoff), so its reported minimum is the worst of 20 independent, short, freshly-reset
exposure windows (19,896, CAP-0009). `wifi-acceptance` never tears Wi-Fi down between cycles (confirmed
live: `net_apply_config`/`net_start` print `skip ... because network is already ready`), so its minimum
is the worst single draw from one long, continuously-growing exposure window. Direct hardware A/B
instrumentation confirms **no leak**: current/resting internal-free bytes is bit-identical cycle-to-cycle
in every run; only the monotonic register moves, and only downward. CAP-0002's candidate byte-recovery
fix (route `METRICS`/`METRICSNET` through the allocation-free serial bypass) was tested directly on
hardware and showed no confirmed benefit, and relocating it to PSRAM was rejected on an unresolved
flash-cache-disabled safety question for too small a speculative gain. No first-party fix was found.

The coordinator's actual BLE-admission gate (`floor_ok` in `src/firmware/net/runtime.rs`) never reads
this monotonic register — it re-probes *current* free bytes at the post-quiescence moment, which
CAP-0009 already showed clears the real 20,496-byte/4,112-contiguous-block requirement by roughly 3x
margin in all 20 cycles. Whether `acceptance_3_cycle`/`acceptance_soak`'s use of the monotonic register
as a hard floor check is the right test for this workload — as opposed to a floor revision, a change to
what the test measures, or accepting the gate as documented and non-clean — is a decision for the user,
not this investigation; per ADR-0011, any floor change requires explicit human acceptance and none was
made here. Once the user decides a path forward and it is implemented and verified, rerun the full
Wi-Fi regression gate on a fresh artifact; only then can this plan hand off to the parent plan for
Phase 1S closure and Phase 2 reconsideration.
