# ADR-0013: Keep the UI catalogue compiled-only

- Status: Accepted
- Author: Codex
- Date: 2026-08-17
- References: [ADR-0006](0006-flash-overlay-app-modules.md),
  [ADR-0007](0007-ui-and-application-structure.md),
  [ADR-0008](0008-app-catalogue-and-launcher.md),
  [ADR-0010](0010-durable-ui-settings.md),
  [DRAM budget](../reference/dram/dram-budget.md)

## Context

Meditamer has no externally loaded native apps or external app catalogue. Its current five catalogue
entries are compiled into the firmware and point to surfaces registered by the base provider. The
launcher, ambient picker, and overlay settings still need one bounded index for stable identity,
capability filtering, ordering, durable selections, and current-boot entry faults.

The shell already owns two runtime lifecycles that do not require external code loading:

- providers are attached or detached from the shell registry under generation tokens;
- surface instances are entered and left while their compiled provider code remains in firmware.

ADR-0008 additionally modelled external-library source presence, executable residency, compatibility,
package integrity, and dynamic catalogue registration. No production path supplies or mutates those
states. Calling all three lifecycles "residency" obscures the ownership boundary and makes a
hypothetical loader shape the base firmware.

## Decision

Keep one fixed-capacity compiled catalogue with filtered launcher, ambient, and overlay views. Each
entry contains only its stable id, label, glyph, registered surface reference, capabilities, default
rank, optional pin, and current-boot availability (`Ready` or `Faulted`). Catalogue construction
rejects empty, oversized, duplicate-id, and invalid-fallback definitions.

The compiled catalogue is constructed as a whole and has no runtime entry-registration API. The shell
provider registry remains the sole authority for attaching and detaching compiled providers and for
rejecting stale generations. Surface enter/leave remains the sole authority for live LVGL and model
instances. These lifecycles are unchanged by this decision.

Catalogue membership is boot-lifetime configuration. A catalogue entry may reference only a surface
whose provider remains attached for that boot; the current base provider satisfies this contract.
Runtime provider fixtures and other detachable providers remain outside the catalogue. Making a
detachable provider discoverable would require a separately decided catalogue-reconstruction or
membership-update contract.

Use distinct terms for the distinct lifetimes:

- **attached/detached** for a compiled provider's shell registration;
- **live/inactive** for a surface instance;
- **installed/not installed** only if external executable providers are introduced later.

Do not model external manifests, source-card presence, executable residency, install-on-launch,
package compatibility, or package integrity in the base catalogue. ADR-0006 is no longer an active
feasibility proposal. Introducing externally supplied executable providers or declarative catalogue
entries requires a concrete product need and a new ADR defining its storage, trust, recovery,
resource, and lifecycle boundaries.

This decision supersedes only ADR-0008's external-package axes and conditional external catalogue.
ADR-0008's compiled catalogue, stable identities, filtered views, ordering, and durable-setting
integration remain accepted. ADR-0007's provider and surface lifecycles remain accepted. ADR-0010
continues to resolve durable ids against capability and current availability.

## Consequences

### Positive

- The catalogue represents only states the production firmware can reach.
- Provider teardown safety and lazy surface lifetime remain independent of app delivery.
- Each entry is smaller and catalogue action derivation has fewer branches.
- A future external-app design must justify and define its own trust and storage model.

### Negative

- External catalogue work cannot extend the current entry type without a new decision and migration.
- A statically compiled but temporarily detached provider must be handled through the provider registry;
  it cannot remain as a placeholder catalogue row.
- Historical ADR and archived-plan evidence still contains the superseded external model and must be
  read together with this decision.

## Alternatives considered

- **Remove the catalogue entirely:** rejected because three UI views and durable settings still need
  shared stable identities, capabilities, and ordering.
- **Keep the unused external state axes:** rejected because they add unreachable states and conflate
  executable installation with provider and surface lifecycles.
- **Keep dynamic catalogue registration for compiled providers:** rejected because the current
  catalogue is assembled once after base surface registration; runtime provider attachment belongs to
  the shell registry.
