# ADR-0012: Retain the `sdcard` package boundary; no Wi-Fi package extraction

- Status: Accepted
- Author: Claude/Sonnet5
- Date: 2026-08-13
- References: [Source tree architecture cleanup](../archive/refactors/source-tree-architecture-cleanup.md) S6,
  [Stepped FAT engine](0004-dma-stepped-fat-engine.md)

## Context

The source-tree cleanup plan's S6 asks whether `packages/sdcard` and the `net`/Wi-Fi boundary should
become (or remain) standalone packages, against five gates: one lifecycle/hardware owner and a
one-way dependency; self-contained behavior relative to Meditamer globals and product
logging/wire/session policy; a small stable public contract; independent build/test/dependency value;
and a domain that survives another consumer. The plan requires the current package set and workspace
layout to stay stable through S6 — this is a decision record, not a migration.

`packages/sdcard` (`Cargo.toml:101`) depends only on `embassy-time` plus generic `embedded-hal`/`esp-hal`
crates — nothing from the `meditamer` binary crate. It exposes 88 `pub` items (`fat::FatEngine`,
`probe::{SdProbe, SdCardVersion, SdFilesystem, SdWriteMetrics, ...}`, `power`, `runtime`) and carries its
own `tests/` directory and `host-tests` feature. `src/firmware/storage/sd_task/**` (24 files) is its only
consumer; no `tools/` crate or other package depends on it. The product-specific pieces — wire responses
(`SDDONE ...`), session/command dispatch, and upload-session bridging — already live in
`firmware::storage::sd_task`, not in the package.

`src/firmware/net/wifi` is the other candidate the plan names. It imports `observability`,
`runtime::service_mode` (now `service_mode`), `psram`, and several `config::*` global channels directly
throughout its connect/retry/scan machinery — the product logging, session, and wire-protocol coupling
the gates ask about. ADR unrelated to this one, but Preserved Ownership item 5 of the cleanup plan already
commits Wi-Fi/net coordination to stay firmware runtime policy with its current authority boundaries; S1
already renamed its status module in place (`net/wifi/status.rs`) rather than extracting anything.

## Decision

Retain `packages/sdcard` as a package, provisionally, unchanged in this cycle. It passes four of the
five gates outright (owns SD SPI/DMA hardware exclusively with a one-way dependency; has zero
Meditamer-global coupling — the session/wire formatting the gates worry about is already correctly
firmware-side, not package-side; carries a bounded, already-`pub`-audited contract; has independent
tests and a `host-tests` feature proving build/test independence). It fails the fifth gate — nothing
outside the firmware crate consumes it yet — but that gate asks whether the boundary has *already*
proven itself against a second consumer, not whether one could plausibly exist; "provisionally" is the
correct disposition until one does.

Do not extract `net`/Wi-Fi into a package. It fails gate 2 outright: its connect/retry/scan machinery is
threaded through with calls into `observability`, `service_mode`, `psram`, and global config channels
that are Meditamer-specific by design, not incidental. A **pure internal Wi-Fi policy seam** — separating
the connection state machine's retry/backoff/auth-rotation decision logic (already partially isolated in
`net/wifi/connect/{retry,state_machine}.rs`) from the I/O and product-global calls around it — could have
real value as a future *internal* refactor (independent testability, a clearer read on the recovery
ladder), but that is a semantic redesign inside firmware, not a package-boundary question, and is out of
this ADR's scope.

## Consequences

### Positive

- No new package, no new workspace member, no new public-API commitment to maintain — S6 closes with
  zero structural risk, matching the plan's "package set stays stable" constraint.
- SD probe/RW session formatting's existing split (generic FAT/probe mechanics in the package, product
  wire/session formatting in firmware) is confirmed correct rather than re-litigated.

### Negative

- `packages/sdcard`'s single-consumer status remains unproven; if a second consumer never materializes,
  the package boundary's value stays partly theoretical (bounded blast radius and independent tests are
  real, but the strongest justification — reuse — is still hypothetical).
- The Wi-Fi policy-seam opportunity is named but not scheduled; it has no owner or timeline and could be
  lost to backlog drift like the plan's other deferred corrections.

## Alternatives considered

- **Extract `sdcard` fully now (workspace member with its own semver contract):** rejected — no second
  consumer exists to validate the contract, and the plan explicitly holds the package set stable through
  S6.
- **Extract `net`/Wi-Fi as a package:** rejected — fails the Meditamer-global-independence gate; the
  coupling is intentional per the cleanup plan's own Preserved Ownership commitments, not an oversight to
  fix via extraction.
- **Do nothing / record no decision:** rejected — the plan requires S6 to produce an accepted
  decision with passed/failed gates recorded, not a deferral.
