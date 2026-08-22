# ADR-0008: App catalogue and launcher

- Status: Accepted
- Date: 2026-08-07
- Amended: 2026-08-09
- Amended: 2026-08-14 — accepted after the compiled catalogue, shared filtered views, stable
  ordering, and launcher presenters were implemented and evidenced in E-0013 of the
  [archived ledger](../archive/refactors/ui-app-structure-rework-ledger.md). The loader-dependent
  external catalogue remains conditional.
- Amended: 2026-08-17 — ADR-0013 supersedes the external-package state axes and conditional external
  catalogue. The compiled catalogue, filtered views, ordering, and durable identities remain accepted.
- References: [ADR-0006](0006-flash-overlay-app-modules.md),
  [ADR-0007](0007-ui-and-application-structure.md),
  [ADR-0013](0013-compiled-only-ui-catalogue.md),
  [UI/app rework plan](../archive/refactors/ui-app-structure-rework-plan.md),
  [UX guidelines](../product/ux-guidelines.md)

## Context

ADR-0007 gives the shell a provider registry but intentionally does not hard-code product apps into
navigation. The launcher, ambient picker, and overlay settings need a common description of what is
offered, what selecting it means, and why it may be unavailable.

The first catalogue will contain base and statically linked providers. A later catalogue may merge SD
manifests and native-provider residency, but only after ADR-0006's decision gate. Catalogue semantics
must therefore remain independent of storage and loading mechanics.

## Decision drivers

- One stable entry id must represent the same provider in every view.
- A provider may be launchable, ambient-capable, overlay-capable, or any combination.
- Library presence, residency, compatibility, and runtime health are independent facts.
- Invalid external metadata must not prevent base system entries from rendering.
- Ordering should remain calm and predictable.
- Startup and durable user choices must recover without boot loops or automatic installation.
- All manifest fields and in-memory catalogue capacities must be bounded.

## Decision

Use one catalogue with filtered views. The catalogue is a shell service; presenters render it but do
not own membership, ordering, eligibility, loading, or persistence.

### Entry model

Every entry contains bounded values for:

- stable namespaced `EntryId`;
- user-facing label and glyph reference;
- provider id and generation when registered;
- capability mask: `Launchable`, `Ambient`, `Overlay`;
- default rank and optional user pin position;
- source and state axes below.

State is not one flattened enum:

| Axis | Values |
| --- | --- |
| Source presence | `BuiltIn`, `LibraryPresent`, `LibraryAbsent` |
| Residency | `Resident`, `NotResident`, `NotApplicable` |
| Compatibility | `Compatible`, `Incompatible`, `Unknown` |
| Health | `Ready`, `Faulted`, `Corrupt`, `Unverified` |

For example, removing the SD card does not make an already verified resident provider disappear; it
changes source presence while residency remains true. A view derives its badge and allowed action
from all axes.

### Filtered views

| View | Filter | Selection |
| --- | --- | --- |
| Launcher | `Launchable` | Enter app root, or request installation if supported. |
| Ambient picker | `Ambient` | Persist the ambient binding after successful entry validation. |
| Overlay settings | `Overlay` | Enable or disable the provider after lifecycle validation. |

One entry providing several capabilities appears in several views under the same id. Overlay-only
providers do not appear in the launcher.

### Sources

The catalogue always starts with base system entries and statically linked provider registrations.
They are available without SD and cannot be hidden by external metadata.

External manifests are conditional on ADR-0006 or a future declarative-content decision. Before that
phase begins, a manifest schema must define version, maximum field lengths, capability encoding,
image/content length, compatibility identity, content hash, glyph constraints, duplicate-id policy,
and unknown-field behaviour. Untrusted lengths and glyph data are never allocated or rendered before
validation.

### Discovery and caching

Correctness never depends on a cache. A directory digest still requires a directory walk, and FAT
names, sizes, and timestamps are not a content identity. Therefore this ADR does not promise a
one-read launcher open or choose `catalogue.cache` as the production mechanism.

The external-catalogue phase must first measure card power-up, mount, directory walk, manifest read,
and rendering costs. It may then choose one of:

- bounded scan with an in-memory result for the current wake;
- host-generated authoritative index plus explicit rescan/recovery;
- checksummed cache keyed by a stronger card/library generation.

Any cache is discardable, excludes itself from invalidation identity, and falls back to base entries
plus a bounded rescan.

### Ordering

Initial order is user pins followed by stable default rank and entry id. Launching an entry does not
move it. Recency may be displayed as metadata later, but it is not an automatic ordering input unless
a subsequent UX decision adopts it.

### Startup and persistence

Durable settings own:

- ambient binding;
- pinned order;
- provider enablement;
- startup entry and startup overlays.

These settings are separate from `AppStateSnapshot` and from the volatile navigation stack. Their
storage needs versioning, checksum, atomic recovery, and write-rate limits before implementation.

Boot never installs a provider. Startup resolution follows this order:

1. validate a one-shot test override without weakening the durable setting;
2. resolve the durable startup entry and overlays against the catalogue;
3. enter only compatible, healthy, already resident or built-in providers;
4. otherwise land on the nearest valid provenance with an explanatory badge;
5. finally fall back to the base ambient provider.

A card-based one-shot override is not considered consumed until its acknowledgement is durably
recorded; exact-once behaviour is not claimed across arbitrary power loss.

### Launch and enablement

For built-in or resident entries, selection asks the shell to enter or enable the registered
provider. Entry failure preserves the current composition and marks health as faulted for the current
boot.

Install-on-launch and non-resident provider enablement do not exist until ADR-0006 is accepted. If
accepted later, installation remains a separate acknowledged transaction; only a verified commit may
change residency and expose provider code to the shell.

## Consequences

### Positive

- Launcher, ambient picker, and overlay settings share identity and eligibility rules.
- The MVP catalogue can ship without SD scanning or native loading.
- Independent state axes accurately represent resident providers whose source card disappears.
- Stable ordering avoids a launcher that moves after every use.

### Negative

- Views must derive presentation and allowed actions from several state axes.
- Durable settings need a storage decision and migration policy before customization ships.
- External discovery performance remains an evidence-driven later decision.

## Alternatives considered

- **Separate lists per view:** rejected because multi-capability providers would diverge.
- **One state enum:** rejected because source presence, residency, compatibility, and health overlap.
- **Resident-only launcher:** rejected for a future external library, but sufficient storage is not
  required for the compiled-first phase.
- **Automatic recency ordering:** rejected initially because it makes the calm launcher unstable.
- **Directory digest as content identity:** rejected because it can miss content changes and still
  requires a directory walk.

## Validation

Implementation order and gates are defined in the
[plan](../archive/refactors/ui-app-structure-rework-plan.md); run-specific evidence is recorded in the
[ledger](../archive/refactors/ui-app-structure-rework-ledger.md).
