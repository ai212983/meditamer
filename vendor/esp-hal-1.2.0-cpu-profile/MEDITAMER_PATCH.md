# Meditamer esp-hal patches

Based on crates.io esp-hal 1.2.0. The maintained differences from upstream are
the `cpu-profile` feature, Xtensa interrupt-dispatch callbacks,
ESP32 LACT snapshot serialization, and the ESP32-S3 flash-text alignment
section described below. Callbacks surround
individual peripheral or CPU-internal handlers; the existing outer level
wrappers account for dispatch overhead separately.

The Inkplate target enables this feature together with Embassy `trace` through
its `cpu-load` feature for the temporary CPU0 investigation. The callbacks are
implemented by `platform/diagnostics/cpu-load`; other targets do not select this fork.

Remove this patch selection and the temporary profiler after the investigation,
or replace it with an explicitly opt-in diagnostic feature before production.

## ESP32 boot literal reach (2026-09-29)

The ESP32 BLE-linked image crossed the Xtensa `l32r` reach from the
`__post_init`/`__pre_init` boot routines in flash text to their pooled
`.rwtext.literal` words in IRAM. Moving only the routines to `.rwtext` then
failed the default profile: its global-assembly literals landed in flash
instead. Each literal definition now explicitly selects `.rwtext`, and both
consumers select that section too. This keeps the small boot routines and
their literals within `l32r` reach in both profiles. Default and BLE release
links, IRAM references, and image budgets pass; physical boot still needs
qualification before treating the image as fully proven.

## ESP32 LACT snapshot serialization (2026-09-28)

Both ESP32 cores use the same `TIMG0` LACT time source. `LACTUPDATE` refreshes
the peripheral's low/high snapshot registers, but upstream `raw_counter()`
did not serialize the latch request with the subsequent two word reads. A
second core could refresh the snapshot between those reads, yielding an
incoherent 64-bit instant near the low-word rollover (every 268.435456 s at
16 MHz). A failed Mountain run retained an `embassy-time` elapsed-time
underflow marker and watchdog tick 135 (two-second cadence), placing its
stall near that boundary. This is a strong temporal match, not a hardware
trace of the exact interleaving.

`raw_counter()` now holds a dedicated reentrant `esp_sync::RawMutex` across
`LACTUPDATE` and both reads. It uses the same cross-core/interrupt-safe lock
primitive as other HAL peripherals and adds one small internal-RAM static;
the polling algorithm and clock scale are unchanged. This is a candidate fix
for torn counter reads. It does **not** explain the earlier bad task-pointer
or interrupt-frame faults and requires device qualification before promotion.

## Last-dispatched-IRQ marker (2026-09-28)

After a retained `TASK_POINTER_TRACE` showed a valid saved `THREADPTR` on
level-1 entry and `1` after dispatch, an inner-handler frame probe was tried.
That diagnostic itself faulted while dereferencing an invalid frame pointer;
its result cannot identify the original writer and the probe was removed.
The callback now passes only the interrupt ID. CPU-load records the last
dispatched ID and begin/end phase in RTC slow memory and snapshots it into
the first bad outer task-pointer record. It never dereferences the frame in
the inner callback. IDs 96 and above denote CPU-internal handlers. A later
interrupt may overwrite this marker, so it is context, not writer proof or
a fix.

After the outer midpoint confirmed corruption before the profiler exit hook,
an IRQ-26-only raw-frame-address probe was tried. Two unchanged Mountain
grids on that image faulted at the same HAL dispatcher instruction with
identical corrupt register values, but no `IRQ26_FRAME_TRACE`. The extra
callback may itself perturb the dispatcher; it was withdrawn in favor of
ID-only begin/end callbacks. This experiment cannot
attribute the original writer.

The experimental code generation brought the generic RSA work-queue's `debug!`
messages into an ESP32 IRAM interrupt path, adding three flash-rodata
references. Those two messages are now omitted on ESP32, where flash may be
unavailable during the handler. The diagnostic build returns to the prior
90-reference count; the repository's 87-reference gate remains failing.

## Value-only handler-frame probe (2026-09-29)

The first bad outer `THREADPTR` record still cannot name the writer. The
`cpu-profile` dispatcher now samples the same HAL-owned frame at handler
entry, after profiler begin, after the handler returns, and after profiler
end. Only an impossible pointer invokes the CPU-load callback, passing scalar
IRQ ID, phase, before/after pointer, PC, and SP values; the callback does not
dereference a frame. Its retained `TASK_POINTER_TRACE` stage has bit 16 set,
IRQ ID in bits 8..14, and phase in bits 0..1 (0 entry, 1 profiler begin,
2 handler return, 3 profiler end). A bad entry value is not writer proof;
a valid-to-invalid transition at phase 2 would implicate that handler's
dispatch interval. No scheduling or handler behavior was intentionally changed.
The default release links and passes its IRAM-reference and stack-risk gates.
In a controlled physical-access A/B, the probe was flashed with both current
CPU1 thread-mode and historical CPU0 IRQ input scheduling. The IRQ variant
reset by watchdog with a channel-0 watchpoint fault during exception-context
save; no invalid `THREADPTR` was observed. The same workflow passed before
and after on the thread-mode variant. See the Mountain hang gist for the
retained ELF and raw capture paths; this probe is not itself a fix.

## Spurious-interrupt guard in `mapped_to_raw` (2026-09-25)

Device A/B attribution plus a stack-guard diagnostic build showed ProCpu
prohibited-access faults (`InstrError` with PC in DRAM, `StoreProhibited`,
`InstrProhibited` with PC=1, `LoadProhibited`) minutes into radio-era
execution with zero Mountain SD activity, on both Mountain-admitted and
Mountain-suppressed images. The sharpest trace is a Rust index-out-of-bounds
panic at `core_0_intr_map[n]` (`esp32` PAC `dport.rs`), reached from the
Xtensa vectored ISR dispatcher: `should_handle` passes a raw peripheral
interrupt number from `InterruptStatus` into `mapped_to_raw`, which indexed
the mapping table unchecked. A spurious/reserved source number panics inside
the ISR dispatcher, and panicking in interrupt context escalates into the
observed fault zoo (the panic payload never prints).

`mapped_to_raw` now resolves the raw number through
`Interrupt::try_from` first and returns `None` (unmapped, skipped by the
dispatcher) for numbers outside the enum. Every `Interrupt` variant (67
variants, max discriminant 68 on ESP32) addresses the 69-entry mapping
table, so the gated index cannot panic. No new static, channel, task, or
buffer; behavior changes only for numbers that previously panicked.

Revert or upstream this once the owning subsystem (which radio bring-up step
raises the spurious source) is identified; until then this converts a fatal
ISR panic into an ignored spurious IRQ.

## Below-guard sentinel watchpoint, channel 1 (2026-09-26)

The 347 s hardware guard trip fired with a healthy shallow SP, i.e. a stray
pointer-sized write into the guard region, not a descending-stack overflow:
either a blob-`.bss` overflow ascending past `_bss_end` or a wild main-stack
write. `src/debugger.rs` now stamps the word immediately below the main
stack guard (`__stack_chk_guard - 4`, inside the guard-offset dead zone that
no correct code touches) with a canary (`!STACK_GUARD_VALUE`) and arms the
otherwise-unused Xtensa data-watchpoint channel 1 (store-only, word-sized,
same semantics as channel 0) on it, from PRO CPU init next to the existing
channel-0 arming. The Xtensa debug-exception handler discriminates by which
word changed: a channel-1 hit arrives with `DEBUGCAUSE` bit 8 set (DBNUM=1;
upstream's bits-8..11-clear check only admits channel 0), so the branch now
takes every DBREAK hit and reads both words — sentinel-only damage panics as
a below-guard ascending write (blob-`.bss` side), guard damage keeps the
existing guard message, both-intact falls through to the generic breakpoint
message. First catch (2026-09-26, ~623 s uptime): `Debug cause: 260`
(DBREAK + DBNUM=1) with a healthy SP, i.e. the sentinel fired first — the
writer ascends from below, not a descending main-stack overflow. No new
static, channel, task, buffer, or linker change; Xtensa-only, same
debugger-attached gating as channel 0.
Correction (2026-09-28): Cadence specifies that DBREAK occurs *instead of*
the matching access. Reading the watched words at the trap cannot classify
the triggering store by whether they changed; both may correctly be intact.
The earlier channel-1 observation proves an attempted store to the sentinel
only if the watchpoint registers still held their intended addresses. It
does not prove an ascending writer or rule out a wild store. After a new
channel-0 (`DEBUGCAUSE=4`) hit with both words intact, the handler was
changed to report DBNUM, word state, and all four live DBREAKA/C registers.
This removes the false generic-breakpoint classification. Channel 0 is
initially the main-stack guard but `esp-rtos` reprograms it to the running
task's guard when hardware task-overflow detection is enabled (true in this
build). Neither hit identifies the original writer yet.

## ESP32-S3 flash-text section flags

Upstream declares `.rotext_dummy` as `NOLOAD`. GNU ld 2.45 consequently marks
that address-only alignment section writable and combines it with executable
`.text`, producing an RWX load segment. Declaring the same output section
`READONLY` retains its `NOBITS` representation, address, size, and flash mapping
while making the resulting load segment RX. This is a section-flag correction;
it does not change the memory layout.
