# Phase 1S Capacity Recovery Ledger

- Status: Active — root cause identified for CAP-0013 (order-statistics behavior of a monotonic
  since-boot register, not a leak); no safe first-party byte-recovery fix found; floor-methodology
  question referred to the user per ADR-0011's human-acceptance requirement
- Last-reviewed: 2026-08-14 (CAP-0014)
- Started: 2026-08-14
- Plan: [Phase 1S capacity recovery](ble-phase1s-capacity-recovery.md)
- Parent evidence: [BLE implementation ledger](ble-foundation-ledger.md)

## Ledger contract

The status table is a current summary. Evidence entries are append-only and identify the exact source
or artifact they describe. A correction adds a new entry with a `Supersedes` reference.

This ledger covers the Phase 1S capacity model, selected recovery candidate, and validation. Broader
BLE phase evidence remains in the parent ledger.

## Status

| Work item | State | Evidence | Next action |
| --- | --- | --- | --- |
| Capacity model | Passed on the formal gate; CAP-0013's narrower-margin data point is now root-caused | CAP-0001–CAP-0009, CAP-0013, CAP-0014 | CAP-0009: formal 20-cycle `hostctl test ble-phase1s` gate passed clean (`gate_passed: true`, zero violations) on the exact fixed commit `9606e152...`. Off-state 59,608/31,672 bytes (identical, zero drift, all 20 cycles); serving low-water 19,896 bytes, clearing the ADR floor by 3,512. No capacity candidate was needed. CAP-0013 found the separate `wifi-acceptance` workflow's repeated-upload-cycle-in-one-boot load pattern reproducibly drives `min_internal_free_bytes` below 16,384. **CAP-0014** (2026-08-14) root-causes this: `min_internal_free_bytes` is a monotonic, never-reset-except-at-boot low-water register (confirmed by source review — `seed_internal_low_water` runs once, inside `init_allocator`'s one-shot init guard). `ble-phase1s`'s 20 cycles each force a full Wi-Fi controller/stack teardown-and-recreate (a side effect of the BLE-open/close radio handoff, confirmed by source review of `run_network_epoch`), so its reported minimum is the worst of 20 independent, short, freshly-reset exposure windows. `wifi-acceptance` never tears Wi-Fi down between cycles (confirmed live: `net_apply_config`/`net_start` print `skip ... because network is already ready`), so its minimum is the worst single draw from one long, continuously-growing exposure window. Direct hardware A/B instrumentation (CAP-0014) confirms **no leak**: current/resting internal-free bytes is bit-identical cycle-to-cycle in every run; only the monotonic low-water register moves, and only downward, consistent with order statistics over a longer sample — not accumulation. |
| Recovery candidate | **Not needed for CAP-0009's own gate; none found for CAP-0013 either** | CAP-0009, CAP-0014 | The formal `ble-phase1s` gate passed without any capacity-specific code change. CAP-0014 evaluated CAP-0002's overlap A (route `METRICS`/`METRICSNET` through the allocation-free serial-command bypass) directly on hardware and found no confirmed benefit (adding a per-cycle `METRICS` poll did not reproducibly worsen or improve the observed floor), and rejected relocating that allocation to PSRAM given an unresolved flash-cache-disabled-safety question for too small an unconfirmed gain. RX-queue-depth reduction remains known-bad (CAP-0002). No first-party fix is recommended from this investigation. |
| Implementation | **N/A** | CAP-0009, CAP-0014 | No capacity implementation required; the only code change this plan produced was the F-0008 credentials-retry fix (CAP-0007), unrelated to byte recovery. CAP-0014 made no source changes. |
| Device validation | Capacity passed on the formal gate; regression gate's `acceptance_3_cycle` still fails, root cause understood, no fix applied | CAP-0009–CAP-0014 | 20/20 capacity cycles clean (CAP-0009). CAP-0005's `HCTLUPLD.TMP` poisoning bug is fixed and hardware-verified (CAP-0011/CAP-0012); the regression gate's `discovery_debug` and `acceptance_1_cycle` stages pass clean. `acceptance_3_cycle` fails reproducibly (3/3 across CAP-0013 and CAP-0014) on `min_internal_free_bytes` — CAP-0014 shows this is the expected, explained behavior of a monotonic since-boot diagnostic register under a longer, continuous-session workload, not a product-safety defect: the coordinator's own real BLE-admission gate (`floor_ok` in `src/firmware/net/runtime.rs`) never reads this register at all — it re-probes *current* free bytes at the post-quiescence moment, which CAP-0009 already showed clears the real gate (20,496/4,112-contiguous) by roughly 3x margin in all 20 cycles. Whether `acceptance_3_cycle`/`acceptance_soak`'s use of the monotonic register as a hard floor check is the right test for this workload is a methodology question referred to the user (ADR-0011 candidate order 4; human acceptance required, not changed here). |

## Evidence entries — append only

### CAP-0001 — Imported Phase 1S capacity baseline

- Date: 2026-08-14
- Source baseline: `77569d33d575a5ebb70a2edc45047ab351d7ce5c`
- Parent evidence: E-0010 in the [BLE implementation ledger](ble-foundation-ledger.md)
- Supporting reference: [DRAM budget](../reference/dram/dram-budget.md)

Observed baseline:

- BLE image: 1,739,296 bytes against the 1,900,544-byte ceiling.
- Binding Wi-Fi internal-free minimum: 15,156 bytes.
- Current hard floor: 16,384 bytes.
- Hard-floor shortfall: 1,228 bytes.
- Engineering target with the 1,024-byte drift allowance: 17,408 bytes.
- Recovery needed from the observed minimum to that target: 2,252 bytes.
- Remaining `.dram2_uninit` link capacity: 104 bytes.

Result: capacity remains open. The next evidence entry will record the protected-floor derivation and
the owner/lifetime map for the 15,156-byte event. This entry imports existing source and device
evidence; it does not represent a new device run.

### CAP-0002 — Capacity model: protected floor, owner/lifetime map, avoidable overlaps

- Date: 2026-08-14
- Source baseline: `77569d33d575a5ebb70a2edc45047ab351d7ce5c` (unchanged from CAP-0001)
- Evidence kind: source review, doc review, and local log search. No new device run; this satisfies
  plan Step 1, which is explicitly scoped to "existing source, allocation provenance, link maps, and
  device reports."
- Method: read the coordinator's own admission gate
  ([`src/firmware/net/runtime.rs`](../../src/firmware/net/runtime.rs)) and allocator provenance
  ([`src/firmware/psram/mod.rs`](../../src/firmware/psram/mod.rs),
  [`src/firmware/psram/provenance.rs`](../../src/firmware/psram/provenance.rs)) for what is actually
  measured and gated; traced every candidate owner named in the plan (station RX, DHCP/listener,
  upload connection setup, buffers, SD work, release) to its current allocation site; searched
  `logs/*.log` for a captured `min_internal_free_bytes=15156` line (none found — see Gap below).

#### 1. What the floor protects

[ADR-0011](../architecture/0011-bounded-ble-service-foundation.md) states one hard limit, "Internal
free memory >= 16,384 bytes," but the coordinator's actual runtime gate is stricter and is a
**combination of three separate requirements**, not one aggregate number:

```rust
// src/firmware/net/runtime.rs:51-53
const INTERNAL_RESERVE_BYTES: usize = 16_384;
const REQUIRED_BLE_ALLOCATION_BYTES: usize = 4_112;
const REQUIRED_OFF_FREE_BYTES: usize = INTERNAL_RESERVE_BYTES + REQUIRED_BLE_ALLOCATION_BYTES; // 20,496
```

Before the coordinator will grant `StartingBle` (`run_network_epoch`'s `floor_ok` check,
`src/firmware/net/runtime.rs:239-273`), it requires, all at once:

- **Aggregate**: settled internal free >= `REQUIRED_OFF_FREE_BYTES` (20,496), read twice
  (`probe_free_before_bytes`/`probe_free_after_bytes`) to reject a value still moving.
- **Contiguity**: a largest free block above the 16,384 reserve of at least
  `REQUIRED_BLE_ALLOCATION_BYTES` (4,112) — a real block-search via
  `psram::probe_internal_block_above_reserve`, not an aggregate-free proxy. This exists because
  16,384 bytes free does not guarantee a 4,112-byte contiguous run after allocator fragmentation.
- **Capability**: all of the above must be `Internal`-capability memory specifically (`esp_alloc`'s
  `MemoryCapability::Internal`), because BLE's controller/host state cannot live in PSRAM. This is
  the same reason firmware-update serial commands and Wi-Fi RX packets are pinned internal (see part
  2) — it is a hardware constraint (PSRAM needs the flash cache; the BT controller and update-in-
  -flight state must survive with the cache disabled or be usable from contexts PSRAM can't serve),
  not a preference.
- **Zero product-work in flight**: `http_connections == 0`, `sd_roundtrips == 0`, `sd_sessions == 0`,
  `radio_callbacks == 0`, `radio_queues == 0`, no active radio source, no open callback admission, and
  zero late/unknown/reclaim-failed/corrupted/contended queue counters — i.e., the floor is checked only
  once Wi-Fi/HTTP/SD ownership is provably quiescent, so it is *not* measuring peak in-use capacity
  during active Wi-Fi/upload traffic. It is measuring what remains resident after quiesce.

**Answering the plan's closing question directly: the binding requirement is a combination of
aggregate capacity, contiguity, and a capability-specific (Internal-only) allocation — it is not
reducible to any one of the three.** A fix that only grows aggregate free bytes without preserving a
4,112-byte contiguous block, or that relies on PSRAM, does not satisfy the actual gate.

**Discrepancy worth resolving before Step 2 is scored:** the plan's and ADR's headline numbers
(16,384-byte floor; 17,408-byte engineering target with 1,024-byte drift) describe only the aggregate
piece. The code's actual admission gate needs 20,496 bytes free plus a 4,112-byte contiguous block
above the 16,384 reserve. Against the historical 15,156-byte minimum, the real shortfall to
`REQUIRED_OFF_FREE_BYTES` is 5,340 bytes, not 1,228, and the real engineering target (adding the same
1,024-byte drift allowance) is 21,520 bytes — recovery of 6,364 bytes, not 2,252. This entry does not
resolve which number is authoritative; it flags that plan Step 2 must pick a candidate against the
code's actual gate (20,496 / 21,520) or explicitly revise `REQUIRED_OFF_FREE_BYTES` alongside any ADR
floor-revision proposal, so the two numbers cannot silently diverge again.

What the floor protects, in workload/failure-mode terms: it guarantees the BLE controller/host has
enough contiguous Internal RAM to initialize *after* Wi-Fi/HTTP/SD are torn down, so a BLE-open
attempt cannot wedge the allocator, cannot silently fall back to a capability BLE cannot use, and
cannot start against a fragmented heap that looks free in aggregate but cannot serve one real
allocation. The failure mode it prevents is exactly the one `probe_internal_block_above_reserve`
exists to catch: aggregate-free-looks-fine, contiguous-block-is-not.

#### 2. Owner/lifetime map for the 15,156-byte event

The plan asks for this correlated against station RX, DHCP/listener state, upload connection setup,
buffers, SD work, and release. Tracing each to its current allocation site in this exact commit:

| Candidate owner | Internal-heap owner? | Evidence |
| --- | --- | --- |
| Station RX (vendor Wi-Fi packets) | **Yes — established primary suspect** | `wifi_runtime_config` sets `rx_queue_size = 2` in release (`src/firmware/net/wifi/backend_esp_radio.rs:41`). `INTERNAL_HEAP_DRAM2_BYTES`'s own doc comment (`src/firmware/psram/mod.rs:83-101`) records that "two live vendor RX packets" were correlated with "a 3,400-byte heap excursion and a 13,508-byte low-water" (~1,700 bytes/packet) via the allocator's own hook chain: `_esp_alloc_alloc` records the low-water allocation (`src/firmware/psram/provenance.rs`), and `_meditamer_match_internal_low_water_wifi_rx` (called by `esp-radio` immediately before it builds `PacketBuffer` telemetry) correlates that low-water allocation with a station-RX callback, atomically, without allocating/logging/blocking. This mechanism is real and already wired; it is the only owner with a source-code-documented correlation to an internal-heap low-water figure. |
| DHCP/listener state | **No — ruled out** | `embassy-net`'s `StackResources<4>` (`src/firmware/net/runtime.rs:43,55`) is a fixed `StaticCell` allocation in `.bss`/`dram_seg`, already counted in the link-time static budget (1,864 bytes per [DRAM budget](../reference/dram/dram-budget.md)). It does not draw from the runtime internal *heap* the low-water figure tracks. |
| Upload connection setup (HTTP RX/TX/header/chunk buffers) | **No, under observed conditions** | `HttpBuffer<N>` (`src/firmware/storage/upload/http.rs:67-99`) and the SD upload chunk buffer (`src/firmware/storage/transfer_buffers.rs:29-31`) both route through `psram::alloc_large_byte_buffer`, which **prefers PSRAM and only falls back to Internal on PSRAM exhaustion** (`src/firmware/psram/buffer.rs:49-76`). The runtime evidence already on file (`docs/reference/dram/dram-budget.md`: "`large_alloc_internal_ok=0`, `large_alloc_fail=0`: every large buffer lands in PSRAM") shows that fallback has never fired in a captured run. Ruled out unless a future run shows `large_alloc_internal_ok > 0`. |
| SD work (`FatEngine`) | **No — already recovered** | `dram-budget.md`'s "Task-local state onto the heap" section describes an *older* state (`FatEngine` boxed into the internal heap, pool 10160 → 5416). Current source has moved past that: `fat_engine` is now `psram::ExternalValue<FatEngine>` (`src/firmware/storage/sd_task/runtime_loop.rs:47-56`, comment: "Persistent FAT interpreter state has no ISR/DMA ownership. Keep it in external PSRAM so dynamic Wi-Fi RX buffers retain the internal reserve."). The dram-budget doc is stale on this specific point; the source is authoritative. Not a live contributor. |
| Serial command dispatch (diagnostic/telemetry polling) | **Yes — plausible, unconfirmed for this exact event** | `src/firmware/serial.rs:70-220` deliberately allocates command futures from the Internal heap via `InternalValue::try_new_bounded`: 900 bytes (firmware-update family — internal by hardware necessity, since PSRAM is unreachable with the flash cache disabled), 1,200 bytes (`METRICS`/`METRICSNET`), and 1,500 bytes (ordinary commands). A `low_overhead_diagnostic` bypass already exists and is allocation-free for a named subset (`StackStatus`, `AllocatorStatus`, `RadioHandoff*`, and, only with both `ble-foundation` and `asset-upload-http`, `NetStatus`/`StateSet`/`StateDiag`/`NetStart`/`NetStop`/`NetRecover`) — the comment at the call site says this exists explicitly "while the network owner measures or changes radio ownership." `METRICS`/`METRICSNET` and ordinary commands are *not* in that bypass. Since capacity/telemetry evidence is gathered by polling serial commands during a live run, if `METRICS` (1,200 B) or an ordinary command (1,500 B) fired near the low-water moment, it would transiently compete for the same Internal heap as station RX. Not confirmed for the specific 15,156-byte event — no local log matches it (see Gap below) — but it is the second-most-plausible owner and is directly actionable (see part 3, item A). |
| BLE controller's own runtime allocation | **Not a cause, but defines the real gate** | `REQUIRED_BLE_ALLOCATION_BYTES = 4,112` (`src/firmware/net/runtime.rs:52`) matches `dram-budget.md`'s "BTDM controller task still consumes at least 4,112 release bytes from the separate internal heap." This allocation only happens *after* the coordinator grants `StartingBle`, i.e., after quiesce — it cannot itself explain a low-water reached during/before quiesce, but it is why part 1's real target is 20,496/21,520, not 16,384/17,408. |
| Wi-Fi/PHY blob's own opaque heap use | **Open — explicitly flagged as a device gate in existing docs** | `dram-budget.md` and E-0009/E-0010 both note that runtime heap high-water for vendor controller-owned allocations "remains a device gate." The vendor blob's internal allocations are visible to `esp-alloc`'s hooks (since `esp-radio` is patched to allocate `PacketBuffer`s through `esp-alloc`), but any allocation the vendor blob makes *outside* that patched path is not provenance-tracked. Not ruled in or out here; requires the Step 3 device run with `min_internal_alloc_wifi_rx_matched` evidence. |
| "Release" (teardown/dealloc path) | **Instrumented, not itself a consumer** | `_esp_alloc_dealloc_completed` / `mark_internal_low_water_released` (`src/firmware/psram/provenance.rs`) mark the winning low-water allocation's generation released, specifically to reject stale-pointer-reuse matches. This is bookkeeping for the correlation mechanism, not a byte owner. |

**Gap**: no file under `logs/` in this repository contains a captured `min_internal_free_bytes=15156`
line or an accompanying `RADIO_HANDOFF`/`wifi_rx_matched` correlation for that exact value (checked via
`grep -rho 'min_internal_free_bytes=[0-9]*' logs/*.log`; the lowest captured value across all local
logs is 17,016, the non-BLE baseline cited in `dram-budget.md`). The 15,156 figure is inherited from
the historical Phase 1S source branch's device evidence (per E-0010/F-0006 in the
[BLE implementation ledger](ble-foundation-ledger.md)) and was not reproduced by a device run on this
exact commit. The owner/lifetime map above is therefore built from allocation-site source review, not
from re-deriving the 15,156 figure itself — consistent with plan Step 1's evidence scope, but it means
the station-RX-vs-serial-dispatch split above is source-plausible, not device-confirmed. Step 3's
device run should capture and correlate this explicitly.

#### 3. Avoidable overlaps: estimated recoverable bytes and confidence

| # | Overlap | Estimated recoverable bytes | Confidence | Basis |
| --- | --- | --- | --- | --- |
| A | Route `METRICS`/`METRICSNET` (and, if safe, ordinary commands) through the existing `low_overhead_diagnostic` allocation-free path instead of `InternalValue::try_new_bounded` | up to 1,200 (METRICS/METRICSNET) or 1,500 (ordinary) per in-flight command, transient | Medium | The bypass mechanism, precedent, and stated rationale ("while the network owner measures or changes radio ownership") already exist in `src/firmware/serial.rs` for other commands; extending it is a narrow, auditable change per plan Step 3 item 1. Unconfirmed that this command family was actually in flight during the 15,156 event (see Gap). |
| B | Reduce release `rx_queue_size` from 2 to 1 | ~1,700 (half the documented 3,400-byte two-packet excursion) | Low — not recommended | This is the RX buffer count that exists specifically to avoid the Wi-Fi zero-discovery blackout this codebase has a dedicated regression gate for (`docs/guides/wifi-regression-gate.md`); reducing it risks reintroducing that class of failure. A closely related experiment (AMPDU-off, E-0037, evidence no longer present in current docs but referenced verbatim in `src/firmware/net/wifi/backend_esp_radio.rs:37-40`) was already tried, reached only a 13,376-byte low-water in a ten-cycle BLE-release soak, and was "closed as insufficient" — negative prior evidence against tuning this knob further without new justification. |
| C | Reconcile `REQUIRED_OFF_FREE_BYTES`/target numbers between code and plan/ADR (part 1's discrepancy) | 0 (not a byte recovery) | High | Not a capacity recovery candidate, but a precondition for scoring any candidate correctly in Step 2 — recovering 2,252 bytes satisfies the plan's stated target but not the code's actual gate. |
| D | Confirm `large_alloc_internal_ok == 0` holds on the exact current artifact (upload/SD buffers still PSRAM-only) | 0 if confirmed; otherwise the size of whichever buffer fell back (up to `HTTP_RX_BUF_TARGET_BYTES` for the RX buffer) | High confidence this is currently true (matches all captured runs to date) | Zero-cost verification via existing telemetry (`large_alloc_internal_ok`/`large_alloc_fail` counters); recommended as a pass/fail check folded into the Step 3 device run rather than a standalone change. |

No first-party owner in this source tree was found allocating *avoidably* from the Internal heap
beyond items A and D; the SD/upload/FatEngine candidates the plan asked about are already PSRAM-backed
in this exact commit (ruled out in part 2), and B is a known-bad prior experiment rather than a fresh
candidate. This narrows plan Step 2's candidate order: item 1 ("shorten or resequence a first-party
transient lifetime") maps directly to overlap A; items 2–3 (PSRAM relocation, new vendor RX ownership
mechanism) have no identified applicable target here since the PSRAM-eligible owners are already
relocated and RX queue depth is a known-bad lever; item 4 (floor-revision proposal) is plausible only
after overlap C is resolved, since the "current reserve exceeds the protected workload" comparison
requires knowing which floor number (16,384/17,408 or 20,496/21,520) is actually being compared.

Result: capacity model complete at source/doc scope per plan Step 1. Estimated ceiling of the single
identified actionable overlap (A) is up to ~1,500 bytes, transient and only when a `METRICS`/ordinary
serial command coincides with the low-water window — short of the plan's own 2,252-byte target under
the plan's stated floor, and further short of the 6,364-byte gap implied by the code's actual
`REQUIRED_OFF_FREE_BYTES` gate (see part 1 discrepancy). No candidate identified here is sufficient by
itself; Step 2 selection should treat overlap A as one input among several, likely alongside a
floor-revision review (candidate 4) once overlap C is resolved, and should not proceed on the
1,228/2,252-byte framing without first reconciling it against 5,340/6,364.

Next action: select one recovery candidate per plan Step 2, informed by the ranked overlaps above, and
record the choice — owner, expected recovery, expected timeline change, and the observation that would
disprove it — in a new evidence entry before implementation begins.

### CAP-0003 — First device read on the exact current commit: one manual radio-handoff cycle

- Date: 2026-08-14
- Build: HEAD `33061989bf9c39b41ac1ccdb4bc18f2bba761bdb` (docs-only ahead of CAP-0001/0002's
  `77569d33...`; `git diff 77569d33...HEAD -- src/` is comment-only in
  `src/platform/inkplate/waveform.rs` and `src/firmware/ui/TODO.md` — no functional or memory-layout
  change, confirmed before relying on this build). `ble-release` profile, `CARGO_FEATURES=ble-foundation`,
  `MEDITAMER_FIRMWARE_BUILD_ID=cap-diag-001`. ELF `target/xtensa-esp32-none-elf/ble-release/meditamer`
  SHA-256 `484c1ec09b5f4e3f1eae3163f688a564da06bfdb9365bfca5e0c262390a35cb4`. Board port
  `/dev/cu.usbserial-2110`.
- Evidence kind: one interactive device session, not the formal `hostctl test ble-phase1s` 20-cycle
  gate (that harness hard-requires `--cycles >= 20`,
  `tools/hostctl/src/workflows/ble_phase1s/setup.rs:124-126`). This entry is diagnostic evidence toward
  closing CAP-0002's Gap, not a P1S-A4 pass/fail run. Full flash + 8s boot capture via
  `hostctl flash-capture` (`logs/ble_phase1s_capacity_diag_20260814/capture.log`) confirmed a clean
  boot first (no panics, `RUNTIME_READY` reached). Interactive commands were then sent with a
  throwaway `serialport`-crate probe (`tools/hostctl/examples/serial_probe.rs`, deleted after this
  entry — not part of the product or tool surface) because a plain `stty`/`cat` open of the port
  produced framed garbage on this host; matching `SerialConsole::open`'s settings (DTR/RTS held low
  on open) fixed it. Full transcript: `logs/ble_phase1s_capacity_diag_20260814/handoff_probe1.log`.
- Sequence: `NETCFG SET` (credentials from the user's local `.env.local`, not recorded here) → `NET
  START` → Wi-Fi associated to the configured SSID at ~22.0s (scan took ~11.5s of the ~15.5s to
  association; this network was not the fastest-associating candidate in range) → `RADIOHANDOFF
  ACQUIRE <boot> 1` sent at 25.0s, **while DHCP was still pending** (`listener_gate reason=no_ipv4
  wifi_connected=true link_up=true` at 22.235s, no lease yet at ACQUIRE time) → accepted and quiesced
  at 26.4s → `RADIOHANDOFF RELEASE <boot> 1` sent 4s later, Wi-Fi began reassociating as expected.

Measurement, at the moment the coordinator's `floor_ok` check passed
(`src/firmware/net/runtime.rs:239-273`):

```
RADIO_HANDOFF state=off_confirmed kind=quiesced reason=none boot=2239310966 epoch=1
  internal_free=59608 block_above_reserve=31672 probe_before=59608 probe_after=59608
  probe_reserve=16384 http=0 sd_roundtrip=0 sd_session=0 callbacks=0 queues=0
  source_active=false callback_admission=false stable=true
```

- 59,608 bytes aggregate free and a 31,672-byte largest block above the 16,384 reserve — both
  comfortably clear not just the plan's headline 16,384/17,408 numbers but also CAP-0002's corrected
  20,496/21,520 gate (`REQUIRED_OFF_FREE_BYTES`/`REQUIRED_BLE_ALLOCATION_BYTES`), by roughly 3x on the
  aggregate figure.
- This is nowhere near the historical 15,156-byte low-water. It does not reproduce or refute that
  figure — it establishes a new, honestly-scoped reference point: **an idle-ish Wi-Fi station
  (associated, no active HTTP/SD/upload traffic, DHCP not even complete) clears the real gate with
  ~39,000 bytes of margin over CAP-0002's 20,496 target.**

Result and effect on the capacity model: this is the first piece of concrete evidence that the
15,156-byte event is very unlikely to be explained by station-RX packet buffers or serial-dispatch
allocation alone at baseline load — both of those owners (CAP-0002 part 2) are present in this run
too (RX queues were live per the earlier `serving` snapshot showing `queues=2`) and the result is
still 59,608, not anywhere near 15,156-21,520. **This shifts weight away from CAP-0002's overlap A
(serial-dispatch bypass, ~1,500 bytes) as a sufficient fix and toward the historical low-water
requiring genuine concurrent load** — an in-flight HTTP upload and/or SD roundtrip at the moment of
acquire, consistent with plan Step 1's "upload connection setup... buffers... SD work" owners, which
this session did not exercise. Reproducing 15,156 (or something near it) plausibly needs an ACQUIRE
issued while an actual `/assets` upload and SD write are in flight, not just a connected station.

This does not close CAP-0002's Gap for the historical figure itself, but it does answer one open
question from CAP-0002 part 2: with product work genuinely idle at the moment of acquire (matching
the coordinator's own `ownership_quiescent` precondition), the floor is not remotely at risk on this
exact commit. The risk, if it is real, is load-dependent.

Next action: before selecting a Step 2 candidate, reproduce the low-water under load — trigger
`RADIOHANDOFF ACQUIRE` while an `/assets` HTTP upload and/or SD write is genuinely in flight (or as
close to it as the coordinator's `ACTIVE_OPERATION_GRACE`/`FORCED_ABORT_GRACE` drain window allows),
and record `internal_free`/`block_above_reserve` at that moment. That result — not this idle one —
should decide whether overlap A is worth implementing or whether attention belongs on the upload/SD
buffer path instead.

### CAP-0004 — Under-load radio handoff: apparent full-system hang, not a memory reading

**Severity: this supersedes the memory-recovery question as the immediate blocker.** The load
reproduction CAP-0003 called for did not produce a clean `internal_free` reading under load — it
surfaced an unhandled error path that left the device producing zero serial output of any kind
(including the always-on `tap_trace` touch stream) until the next hardware reset. This looks like a
full-system hang, not a rejected handoff.

- Date: 2026-08-14
- Build/artifact: identical to CAP-0003 (HEAD `33061989...`, ELF SHA-256
  `484c1ec09b5f4e3f1eae3163f688a564da06bfdb9365bfca5e0c262390a35cb4`, `ble-release` /
  `ble-foundation`, `MEDITAMER_FIRMWARE_BUILD_ID=cap-diag-001`). Same board, `/dev/cu.usbserial-2110`.
- Evidence kind: one interactive device session, same throwaway probe as CAP-0003, extended to spawn a
  real `hostctl upload` (`tools/hostctl/examples/serial_probe.rs`, again deleted after this entry — not
  part of the product or tool surface) of a 4 MiB local test file
  (`.../scratchpad/cap_load_test.bin`, content irrelevant, `dd if=/dev/zero`) against the device's own
  `/assets` HTTP listener as soon as it bound, then firing `RADIOHANDOFF ACQUIRE` ~4s into that
  transfer so the coordinator would see genuinely in-flight HTTP/SD ownership. Full transcript:
  `logs/ble_phase1s_capacity_diag_20260814/handoff_load_probe1.log`; upload-client-side log:
  `/tmp/cap_upload_child.log` (not preserved under `logs/` — recreate by rerunning if needed).

Sequence (probe-clock ms, single boot/session):

```
23701  upload_http: upload_mem stage=request_begin ...
23720  upload_http: request method=PUT path=/upload
23726  sd_upload: begin path=/assets/cap_load_test.bin expected_size=4194304
24186  >> RADIOHANDOFF ACQUIRE 2089621671 1                      (~460ms into the SD write)
                                                                   -- 12,082ms of nothing --
36268  sd_upload: abort remove failed temp_path=/assets/HCTLUPLD.TMP result=Error(Fat(ClusterChainTooLong))
36271  upload_http: waiting for NETCFG credentials over UART      (network owner re-entering a fresh epoch)
36285  upload_http: listener_gate reason=wifi_down wifi_connected=false link_up=true config_ipv4=192.168.114.40 ...
36289  upload_http: waiting for dhcp ipv4 lease
                                                                   -- total silence from here on --
44222  >> RADIOHANDOFF RELEASE 2089621671 1                       (sent; no reply, no echo, nothing)
```

The probe kept the port open and listening until its own 110s timeout; after 36289ms it received
**zero further bytes of any kind** for the remaining ~74 seconds — not a `RADIO_HANDOFF_ACK`, not a
reply to `RELEASE`, not one of the multiple-times-per-second `tap_trace` touch lines that had been
continuous in every single capture so far in this ledger (CAP-0001 through CAP-0003, and this same
session's own first ~36 seconds). `tap_trace` is produced by an unrelated task; its disappearance
means this is not a narrowly-scoped network-owner stall, it reads as every task on the device going
silent together.

- The 12,082ms gap between `ACQUIRE` and the abort log line matches `ACTIVE_OPERATION_GRACE` (12s,
  `src/firmware/net/runtime.rs:43`) almost exactly — consistent with `drain_product_work`
  (`src/firmware/net/runtime.rs:560-608`) waiting the full grace period because
  `product_work_quiescent()` never went true, then `finish_product_quiescence()`
  (`src/firmware/net/runtime.rs:652+`) forcing `storage::upload::abort_sd_upload()`.
- The abort's temp-file removal (`src/firmware/storage/sd_task/upload/stream/finish.rs:96-139`) got
  back `FatResult::Error(Fat(ClusterChainTooLong))` from `run_fat_request(FatRequest::Remove{...})`
  instead of `Done`/`NotFound`, logged it, and returned normally through `upload_result(false, ...)` —
  that function itself does not hang. The hang, if it is a hang, is downstream: in whatever consumes
  this abort result on the SD-task/HTTP-owner side, or in a lock the FAT error path left held (a
  `FatEngine`-scoped mutex is the natural suspect, since SD is a single shared owner and every other
  task going quiet together is more consistent with a globally-visible stuck lock or executor stall
  than with several independent tasks failing the same way at once). This entry does not trace further
  than that; a dedicated investigation is needed and is out of this plan's scope.
- **The device recovered on the next hardware reset.** A fresh port-open (which resets this board, see
  CAP-0003) produced a normal clean boot afterward, including a **successful** SD probe this time
  (`SDDONE id=0 op=probe status=ok code=ok ... fs=fat32 size_gib=7.49`, vs. `power_on_failed` on every
  earlier boot in this session) — so the hang did not corrupt the running firmware image or brick
  anything, and is not obviously an SD-card-absent condition.
- **Not yet checked: whether `/assets/HCTLUPLD.TMP` is left on the physical SD card in the bad
  cluster-chain state the FAT driver reported.** If so, further uploads to this card may keep hitting
  the same `ClusterChainTooLong` result until it's removed out-of-band (e.g. `fsck`/reformat, or a
  dedicated repair path) — worth checking before reusing this card for further Phase 1S testing.

Result and effect on the capacity model: CAP-0003's plan ("reproduce 15,156 under load") did not
complete — it hit something that looks strictly worse. Per ADR-0011's resource acceptance table and
this plan's own acceptance criteria ("no resets, panics, allocator mismatches, ownership leaks... 20
Phase 1S handoff cycles"), an apparent hang triggered by an ordinary ACQUIRE-during-active-upload
sequence is a correctness/liveness blocker independent of the byte-count question CAP-0001–0003 were
chasing. Continuing to size a memory-recovery candidate against a coordinator that may hang under the
exact condition Phase 1S is supposed to handle (an in-flight product operation at handoff time) would
be scoring the wrong problem.

Next action, superseding CAP-0003's: **investigate this hang before any Step 2 candidate work.**
Concretely: (1) confirm the SD card's on-disk state (does `/assets/HCTLUPLD.TMP` still exist / is the
cluster chain actually damaged, or was `ClusterChainTooLong` a FAT-engine-side misread of otherwise-
valid state); (2) reproduce with `RUST_LOG`/SD-domain logging turned up to see whether the SD task's
request loop or a specific lock is where things actually stop; (3) once the mechanism is understood,
decide whether it is a pre-existing condition on this card (unrelated to CAP-0002/0003's owner map) or
a genuine handoff/abort-path bug in the source imported at commit `77569d33...`. Either way this
belongs in the parent [BLE implementation ledger](ble-foundation-ledger.md) as well, since it bears on
Phase 1S readiness generally, not only capacity.

### CAP-0005 — Hang confirmed and reproduced deterministically; FAT layer and one candidate ruled out

Follow-up investigation of CAP-0004/F-0008 on the same board and artifact. Four more interactive
sessions, same throwaway `serialport`-crate probe pattern (built, used, deleted each time — never
committed). Full transcripts: `logs/ble_phase1s_capacity_diag_20260814/handoff_hang_repro{2,3,4,5}.log`.

**1. The FAT `ClusterChainTooLong` error itself is not the cause.** Reproduced the identical error
directly and in isolation with `SDFATRM /assets/HCTLUPLD.TMP` (no Wi-Fi, no handoff involved): it
returned `rm_error ... err=ClusterChainTooLong` in **12ms**, and the device stayed fully responsive
immediately afterward (`SDFATLS` right after worked normally). The FAT engine's own driver loop
(`src/firmware/storage/sd_task/engine_driver.rs:107-182`) yields every 8 state transitions or 1ms
(`FatStep::Continue` handling) and `advance_free`'s error return
(`packages/sdcard/src/fat/engine/mutate/chain_free.rs:11-13`) is a same-step, no-I/O bail — confirmed
by direct measurement to be bounded and non-blocking. Whatever hangs, it is not this.

**2. The hang is real, deterministic, and reproduces on demand — not a one-off.** Two independent
sessions (`repro2`'s and this entry's `repro5`) hit the exact same terminal sequence:

```
sd_upload: abort remove failed temp_path=<dir>/HCTLUPLD.TMP result=Error(Fat(ClusterChainTooLong))
upload_http: waiting for NETCFG credentials over UART
upload_http: listener_gate reason=wifi_down wifi_connected=false link_up=true config_ipv4=<ip> ...
upload_http: waiting for dhcp ipv4 lease
                                          -- zero bytes from the device, ever again --
```

reproduced both times by sizing the upload so the transfer cannot finish inside
`ACTIVE_OPERATION_GRACE` (12s) before `RADIOHANDOFF ACQUIRE` lands: a 10 MiB file
(`dd if=/dev/zero`, `cap_load_test_big.bin`) with `ACQUIRE` sent ~1.1s after the coordinator reached
`Serving` (confirmed live via `http=1 sd_roundtrip=1 sd_session=1` in the preceding `RADIO_HANDOFF`
snapshot) reliably outlasts the grace window. A 4 MiB file with `ACQUIRE` sent early enough behaves
the same way (original CAP-0004); sent late enough for the transfer to finish first (this entry's
`repro3`), it does not — the coordinator completes a clean `quiesced`/`off_confirmed` cycle instead,
confirming the trigger is specifically "grace expires while genuinely still writing," not "any
concurrent upload."

**3. Decisive liveness test: the device does not respond to new input in this state.** In `repro5`,
8 seconds after the terminal sequence above (43,094ms), a fresh `NETCFG SET` was sent on the same
still-open serial connection that had gotten a clean `NET OK op=config_set` for the identical command
earlier in the very same boot (6,594ms). **Zero bytes were received in response, and zero bytes were
received for the remaining ~62 seconds the probe kept listening.** A live serial command dispatcher
acknowledges `NETCFG SET` within milliseconds (confirmed twice in this same session). This rules out
"legitimately busy/retrying and would resume on its own" — the command-processing loop is not running.
**This is a genuine hang**, not an unusually slow but eventually-successful recovery. (CAP-0004's
softer "apparent full-system hang" language is now confirmed, not merely suspected.)

**4. One candidate mechanism was checked and ruled out by direct code reading.**
`acknowledge_control_quiescence()` (`src/firmware/net/wifi.rs:288-297`) was a plausible deadlock site
(the freshly-recreated `run_wifi_connection_task` calls it as its very first loop action after
"waiting for NETCFG credentials"), but it returns immediately when
`WIFI_CONTROL_QUIESCE_REQUESTED` is false — and `wifi::cancel_control_quiescence()`
(`src/firmware/net/runtime.rs`, called unconditionally right before the retry loop re-enters) clears
exactly that flag beforehand. So this specific function cannot be the blocking `.await`. Not yet
checked: why `run_wifi_connection_task` prints "waiting for NETCFG credentials" (`state.credentials`
is `None`) on what should be a same-epoch retry carrying the same `runtime_config.credentials` captured
once at `run_network_epoch` entry (`src/firmware/net/runtime.rs:299-330`) — that discrepancy implies
either a full new epoch started from `network_owner_task`'s outer loop (plausible if
`finish_product_quiescence()` returned `true` despite `abort_ok=false`, took the `EpochResult::Quiesced`
path, and then failed the settled `floor_ok` check on `http_connections`/`sd_roundtrips` not being
fully zero — looping back through `network_owner_task` with a fresh `wifi::current_runtime_config()`
read) or a mechanism this entry did not trace. This is the next concrete lead, not yet resolved.

**5. Separate, confirmed, standalone bug: aborted uploads permanently poison that directory for future
uploads.** `SDFATLS /assets` after the original CAP-0004 session showed `HCTLUPLD.TMP` present with
`size=0`, surviving a full power-cycle. A second upload attempt to the same directory
(`repro2`/second session) failed immediately client-side with `500 Internal Server Error sd operation
failed` at `/upload_begin`, before writing any data — no hang this time, just a clean, instant, total
failure. `SDFATRM` on that file reproduces `ClusterChainTooLong` and cannot remove it. Since the temp
filename is fixed per parent directory (`SD_UPLOAD_TMP_BASENAME = "HCTLUPLD.TMP"`,
`src/firmware/storage/sd_task.rs:41`, joined via `parent_len` in
`src/firmware/storage/sd_task/upload/path_ops.rs:161-176`), **one aborted-mid-grace upload permanently
blocks every future upload to that exact directory** until the corrupted entry is cleared by some means
outside the exposed serial API (confirmed: `SDFATRM` cannot do it). Uploading to a fresh, never-used
directory sidesteps this (used for `repro3`/`repro5`) but is not a fix.

Result: F-0008 is upgraded from "apparent full-system hang" to confirmed. The capacity-recovery block
in the Status table stands. This entry narrows the search space (FAT layer clean; one specific await
ruled out; exact log boundary identified) but does not identify the actual blocking call. A full root
cause needs either a debug build with task-level introspection (e.g. an RTOS task-list dump, if one
exists) or instrumenting `run_network_epoch`'s retry path with additional print points around
`finish_product_quiescence()`'s return value and whichever code path re-reads `current_runtime_config()`
next, then re-running this same reproduction (`repro5`'s exact parameters: fresh directory, ~10 MiB
file, ACQUIRE sent ~1s after reaching `Serving`).

**Update (CAP-0011, 2026-08-14): the `HCTLUPLD.TMP` per-directory poisoning found in item 5 above is
root-caused and fixed.** It was not genuine on-disk corruption — `free_remove_chain_or_delete`'s
chain-removal step budget was derived from the directory entry's on-disk `size`, which stays 0 for the
whole life of an in-progress upload (`UploadBegin` pre-allocates and links the full expected-size chain
up front but only persists `size` at `UploadCommit`), so any aborted-mid-write chain longer than the old
`size(0)+32` budget was misdiagnosed as `ClusterChainTooLong`. See
[CAP-0011](#cap-0011--cap-0005-root-caused-and-fixed-chain-free-step-budget-derived-from-a-stale-directory-entry-size-not-the-actual-chain-length)
and [CAP-0012](#cap-0012--cap-0011-fix-hardware-verification-pre-fix-repro-post-fix-prevention-and-poisoned-file-recovery)
for the fix and its hardware verification, including recovery of the exact `/assets/HCTLUPLD.TMP` this
entry poisoned.

### CAP-0006 — Correction: not a hang. Bounded (~130s) self-recovery; exact root cause identified

**This corrects CAP-0004/CAP-0005's "confirmed hang" finding. It was not a hang.** The device recovers
fully and deterministically on its own, without a reset, in roughly 130 seconds — bounded by an
existing timeout, not open-ended. CAP-0005's liveness test was too short to see this: it waited at most
~74s after the trigger, well under the ~130s this actually takes. F-0008 should be read alongside this
entry, not as a standing "hang" finding.

- Date: 2026-08-14
- Method: added five temporary `println!` markers (`DBGHANG` prefix) at the exact decision points
  CAP-0005 left unresolved — `run_network_epoch`'s retry-loop entry (with
  `runtime_config.credentials.is_some()`), `finish_product_quiescence()`'s entry/return and
  `abort_sd_upload()`'s return, `run_wifi_connection_task`'s unconditional entry, and
  `network_owner_task`'s call/return around `run_network_epoch` — built as `ble-release`/
  `ble-foundation` (`MEDITAMER_FIRMWARE_BUILD_ID=dbghang-001`), full-flashed, and reproduced with the
  same `repro5`-style trigger (fresh directory `/assets/dbg1`, 10 MiB file, `ACQUIRE` sent ~1s after
  `Serving`) but with the host-side probe run for ~260s instead of ~100s. All five print sites were
  reverted (`git checkout --`) immediately after this session; none reached committed source. Full
  transcript: `logs/ble_phase1s_capacity_diag_20260814/dbghang_repro1.log`.

**What the markers showed, in order:**

```
 22992ms  >> RADIOHANDOFF ACQUIRE
 35040ms  DBGHANG service_result_is_ok=true          (drain accepted the abort)
 35102ms  sd_upload: abort remove failed ... ClusterChainTooLong
 35108ms  DBGHANG abort_sd_upload done abort_ok=false  (correctly propagated, not swallowed)
 35116ms  DBGHANG finish_product_quiescence returned false
 35121ms  DBGHANG retry prep done, looping back to recreate services
 35127ms  DBGHANG inner_loop iteration credentials_some=false   <-- the actual defect
 35133ms  DBGHANG run_wifi_connection_task ENTER credentials_some=false
 ...      -- 102 seconds of legitimate, bounded internal retrying with no SSID to try --
137161ms  DBGHANG service_result_is_ok=false          (await_restoration timed out: "restore readiness deadline")
137240ms  DBGHANG finish_product_quiescence returned true   (nothing left in flight now)
137245ms  DBGHANG outer_loop run_network_epoch returned result_is_ok=false
137254ms  RADIO_HANDOFF state=restoring ... reason=restore_failed setup=restore readiness deadline
137291ms  RADIO_HANDOFF_ACK kind=rejected reason=restore_failed ...   (our original ACQUIRE finally acknowledged)
139259ms  DBGHANG inner_loop iteration credentials_some=true   (fresh epoch, config re-read correctly)
152180ms  upload_http: wifi connected                  (full recovery)
154323ms  RADIO_HANDOFF state=serving kind=rejected reason=quiescence_timeout ...
218031ms  upload child exited status=ExitStatus(unix_wait_status(0))   (a second, unrelated upload succeeds normally)
```

**Root cause, precisely.** `run_network_epoch` reads `wifi::current_runtime_config()` exactly once, at
epoch entry (`src/firmware/net/runtime.rs:307`, before this investigation's revert), and captures its
`.credentials` into `runtime_config`, a value never reassigned for the rest of the function. That
snapshot is correct at the moment it is taken. The bug is what happens on **retry within the same
epoch**: when `finish_product_quiescence()` returns `false` (exactly what happens here, correctly,
because the FAT abort genuinely failed), the loop at lines 333-404 recreates
`wifi::run_wifi_connection_task(&mut controller, runtime_config.credentials, ...)` using that same
frozen, epoch-entry snapshot — discarding whatever live credential state the connection task had
actually been using (this session's snapshot was taken at boot, before `NETCFG SET` ever arrived, so it
is `None`; the first connection succeeded only because `run_wifi_connection_task`'s internal state
machine picks up runtime config updates dynamically after starting, via
`apply_pending_runtime_policy_updates`/similar — machinery this stale reseed bypasses entirely). The
freshly reseeded task has no SSID to try, sits idle printing "waiting for NETCFG credentials over UART"
(confirmed correct, not a hang — it genuinely has none), and nothing moves until
`await_restoration`'s own `restoration_timeout` (clamped to `[RESTORE_MIN_TIMEOUT_MS, RESTORE_MAX_TIMEOUT_MS]`
= `[15_000, 180_000]` ms, `src/firmware/net/runtime.rs:47-48,703-710`) finally expires with
`"restore readiness deadline"`. Only then does `run_network_epoch` return an `Err`, letting
`network_owner_task`'s outer loop call it again — which re-reads `current_runtime_config()` fresh and
gets the correct, current credentials, exactly as this entry's markers show.

**Severity, corrected.** This is a real, reproducible bug — but a **bounded stall of up to
`RESTORE_MAX_TIMEOUT_MS` (180s worst case; ~130s observed here), not a hardware-reset-requiring hang**.
Wi-Fi and the upload listener come back fully on their own; a subsequent, unrelated upload completed
cleanly with no anomaly (`exit 0`) at the end of this same session, and the original ACQUIRE gets a
correct, well-formed rejection ack once recovery completes. This still fails ADR-0011's resource
acceptance bar in spirit (an unbounded-feeling multi-minute Wi-Fi outage triggered by a routine
BLE-handoff-adjacent abort is not acceptable product behavior) but it is a straightforward, well-scoped
fix, not an open-ended reliability investigation.

**Proposed fix** (not yet implemented; needs review before landing): re-read
`wifi::current_runtime_config()` (or otherwise thread through the connection task's live, most-recent
credentials rather than the epoch-entry snapshot) each time `run_network_epoch`'s retry loop recreates
`run_wifi_connection_task`, not only once at function entry. The narrowest fix is scoped to the retry
site (`src/firmware/net/runtime.rs`'s loop body); a broader one could remove the frozen
`runtime_config` capture entirely in favor of always reading live config at each connection-task
(re)creation. Either should be validated by rerunning this exact reproduction and confirming recovery
happens within a few seconds of the abort rather than ~130s.

**Also still true and unaffected by this correction:** the `HCTLUPLD.TMP` per-directory poisoning found
in CAP-0005 is real, confirmed, and separate from this bug — it is not caused by or dependent on the
credentials-reseed issue, and needs its own fix (unique temp filenames, or a cluster-chain repair path).

Next action: propose this fix for review, implement it, and rerun the exact `dbg1`-style reproduction
to confirm recovery drops from ~130s to a few seconds. Once landed, F-0008 can close and the Phase 1S
capacity-recovery block can lift — the capacity model (CAP-0001–CAP-0003) is otherwise unaffected by
this finding and its Step 2 candidate selection can resume once this fix lands and is verified.

### CAP-0007 — Fix implemented and verified: retry reads live config, not a stale snapshot

- Date: 2026-08-14
- Change: `src/firmware/net/runtime.rs` — moved `let runtime_config = wifi::current_runtime_config();`
  from `run_network_epoch`'s function-entry scope into the top of its retry `loop`, so every
  iteration (the first pass and every retry after `finish_product_quiescence()` returns `false`)
  re-reads live config instead of reusing the value captured once at epoch entry. Minimal, single-site
  diff (net: -1/+8 lines, one relocated statement plus a comment). No other files changed.
- Validation before hardware: `CARGO_FEATURES=ble-foundation scripts/build/build.sh ble-release` and
  plain `scripts/build/build.sh release` both build clean; `scripts/ci/check_ble_controller_patch.sh`
  and `scripts/ci/check_network_owner_source.sh` pass; `scripts/ci/check_software_baseline.sh
  firmware-builds` (locked default/BLE/minimal/slim/telemetry/all-feature) and `firmware-clippy` both
  pass with no new warnings.
- Device validation: full flash (`MEDITAMER_FIRMWARE_BUILD_ID=fix-verify-001`), then the exact
  CAP-0006 trigger reproduced twice more (fresh directories `/assets/fix1`, `/assets/fix2`, same 10 MiB
  file, `ACQUIRE` ~1s after `Serving`). Transcripts:
  `logs/ble_phase1s_capacity_diag_20260814/fix_verify_repro{1,2}.log`.

**Result, both runs:** `NET_EVENT {"from":"Idle","to":"Scanning",...}` fires **14ms** after the abort
error, both times — versus the pre-fix ~102s of idle "waiting for NETCFG credentials" silence in
CAP-0006. `grep -c "waiting for NETCFG credentials"` on both transcripts returns exactly 1 (the
legitimate boot-time occurrence before `NETCFG SET` is ever sent); it never reappears after the abort.
The reconnecting scan correctly finds and selects the configured SSID every
time — proof the retry has real credentials, not none.

**A confound worth recording honestly:** both verification runs then hit an unrelated ~42.7s
`Associating → Recovering (connect_timeout)` step before a driver-restart/rescan completed the
reconnect — nearly identical timing in both independent runs, which reads as the AP holding stale
association state from the abrupt pre-fix teardown rather than random radio flakiness. This is a
distinct, pre-existing part of the Wi-Fi driver's own recovery ladder (`ladder_step=DriverRestart`,
`watchdog_start_reason=connect_timeout`) — nothing to do with the credentials-snapshot bug this entry
fixes, and not touched by this change. Total wall time to `RADIO_HANDOFF_ACK`/`kind=serving` was
~73-114s in these two runs specifically *because of* that separate, pre-existing timeout — not because
credentials were lost. With that confound stripped out (i.e., on a clean scan/associate/DHCP cycle,
which this session measured at consistently ~12-14s elsewhere), the fix's actual contribution is
recovery beginning in single-digit milliseconds rather than after 102+ seconds of dead time, which is
what the defect actually was.

**What is not yet touched by this fix, and remains open:** the `HCTLUPLD.TMP` per-directory poisoning
(CAP-0005) — confirmed unrelated and unaffected; and the AP-session-timeout confound noted above, which
this session did not investigate further (plausibly a missing/late deauth on the abrupt abort path, but
that is a hypothesis, not confirmed).

Result: F-0008's root defect (stale credentials snapshot on same-epoch retry) is fixed and verified
2/2. The capacity-recovery block in the Status table can lift for this specific finding; Step 2
candidate selection may resume. The separate `HCTLUPLD.TMP` issue remains open and unfixed.

### CAP-0008 — Consolidated live-load aggregate reading; six independent samples

Resuming the capacity model after F-0008's fix (CAP-0007). Before selecting a Step 2 candidate, this
entry consolidates an aggregate-memory data point that was actually captured six separate times as a
side effect of CAP-0004 through CAP-0007's device sessions, but never previously pulled together and
read as capacity evidence in its own right.

- Date: 2026-08-14
- Source: the `RADIO_HANDOFF state=serving kind=restored` line every one of CAP-0004–0007's six device
  sessions printed at the moment the coordinator considered the epoch newly `Serving` with an upload
  already accepted (`http=1 sd_roundtrip=1 sd_session=1`) — i.e., genuine early-upload load, not idle.
  Six independent boots, five different builds (pre-fix instrumented, pre-fix plain, and post-fix),
  three different SD directories:

  | Run | internal_free | http/sd_roundtrip/sd_session |
  | --- | ---: | --- |
  | `handoff_hang_repro3` | 24,748 | 1/1/1 |
  | `handoff_hang_repro4` | 24,964 | 1/1/1 |
  | `handoff_hang_repro5` | 24,964 | 1/1/1 |
  | `dbghang_repro1` | 24,964 | 1/1/1 |
  | `fix_verify_repro1` | 24,964 | 1/1/1 |
  | `fix_verify_repro2` | 25,156 | 1/1/1 |

- All six cluster tightly at 24,748–25,156 bytes (range: 408 bytes), against the idle-state readings
  from the same sessions and CAP-0003, which cluster at 24,748–27,764 while `http=0`. The ~35,000-byte
  drop from CAP-0003's fully-idle 59,608-byte reading to this ~25,000-byte early-load reading is the
  first concrete measurement of what accepting one HTTP connection plus opening one SD upload session
  actually costs on the internal heap, on this exact commit.
- Against the plan's originally-stated 16,384/17,408-byte numbers, this clears with 8,364–8,772 bytes
  of margin. Against CAP-0002's corrected, code-accurate 20,496/21,520-byte gate, it clears with
  **4,252–4,660 bytes of margin** — smaller, but still positive, and consistent across six independent
  boots rather than a single sample.
- **Important limitation: this is an aggregate-only reading, not a contiguity one.** These snapshots
  come from `resource_snapshot(false)` (the non-probing path) — `block_above_reserve` reads back `0` in
  all six not because contiguity is bad, but because the largest-block probe never runs for this
  snapshot kind (`src/firmware/net/runtime.rs`'s `resource_snapshot`: `probe_largest_block: false` skips
  `psram::probe_internal_block_above_reserve` entirely). No live-load contiguity measurement exists yet;
  the only probed largest-block readings on record are CAP-0003/CAP-0007's fully-idle ones
  (27,672–31,672 bytes above reserve).
- This also does not resolve CAP-0002's Gap: it is a snapshot at the *start* of upload activity (one
  HTTP connection, one SD session just opened), not a sustained-load low-water measurement across a full
  transfer or repeated cycles. The historical 15,156-byte figure could still reflect deeper-into-transfer
  pressure (buffer accumulation, fragmentation over repeated chunks) that a snapshot at t+0 of the
  session wouldn't show.

Result: on the exact current commit, early-load aggregate internal-heap pressure is real and
measurable (~35,000-byte drop from idle) but the margin against the corrected floor is still positive
and reproducible across six samples — a materially better picture than the inherited 15,156-byte figure
implied, though not a like-for-like comparison (different branch, different load profile, no
contiguity data, single-point-in-time rather than sustained/repeated).

Next action: this changes what Step 2 should optimize for. Given six consistent samples already show
positive (if narrower) margin at upload start, the higher-value next step is not necessarily
implementing a byte-shaving candidate (CAP-0002's overlap A recovers ~1,200–1,500 bytes — a real but
now less urgently-needed cushion) but running the plan's own formal Step 3 validation
(`hostctl test ble-phase1s --cycles 20` against this exact fixed artifact) to get a real sustained-load,
repeated-cycle, low-water-tracked, contiguity-probed measurement — the kind of evidence CAP-0002's Gap
noted was missing entirely. That result should decide whether any capacity candidate is still needed at
all, and if so, which one.

### CAP-0009 — Formal 20-cycle gate: passed clean. No capacity candidate needed.

- Date: 2026-08-14
- Command: `HOSTCTL_PORT=/dev/cu.usbserial-2110 cargo run --manifest-path tools/hostctl/Cargo.toml --
  test ble-phase1s --artifacts logs/ble_phase1s_gate_20260814 --board-id cap-recovery-dev-01 --cycles 20
  --output logs/ble_phase1s_gate_20260814/report.json`, run against a clean, committed exact artifact —
  full flash of `MEDITAMER_FIRMWARE_BUILD_ID=cap-gate-001` (`ble-release`/`ble-foundation`) built from
  commit `9606e152e816215449486f286cb400bc52d08bab` (the CAP-0006/CAP-0007 fix, source-clean at build
  time — `git_status_begin`/`git_status_end` empty in the artifact's own recorded metadata). ELF SHA-256
  `dfe62434c45578070a9919ae64fa7365108ee9387b1ef16b779b110f7823c261`. Full report:
  `logs/ble_phase1s_gate_20260814/report.json`; raw run log: `logs/ble_phase1s_gate_20260814/gate_run.log`.
- This is the formal `hostctl` workflow the plan's Step 3 and the parent ledger's P1S-A4/A5 criteria
  call for — schema-versioned, artifact-identity-checked, 20 full acquire/BLE-window/release cycles each
  with a real upload before and after, not an ad hoc reproduction.

**Result: `gate_passed: true`, `violations: []`, `completed_cycles: 20`, `completed_ble_cycles: 20`.**

| Metric | Value | Requirement | Margin |
| --- | ---: | ---: | ---: |
| Off-state internal free (all 20 cycles, identical) | 59,608 | >=20,496 (`REQUIRED_OFF_FREE_BYTES`) | +39,112 |
| Off-state largest block above reserve (all 20, identical) | 31,672 | >=4,112 (`REQUIRED_BLE_ALLOCATION_BYTES`) | +27,560 |
| `minimum_serving_internal_free_bytes` (Wi-Fi active, lowest of 20) | 19,896 | >=16,384 (ADR-0011 hard floor) | +3,512 |
| `minimum_ble_active_internal_free_bytes` | 43,996 | >=16,384 | +27,612 |
| Post-warmup free drift | 0 | <=1,024, non-monotonic | 0 (perfectly stable) |
| Post-warmup largest-block drift | 0 | <=1,024, non-monotonic | 0 |
| CPU0 stack headroom (min of 20) | 16,696 | >=8,192 (>=12,288 target) | clears target too |
| Touch-core stack headroom (min of 20) | 3,300 | >=1,024 | +2,276 |
| UART overflow during gate | 0 | 0 | exact |

- The off-state reading (59,608 / 31,672) is bit-for-bit identical across all 20 cycles — no
  cycle-to-cycle drift, no fragmentation accumulation, confirming CAP-0003/CAP-0007's single-sample idle
  readings were representative, not lucky.
- `minimum_serving_internal_free_bytes: 19,896` is the low-water for the whole gate, and it clears the
  historical 15,156-byte failure by 4,740 bytes and the ADR's 16,384-byte hard floor by 3,512. It
  carries its own provenance: `minimum_serving_internal_alloc_wifi_rx_matched: true`,
  `..._charge_bytes: 1700`, `..._correlation_stable: true` — the low-water is attributed to a single
  ~1,700-byte vendor Wi-Fi RX packet allocation, matching CAP-0002's owner-map hypothesis and the
  `INTERNAL_HEAP_DRAM2_BYTES` doc comment's own historical per-packet estimate almost exactly.
- This 19,896-byte reading is the aggregate ADR floor (16,384), not the stricter BLE-start gate
  (20,496) — that stricter gate applies only at the off/pre-BLE-start moment, which every one of the 20
  cycles cleared at 59,608. The two floors protect different moments and this result satisfies both
  where each applies.

Result and effect on the capacity model: **the plan's core acceptance bar for Phase 1S capacity is met
on the exact current commit, with the credentials-retry fix and zero capacity-specific code changes.**
None of CAP-0002's ranked candidates (serial-dispatch bypass, PSRAM relocation, new RX ownership
mechanism, floor revision) were implemented or needed. The historical 15,156-byte failure that opened
this entire plan does not reproduce on this source tree; CAP-0002's Gap (no local evidence ever
captured 15,156) turns out to have been the right thing to flag skeptically, and CAP-0008's early-load
sampling correctly predicted the direction of this result, if not its exact margin.

**What is not yet covered by this entry:** the plan's acceptance list also names the separate
[Wi-Fi regression gate](../guides/wifi-regression-gate.md) (`scripts/tests/hw/test_wifi_regression_gate.sh`)
as a required, independent check on the exact artifact — not run in this session. The `HCTLUPLD.TMP`
per-directory poisoning (CAP-0005) and the AP-session-timeout observation (CAP-0007) remain open,
unrelated to capacity, and did not block or affect this gate's pass.

Next action: run the Wi-Fi regression gate on this same exact artifact
(`logs/ble_phase1s_gate_20260814`, commit `9606e152...`) to complete the plan's full Step 3 checklist.
Once that passes, the parent ledger's P1S-A4 criterion can close and Phase 2 reconsideration can begin,
per the plan's Acceptance section.

### CAP-0010 — Wi-Fi regression gate: discovery passes clean; acceptance blocked by CAP-0005, not a regression

- Date: 2026-08-14
- Command: `scripts/tests/hw/test_wifi_regression_gate.sh` (`HOSTCTL_NET_PORT=/dev/cu.usbserial-2110`,
  policy `tools/hostctl/scenarios/wifi-policy.default.json`), run against the same device state left by
  CAP-0009 — no reflash, same exact `cap-gate-001` artifact / commit `9606e152...`. Report:
  `logs/wifi_regression_gate_cap_recovery/report.json`; stage logs alongside it.

| Stage | Status | Duration |
| --- | --- | ---: |
| `discovery_debug` | **passed** | 71,561ms (8/8 ready rounds, `zero_discovery_rounds=0`) |
| `acceptance_1_cycle` | **failed** | 37,507ms |
| `acceptance_3_cycle` | skipped (fail-fast) | — |
| `acceptance_soak` | skipped | — |

**Discovery debug — the stage most directly tied to this gate's original purpose (the historical Wi-Fi
zero-discovery blackout) — passed cleanly.** 8 of 8 rounds ready, zero blackout events, `ssid_seen` and
scan evidence present. Nothing about the F-0008 fix regressed discovery behavior.

**Acceptance failed at the exact same failure signature CAP-0005 already documented and explained**:
`error: POST http://.../upload_begin?path=/assets/net_acceptance_payload.bin&size=524288 failed: 500
Internal Server Error sd operation failed`. `test_wifi_acceptance.sh` always uploads to `/assets`
(hardcoded), which is the exact directory whose shared `HCTLUPLD.TMP` this session's own earlier
CAP-0004/CAP-0006/CAP-0007 reproductions left permanently corrupted (`ClusterChainTooLong`, confirmed
unremovable via `SDFATRM` in CAP-0005). This is **not a new regression** — it is this ledger's own prior
testing leaving physical SD-card state that a later, unrelated test now trips over. No serial command
exposes an SD format/repair path (`SDFATRM` is the only removal primitive, and it already fails on this
entry); clearing it needs physical access to the card (reformat or swap).

Result: the plan's Wi-Fi-regression-gate acceptance item is **not yet clean**, but for a reason fully
explained by already-open evidence (CAP-0005), not a property of the F-0008 fix or the capacity
conclusion (CAP-0009) — both of those are self-contained, unaffected by this card's state, and stand as
recorded. Discovery debug, the part of this gate with the most direct historical relevance, passed.

Next action: reformat or swap the physical SD card (needs hands-on access this session did not have),
then rerun `acceptance_1_cycle` (and `_3_cycle`) on the same exact artifact to get a clean full-gate
pass. This is the same underlying fix CAP-0005 already called for (unique per-attempt temp filenames,
or a repair path) — fixing that closes both CAP-0005 and this item together.

### CAP-0011 — CAP-0005 root-caused and fixed: chain-free step budget derived from a stale directory-entry size, not the actual chain length

- Date: 2026-08-14
- Source commit (parent, pre-fix): `e9416ca046b6459799b265fbdf11a5f8ff17addd`
- Fix commit: `e0213a76cb80be5b0821e53720b9eafdf24f714f`
- Scope: `packages/sdcard/src/fat/engine/mutate/{rename_remove.rs,mod.rs}` (fix),
  `packages/sdcard/tests/fat_engine.rs` (two new host-test regressions). No firmware/`src/` changes.

**Root cause.** `UploadBegin` pre-allocates and links the *entire* expected-size cluster chain
up front (`mutation_allocate`, `packages/sdcard/src/fat/engine/mutate/chain.rs:9-28`, sized from
`FatRequest::UploadBegin`'s `expected_size` field, not from bytes actually written), but the directory
entry it writes to disk at that same moment always has `size = 0`
(`mutation_write_data`, `chain.rs:30-49`: `if matches!(self.request, Some(FatRequest::UploadBegin
{ .. })) { self.mutation.data_len = 0; ... }`). Every subsequent `UploadChunk` updates only the
in-memory `self.upload.record.size` (`mutation_update_directory`, `chain.rs:51-68`) — the on-disk
directory entry's `size` field is not touched again until `UploadCommit` writes the real final value.
So for the entire lifetime of an in-progress upload, the on-disk entry says `size=0` while its chain
already has `clusters_for_size(expected_size)` real, validly-linked clusters.

Two call sites derived a chain-removal step budget from that stale on-disk `size` instead of the
actual chain length:

1. `free_remove_chain_or_delete` (`rename_remove.rs:209-225`, reached by `FatRequest::Remove` — the
   abort path in `src/firmware/storage/sd_task/upload/stream/finish.rs:96-139`, and the `SDFATRM`
   serial command): `max_steps = clusters_for_size(found.record.size, cluster_size) + 32` — with
   `size=0`, this is always exactly 32.
2. `mutation_start`'s `Write`/`UploadBegin` overwrite-existing-chain branch (`mod.rs:136-147`, reached
   when `UploadBegin` retargets a path that already has an allocated chain — e.g. a client retrying an
   upload to the same temp path after a prior attempt was aborted): same `size`-derived budget.

Both call `advance_free` (`chain_free.rs`), whose own walk is correctly bounded and non-blocking
(confirmed by CAP-0005 item 1 — the FAT engine itself was never at fault) but bails with
`SdFatError::ClusterChainTooLong` (`chain_free.rs:11-13`) as soon as `self.free.steps >= self.free.max_steps`.
For any upload whose real chain exceeds 32 clusters (512-byte clusters on the test volume: any upload
over 16 KiB; on the physical card's real cluster size, any upload of more than a few dozen KiB), the
walk hits that 32-step ceiling long before reaching the chain's real end and returns
`ClusterChainTooLong` on a chain that is not actually corrupted — it is simply longer than a budget that
was never a valid estimate of it. This matches every observed symptom: `SDFATLS`/`SDFATSTAT` reporting
`size=0` for the poisoned file (CAP-0005 item 5), the abort failing (`finish.rs`'s `Remove` request),
and a same-path retry failing immediately at `upload_begin` (call site 2) with no data written.

This answers the task's root-cause question directly: **(c)**, a bug in the chain-free logic's step
budget — specifically, a budget computed from a field (`size`) that this exact code path (in-progress
upload) never keeps in sync with the real chain length — not (a) genuine on-disk corruption from
cancelling a write mid-flight, and not a deeper bug in `chain_free.rs`'s own walk/free mechanics, which
CAP-0005 item 1 already showed to be a bounded, non-blocking, same-step bail.

**Fix.** Both call sites now bound the walk by `volume.total_clusters.saturating_add(2)` instead of a
size-derived estimate — the same bound `free_remove_chain_or_delete` already used for directory removal
in this exact function. This is a safe upper bound for any non-cyclic chain on the volume (files and
directories alike): a valid chain cannot have more links than the volume has clusters, so anything
that exceeds this bound genuinely is a cycle (real corruption), and anything within it that is merely
long is walked and freed correctly, regardless of what `size` says. This is a root-cause fix, not a
dodge: it does not change the temp filename scheme (still shared per directory, per
`SD_UPLOAD_TMP_BASENAME`), does not add an out-of-band repair path, and does not touch the abort path's
control flow — it only replaces an incorrect step-budget estimate with a correct one at both sites that
had it.

**Host-test verification.** Two new tests in `packages/sdcard/tests/fat_engine.rs`, against the crate's
existing `FakeDisk`-backed FAT32 harness (`host-tests` feature):

- `aborting_a_large_preallocated_upload_before_commit_does_not_poison_the_temp_path`: `UploadBegin`
  with a 50-cluster (25,600-byte) `expected_size`, no chunks written, then `Remove` before commit —
  asserts `FatResult::Done`, then asserts a fresh `UploadBegin` on the same path also succeeds.
- `retrying_upload_begin_over_a_large_unfinished_chain_does_not_poison_the_temp_path`: same setup but
  exercises call site 2 directly — a second `UploadBegin` on the same path with no intervening `Remove`.

Both were confirmed to **fail** with `FatResult::Error(FatEngineError::Fat(SdFatError::ClusterChainTooLong))`
against the pre-fix code (verified by `git stash`-reverting only the two fix files and rerunning —
both tests panicked on the exact assertion that a `Remove`/retry `UploadBegin` succeeds) and to **pass**
once the fix is restored. `cargo test --features host-tests --test fat_engine` (`packages/sdcard`):
13/13 pass, including the two new regressions and all 11 pre-existing tests (no regression).

**CI checks**, all clean on the fix commit: `scripts/ci/check_ble_controller_patch.sh`,
`scripts/ci/check_network_owner_source.sh`, `scripts/ci/check_software_baseline.sh firmware-builds`
(all six locked build lanes), `firmware-clippy` (minimal + all-features, zero new warnings), and
`host-tests` (full `scripts/host-test.sh test all` run, all suites including `sdcard` pass — see the
`sdcard` section for the `fat_engine`/`retry_policy` results above).

Result: CAP-0005's root cause is identified and fixed at the source. Next action: verify on the
physical device — reproduce pre-fix on a fresh directory, confirm the fix prevents recurrence, and
attempt recovery of the already-poisoned `/assets/HCTLUPLD.TMP` this ledger's own earlier sessions left
on the physical card. See CAP-0012.

### CAP-0012 — CAP-0011 fix: hardware verification (pre-fix repro, post-fix prevention, and poisoned-file recovery)

- Date: 2026-08-14
- Board: `/dev/cu.usbserial-2110` (same physical card and board as CAP-0003–CAP-0010).
- Evidence kind: five interactive device sessions using two throwaway `serialport`-crate probes
  (`tools/hostctl/examples/serial_probe.rs` for the NETCFG/RADIOHANDOFF/upload-timed reproduction,
  `serial_cmd.rs` for fixed SDFAT* command sequences), same pattern as every prior CAP-000x hardware
  entry — built, used, and deleted before this entry's commit; not part of the product or tool surface.
  Session transcripts went to the session scratchpad, not `logs/` (ephemeral, same as some artifacts
  noted in CAP-0003/CAP-0004); the flash boot captures referenced below are preserved under `logs/`.

**1. Pre-fix reproduction on a fresh, never-used directory.** Temporarily reverted only the two fixed
files (`git stash` on `rename_remove.rs`/`mod.rs`, keeping the new tests), built/flashed
`ble-release`/`ble-foundation` with `MEDITAMER_FIRMWARE_BUILD_ID=cap-pre-fix-repro-001` (ELF SHA-256
`29af6ad67d6fde9ce528744650c7f9d2a3b10de4614904a88d53be09e0324596`; clean boot,
`logs/cap_pre_fix_repro_flash/capture.log`). Reproduced CAP-0004/CAP-0006's exact trigger against a
brand-new directory: NETCFG SET/NET START, a 10 MiB `hostctl upload` to `/assets/prefix1/`, `RADIOHANDOFF
ACQUIRE` sent ~1s after the SD session opened (well inside `ACTIVE_OPERATION_GRACE`, guaranteeing the
transfer is still in flight when grace expires).

Result: identical failure to CAP-0004/CAP-0005/CAP-0006's original discovery, on a directory this
session created for the first time:

```
sd_upload: abort remove failed temp_path=/assets/prefix1/HCTLUPLD.TMP result=Error(Fat(ClusterChainTooLong))
```

Confirmed permanently poisoned exactly as CAP-0005 item 5 described: a follow-up `SDFATSTAT
/assets/prefix1/HCTLUPLD.TMP` showed `size=0`; `SDFATRM /assets/prefix1/HCTLUPLD.TMP` failed with
`err=ClusterChainTooLong` (`rm_error`). The credentials-retry fix (CAP-0007, unaffected by this
session's revert) still reconnected Wi-Fi correctly afterward — this run isolates the CAP-0005 defect
alone, with no other variable changed from the current source tree.

**2. Post-fix prevention on a fresh directory.** Restored the fix (`git stash pop`), rebuilt/reflashed
with `MEDITAMER_FIRMWARE_BUILD_ID=cap-fix-verify-001` (ELF SHA-256
`ef3d15812845280c2585a5eafd5ea1aab7d488b07f9309231102ca996131c214`; clean boot,
`logs/cap_fix_verify_flash/capture.log`; content-identical to fix commit `e0213a76...`, built from the
working tree immediately before that commit). Repeated the exact same trigger against a different fresh
directory (`/assets/capfix1/`, 10 MiB file, `ACQUIRE` ~1s after SD session open).

Result:

```
RADIO_HANDOFF state=off_confirmed kind=quiesced reason=none boot=986639338 epoch=1
  internal_free=59608 block_above_reserve=31672 ... stable=true
RADIO_HANDOFF_ACK kind=quiesced state=off_confirmed reason=none ...
```

No `abort remove failed` line anywhere in the transcript (the abort path only logs on failure — see
`finish.rs::handle_abort`), and the acknowledgment is the clean first-attempt `quiesced` success path,
**not** the `reason=restore_failed` retry-loop path CAP-0006/CAP-0007 observed when the SD abort still
failed pre-CAP-0011. This is the expected signature of the abort's `Remove` now returning `Done`
instead of `ClusterChainTooLong`. After the device reassociated, a fresh `hostctl upload` to the
**same** `/assets/capfix1/` directory (reusing the same shared `HCTLUPLD.TMP` temp path the aborted
attempt had just used) completed end-to-end: `Upload complete.` — direct proof the directory was not
poisoned by the interrupted upload.

**3. Recovery of both poisoned directories.** With the fix flashed (`cap-fix-verify-001`/`-002`):

- `/assets/HCTLUPLD.TMP` — the file this ledger's own CAP-0004/CAP-0005/CAP-0006/CAP-0007 sessions
  poisoned earlier today, `size=0`, confirmed unremovable in CAP-0005 and still blocking
  `acceptance_1_cycle` in CAP-0010: `SDFATRM /assets/HCTLUPLD.TMP` now returns `rm_ok`
  (`status=ok code=ok attempts=1 dur_ms=23`). A following `SDFATLS /assets` confirms `HCTLUPLD.TMP` no
  longer appears in the listing.
- `/assets/prefix1/HCTLUPLD.TMP` — the file this entry's own step 1 freshly poisoned pre-fix, on the
  same physical card, same file, same on-disk state: after reflashing the fix
  (`MEDITAMER_FIRMWARE_BUILD_ID=cap-fix-verify-002`, ELF SHA-256
  `6037ecc1ac35329871be6524624ac795086645a5a755822c861981ef333e159d`, clean boot,
  `logs/cap_fix_verify_flash2/capture.log`), `SDFATRM /assets/prefix1/HCTLUPLD.TMP` returns `rm_ok`
  (`dur_ms=42`); `SDFATLS /assets/prefix1` afterward shows `ls_ok count=2` (only `.`/`..`) — empty and
  clean.

This is a same-file, same-card, before/after comparison for the `prefix1` case: identical file,
identical corrupted-looking on-disk state, `ClusterChainTooLong` pre-fix and `rm_ok` post-fix with
nothing else on the card changed in between.

Result: all three verification requirements are met on hardware. (a) Pre-fix reproduction on a fresh
directory: confirmed, matches CAP-0004/CAP-0005's original signature exactly. (b) Fix prevents
recurrence going forward: confirmed — a forced mid-write abort under the exact CAP-0006 trigger no
longer fails the SD remove, and the directory remains fully usable for a subsequent upload immediately
after. (c) The already-poisoned card is recovered: confirmed for both the original `/assets/HCTLUPLD.TMP`
and this session's own freshly-poisoned `/assets/prefix1/HCTLUPLD.TMP` — no reformat or physical card
access was needed; `SDFATRM` now succeeds directly.

Next action: rerun the [Wi-Fi regression gate](../guides/wifi-regression-gate.md) on the fixed, clean,
committed artifact now that `/assets` is confirmed usable. See CAP-0013.

### CAP-0013 — Wi-Fi regression gate rerun: CAP-0005's SD blocker is gone; a different, unrelated internal-memory-floor failure now blocks a clean pass

- Date: 2026-08-14
- Command: `HOSTCTL_NET_PORT=/dev/cu.usbserial-2110 scripts/tests/hw/test_wifi_regression_gate.sh`
  (`.env.local` supplies `HOSTCTL_NET_SSID`/`HOSTCTL_NET_PASSWORD`/`HOSTCTL_NET_POLICY_PATH`), run twice
  against a full reflash of the fix commit `e0213a76cb80be5b0821e53720b9eafdf24f714f`
  (`MEDITAMER_FIRMWARE_BUILD_ID=cap-gate-002`, `ble-release`/`ble-foundation`, clean committed working
  tree at build time, ELF SHA-256 `def615f184808a2c88fa35ca1dbdd32e863b9f5a2c8775f454a2d2d29ce2d9cb`,
  `logs/cap_gate_002_flash/capture.log`). Reports:
  `logs/wifi_regression_gate_20260814_130620/report.json` (run 1),
  `logs/wifi_regression_gate_20260814_130958/report.json` (run 2, same device state, no reflash).

| Stage | Run 1 | Run 2 |
| --- | --- | --- |
| `discovery_debug` | **passed** (73,060ms) | **passed** (72,943ms) |
| `acceptance_1_cycle` | **passed** (50,604ms) | **passed** (30,060ms) |
| `acceptance_3_cycle` | **failed** (37,299ms) | **failed** (46,399ms) |
| `acceptance_soak` | skipped (fail-fast) | skipped (fail-fast) |

**CAP-0010's blocker is gone.** `acceptance_1_cycle` uploads to `/assets/net_acceptance_payload.bin` —
exactly the path CAP-0010 found permanently blocked by CAP-0005's poisoned `/assets/HCTLUPLD.TMP`. Both
runs now pass it cleanly (`Upload complete.`, `runtime_health_gate: ... probe_stable=true` in run 1).
This is the item this whole investigation (CAP-0011/CAP-0012) set out to unblock, and it is unblocked.

**A different failure now blocks `acceptance_3_cycle`, reproduced 2/2:**

```
error: internal memory gate failed: min_internal_free_bytes=13456 floor=16384
  min_internal_alloc_charge_bytes=1700 min_internal_alloc_internal_required=true
  min_internal_alloc_charge_overflow=false min_internal_alloc_post_free_bytes=13456
  min_internal_alloc_wifi_rx_matched=true
```

(run 2: `min_internal_free_bytes=13496`, `min_internal_alloc_charge_bytes=1660` — same signature,
nearly identical magnitude.) This is the internal-heap ADR floor (16,384 bytes) being violated during
the third of three back-to-back upload cycles within a single boot, attributed by the runtime's own
correlation mechanism to a vendor Wi-Fi RX packet allocation
(`min_internal_alloc_wifi_rx_matched=true`, ~1,660–1,700-byte charge) — the same owner CAP-0002 and
CAP-0009 already identified as the dominant contributor to the historical low-water. No panic, no
unexpected reboot (`panic_detected: false`, `unexpected_reboot_detected: false` in both `report.json`s)
— this is a clean gate-check failure, not a crash.

**This is not caused by, or related to, CAP-0005/CAP-0011's fix.** The fix is pure step-count arithmetic
in the FAT chain-free walk (`volume.total_clusters` comparisons); it performs no heap allocation and
touches no code on the Wi-Fi/network/memory path. It is also a *materially different* result from
CAP-0009's formal `hostctl test ble-phase1s` 20-cycle gate on a source-equivalent commit, which found a
19,896-byte low-water (margin +3,512 over the floor) with zero violations across 20 full cycles. The
`wifi-acceptance` workflow's specific load pattern here — three consecutive upload cycles inside one
boot, `net_apply_config`/`net_start` skipped because the network is "already ready" between cycles —
evidently drives the low-water 6,400+ bytes lower than the `ble-phase1s` gate's own cycling pattern did.
This is a genuine, reproducible finding, not a fluke (confirmed 2/2 with closely matching values), but
it is squarely CAP-0001–CAP-0009's capacity-model subject matter, not this entry's SD-storage bug, and
implementing a fix for it is out of this entry's scope.

Result: the Wi-Fi regression gate's SD-poisoning blocker (CAP-0005/CAP-0010) is resolved and verified
clean twice. **The gate as a whole does not yet pass** — `acceptance_3_cycle`'s internal-memory-floor
failure is a new, separate, reproducible finding that reopens CAP-0009's capacity conclusion for this
specific repeated-cycle load pattern (see the amended Capacity model status-table row above). The plan's
full acceptance bar (a clean Wi-Fi regression gate run) is **not** met by this entry.

Next action: this is a capacity-recovery question, not a CAP-0005 follow-up — open a new investigation
against CAP-0002's owner/lifetime map and candidate list (serial-dispatch bypass, RX queue depth, floor
revision) specifically for the `wifi-acceptance` workflow's repeated-cycle load pattern, since it
demonstrably reaches a lower low-water than the `ble-phase1s` gate's own cycling does. Once that
clears `acceptance_3_cycle` (and, if run, `acceptance_soak`), rerun this same gate command on a fresh
artifact to get the plan's full acceptance bar to a clean pass.

### CAP-0014 — CAP-0013 root-caused: a monotonic since-boot low-water register sampled over a longer, teardown-free session, not a leak

- Date: 2026-08-14
- Source commit: `347b265a6e218eb0bc2f6b9a2a4ff56a30cf8fe4` (HEAD; docs-only ahead of CAP-0013's
  `e0213a76...` — `git diff e0213a76...347b265a` is documentation-only, no `src/` change). Clean working
  tree at every build below except the throwaway probe under `tools/hostctl/examples/` (never committed;
  deleted before this entry's commit, same pattern as every prior CAP-000x hardware entry).
- Evidence kind: source review (the deciding evidence) plus three fresh-boot hardware sessions on
  `/dev/cu.usbserial-2110`, using a throwaway `serialport`-crate probe
  (`tools/hostctl/examples/cap_cycle_probe.rs`, built/used/deleted, not part of the product or tool
  surface) that interleaves real `hostctl upload` cycles with `PSRAM` allocator-status polls on one
  held-open serial connection, plus one direct run of the real `test_wifi_acceptance.sh` stage.

#### 1. Source review: `min_internal_free_bytes` is monotonic since boot, not per-session

`psram::provenance::seed_internal_low_water` (`src/firmware/psram/provenance.rs:316-328`) is the only
writer that can *raise* the tracked low-water; every allocation can only lower it
(`record_internal_low_water`/`record_internal_low_water_packed`, same file). `seed_internal_low_water`
is called from exactly one call site, `psram::init::init_allocator`
(`src/firmware/psram/init.rs:11-48`), which itself early-returns without reseeding if the allocator
state is already `Initialized` (line 12-14) — i.e., it runs exactly once per boot. Nothing in
`src/firmware/net/` or `src/firmware/net/wifi/` calls it. **This means `min_internal_free_bytes` is a
whole-boot-lifetime worst-ever reading, identical in nature whether a device has been through one Wi-Fi
session or twenty.** The two workflows differ only in what happens *inside* that boot before the
register is read.

#### 2. Source review: `ble-phase1s` forces a full Wi-Fi teardown/recreate every cycle; `wifi-acceptance` does not

Confirmed by re-reading `run_network_epoch` (`src/firmware/net/runtime.rs:299-481`, unchanged by this
entry): once the coordinator grants `StartingBle` and the BLE window later closes, `network_owner_task`'s
outer `loop` (`runtime.rs:223-296`) calls `run_network_epoch` again, which calls
`wifi::initialize_runtime_sta` and `embassy_net::new(...)` fresh — a brand-new controller, station
device, and network stack, not a reused one. Every one of `hostctl test ble-phase1s`'s 20
acquire/BLE-window/release cycles goes through this path once. Confirmed live for `wifi-acceptance`
today (`logs/cap0014_acceptance3_before/acceptance_3_cycle.log`, fresh boot, commit `347b265a`, build
`cap-0014-probe-002`):

```
net_apply_config: skip NETCFG SET because network is already ready
net_start: skip NET START because network is already ready
```
on cycles 2 and 3 — the Wi-Fi controller and stack from cycle 1 are reused unchanged for the rest of the
run. `wifi-acceptance` never exercises the teardown/recreate path CAP-0013 hypothesized; this entry
confirms that hypothesis directly from source, not just by inference.

#### 3. Hardware: no leak. Current/resting internal-free bytes is stable across repeated cycles; only the monotonic register moves

Two independent fresh-boot sessions, each a full reflash (`CARGO_FEATURES=ble-foundation`,
`ble-release`/`ble-foundation`) immediately before the run:

- **Run A** — `MEDITAMER_FIRMWARE_BUILD_ID=cap-0014-probe-001`, ELF SHA-256
  `bb9c4d4a9e1eb780a34d84bcdfc0256f41c8e9137b86291f572123190c2e1a6a`
  (`logs/cap0014_flash/capture.log`, clean boot, `RUNTIME_READY` reached, no panic). 5 upload cycles
  (524,288-byte payload, real `hostctl upload` CLI subcommand, `Auto` transport, to `/assets/cap_probe/`)
  with a `PSRAM` poll immediately after each upload and again 3 seconds later ("settled"). Serial
  transcript: session scratchpad (ephemeral, not preserved under `logs/`, same as some artifacts in
  CAP-0003/CAP-0004 — the numbers below are transcribed directly from it).

  | Point | internal_free (current) | min_internal_free (register) | charge | wifi_rx_matched |
  | --- | ---: | ---: | ---: | --- |
  | baseline (pre-cycle-1) | 25,280 | 23,444 | 136 | false |
  | cycle 1 immediate | 25,156 | 18,312 | 1,660 | true |
  | cycle 1 settled | 24,968 | 18,312 | 1,660 | true |
  | cycle 2 immediate | 23,268 | 16,436 | 1,660 | true |
  | cycle 2 settled | 24,968 | 16,436 | 1,660 | true |
  | cycle 3 immediate/settled | 24,968 | 16,436 | 1,660 | true |
  | cycle 4 immediate/settled | 24,968 | 16,436 | 1,660 | true |
  | cycle 5 immediate/settled | 24,968 | 16,436 | 1,660 | true |

- **Run B** — `MEDITAMER_FIRMWARE_BUILD_ID=cap-0014-probe-002`, ELF SHA-256
  `2e61d8f740fd7ea70321f2aa7d65e181ed2fa217be4df8337c004b96b908dae9`
  (`logs/cap0014_flash2/capture.log`, clean boot, `RUNTIME_READY` reached, no panic). Identical to Run A
  except a bare `METRICS` command (matching `wifi-acceptance`'s own `assert_upload_metrics` step, which
  is *not* in the allocation-free `low_overhead_diagnostic` bypass — see part 4) was sent once per cycle,
  immediately after the upload, before the `PSRAM` polls — testing CAP-0002's overlap A directly.

  | Point | internal_free (current) | min_internal_free (register) | charge | wifi_rx_matched |
  | --- | ---: | ---: | ---: | --- |
  | baseline (pre-cycle-1) | 25,280 | 21,880 | 1,700 | **true** |
  | cycle 1 immediate/settled | 23,456 / 25,156 | 18,476 | 1,700 | true |
  | cycle 2 immediate/settled | 25,156 / 25,156 | 18,476 | 1,700 | true |
  | cycle 3 immediate/settled | 25,156 / 25,156 | 18,232 | 1,700 | true |
  | cycle 4 immediate/settled | 25,156 / 25,156 | 18,232 | 1,700 | true |
  | cycle 5 immediate/settled | 25,156 / 25,156 | 16,776 | 1,660 | true |

**Two findings, read together:**

- **No leak.** In both runs, once the resting internal-free value is established (cycle 1), it is
  bit-identical at every subsequent "settled" reading through cycle 5 (24,968 in Run A; 25,156 in
  Run B). If anything were failing to release between cycles — an HTTP connection, an SD session
  artifact, a queue registration — resting free bytes would trend downward cycle over cycle. It does
  not, in either run. This directly rules out CAP-0002 candidates 1 and 2 (shorten a transient lifetime;
  relocate to PSRAM) for a target that doesn't exist here — nothing is being retained.
- **The monotonic register still finds new lows, then stops.** In both runs the register drops during
  the first one or two cycles (matching a single ~1,660–1,700-byte vendor-RX-packet-sized charge each
  time, `wifi_rx_matched=true` — the same owner CAP-0002/CAP-0009 already identified), then holds flat
  for the remaining three to four cycles. Run B's baseline (before any upload) was *already*
  `wifi_rx_matched=true` — a bad coincidence can land at essentially any point once Wi-Fi RX traffic is
  flowing, not specifically "the 3rd cycle." This is consistent with recording the minimum of a growing
  number of independent, similarly-distributed draws (an RX packet's ~1,660–1,700-byte charge landing at
  an unlucky moment relative to whatever else is transiently resident on the internal heap) — exactly the
  behavior a monotonic low-water register produces over a longer sample, with no accumulation required.

Neither run reached CAP-0013's exact 13,456–13,496 or this same day's fresh `acceptance_3_cycle` rerun
(15,116, below); the real workflow's extra per-cycle traffic (a `net_verify_once` GET beyond the upload
PUT, `net_wait_ready_loop`'s extra status polling, and an initial `boot_discovery_gate` scan/associate
sequence this probe did not replicate) plausibly gives it more independent draws at the same coincidence
before its one `assert_runtime_health` check fires. That gap is not fully explained here; see the
Gap note below.

#### 4. Hardware: CAP-0002's overlap A tested directly — no confirmed benefit, and a relocation risk that outweighs it

Run B's per-cycle `METRICS` command (1,200-byte `InternalValue`, per
`src/firmware/serial.rs:109-129` — confirmed *not* in the `low_overhead_diagnostic` bypass list at
`serial.rs:153-193`) did **not** reproduce a materially lower floor than Run A's `METRICS`-free
cycling (16,776 vs. 16,436 — within run-to-run noise, and Run B's was marginally *higher*, i.e. better).
This is direct hardware evidence against CAP-0002 overlap A being the dominant driver of CAP-0013's
gap, correcting that entry's "Medium confidence" framing downward.

Separately, and independent of the (weak) hardware signal: relocating `METRICS`/`METRICSNET`'s command
future from `InternalValue` to `ExternalValue` (PSRAM) — the only way to remove this allocation from the
internal heap without inlining it into `run_low_overhead_diagnostic_command` and growing that function's
generated state machine (and therefore the CPU0 stack footprint of every low-overhead command, including
ones on the BLE radio-handoff hot path) — was evaluated and rejected. `InternalValue`'s own doc comment
(`src/firmware/psram/internal_value.rs:9-12`) states its purpose is futures "that may execute while the
flash cache is disabled"; PSRAM is unavailable under exactly that condition (ADR-0009). This entry did
not trace far enough to prove whether `METRICS`'s command future can ever be polled during a firmware-
update cache-disabled flash write (ADR-0009 describes the writer parking "the other core," which may or
may not exclude a UART/serial task sharing that core) — and given the hardware evidence above already
argues against a meaningful benefit, resolving that open safety question was not worth the risk for a
speculative, unconfirmed gain. No change was made.

RX-queue-depth reduction (`rx_queue_size` 2→1) remains the known-bad lever CAP-0002 already flagged
(negative prior evidence, regression risk for the Wi-Fi zero-discovery blackout this repo has a
dedicated gate for). No other avoidable internal-heap owner was identified.

#### 5. Confirming rerun of the real workflow, same fresh boot as Run B

Immediately after Run B, on the same boot (`cap-0014-probe-002`, no reflash — confirmed by
`BOOT_RESET reason=Some(ChipPowerOn)` appearing only once, before Run B, in the combined session):
`HOSTCTL_NET_CYCLES=3 scripts/tests/hw/test_wifi_acceptance.sh` was run directly. Fresh boot confirmed
independently in its own log
(`logs/cap0014_acceptance3_before/acceptance_3_cycle.log:36`: `BOOT_RESET reason=Some(ChipPowerOn)
code=1`, `internal_free_bytes=68736` — opening hostctl's serial connection resets this board, matching
CAP-0003/CAP-0005's established finding; this was therefore an independent fresh-boot run, not a
continuation of Run B's already-lowered state).

```
error: internal memory gate failed: min_internal_free_bytes=15116 floor=16384
  min_internal_alloc_charge_bytes=1700 min_internal_alloc_internal_required=true
  min_internal_alloc_charge_overflow=false min_internal_alloc_post_free_bytes=15116
  min_internal_alloc_wifi_rx_matched=true
```

Third independent reproduction of CAP-0013's finding (CAP-0010's two runs, now this one), same
signature, same ~1,660–1,700-byte vendor-RX-packet charge, on today's exact HEAD. `discovery_debug` and
`acceptance_1_cycle` passed first (not shown here; same as CAP-0013).

#### 6. Why this does not appear to be a product-safety issue

The coordinator's actual BLE-admission gate (`floor_ok` in `network_owner_task`,
`src/firmware/net/runtime.rs:239-273`) never reads `min_internal_free_bytes` at all. It calls
`settled_off_resource_snapshot()`, which re-probes *current* internal free bytes and the current largest
contiguous block above the reserve, specifically at the post-quiescence moment
(`resource_snapshot(true)` → `psram::allocator_memory_snapshot().free_internal_bytes` and
`psram::probe_internal_block_above_reserve`). CAP-0009 already measured this real gate directly, 20/20
cycles, at 59,608 bytes free / 31,672-byte largest block — roughly 3x the actual 20,496/4,112-byte
requirement, with zero drift. `wifi-acceptance`'s `assert_runtime_health` step instead compares the
monotonic *historical* register against the same 16,384-byte number, which — per parts 1–3 above — is
mechanically guaranteed to trend lower as a continuous, teardown-free session runs longer or does more
cycles, independent of whether any actual BLE-open attempt would be at risk.

**Gap.** This entry does not fully close the numeric distance between this investigation's probes
(16,436–18,232 by cycle 5) and the real workflow's own reproduced floor (15,116 in 3 cycles here;
13,456–13,496 in CAP-0013). The most likely remaining differences — `boot_discovery_gate`'s own
scan/associate activity before cycle 1, and `net_verify_once`'s per-cycle GET beyond the upload PUT —
were identified but not isolated by a dedicated experiment; both are consistent with "more independent
draws before the one health check fires," not a new mechanism, but this is not proven to the same
standard as parts 1–3.

Result and effect on the capacity model: CAP-0013's finding is root-caused as expected behavior of a
monotonic since-boot diagnostic register sampled over a longer, continuous-session workload — not a
leak, not an accumulation, and (per part 6) not evidence that a real BLE-open attempt would be at risk
under this load pattern. No genuine, safely-scoped, first-party byte-recovery fix was found; CAP-0002's
overlap A was tested directly and does not clear the bar for implementation (part 4). Per this plan's
own instructions and ADR-0011's explicit human-acceptance requirement for any floor change, this entry
does **not** propose or make a floor revision. Whether `acceptance_3_cycle`/`acceptance_soak`'s
comparison of this specific register against the ADR floor is measuring the right thing for this
workload — as opposed to, say, re-probing current free bytes the way the coordinator's own gate does, or
accepting the monotonic register's workload-length sensitivity as expected and adjusting the *test's*
floor or scope rather than the ADR's — is a decision for the user, not this investigation.

Next action: none from this investigation. If the user decides on a resolution path (floor-revision ADR
amendment, changing what `assert_runtime_health` measures, or accepting the gate as documented and
currently non-clean), implement and verify that decision, then rerun the full Wi-Fi regression gate on a
fresh artifact per the plan's Step 3/4.

## Entry requirements

Each new entry records:

- date and evidence ID;
- exact commit and artifact hashes when an artifact is involved;
- question or hypothesis;
- measurements, including aggregate internal free and largest contiguous block;
- result and its effect on the status table; and
- next action or superseded entry.
