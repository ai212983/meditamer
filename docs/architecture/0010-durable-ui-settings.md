# ADR-0010: Store durable UI settings in the app-state transaction

- Status: Accepted
- Author: Codex
- Date: 2026-08-10
- References: [ADR-0007](0007-ui-and-application-structure.md),
  [ADR-0008](0008-app-catalogue-and-launcher.md),
  [ADR-0009](0009-ab-firmware-update-foundation.md),
  [UI/app rework plan](../plans/ui-app-structure-rework-plan.md),
  [implementation ledger](../plans/ui-app-structure-rework-ledger.md),
  [DRAM budget](../reference/dram-budget.md)

## Context

ADR-0009 reserves two 4 KiB sectors at `0x12000` for alternating device-lifecycle records. Phase 6
must add durable ambient binding, launcher pins, provider enablement, and startup composition without
changing the accepted A/B map or weakening the interruption-safe lifecycle transaction.

Splitting either sector between independently erased records is unsafe because 4 KiB is the flash
erase unit. Reserving one sector per owner would also remove the second valid copy needed during an
erase. No other non-overlapping flash region remains without consuming OTA metadata, a firmware slot,
or the accepted slot-capacity floor.

The current compiled catalogue has eight entries. Durable settings therefore need bounded identity
lists, not dynamic allocation. Navigation state is a separate volatile owner. The linker configuration
does not retain selected RTC memory across deep sleep, so ordinary flash persistence must not be used
to imitate short-window navigation resume.

## Decision

Upgrade the alternating `app_state` envelope from version 4 to version 5 and copy device lifecycle
plus UI settings together on every successful write. Keep their types, byte ranges, and public store
operations separate. A lifecycle save copy-forwards the current UI settings, and a UI-settings save
copy-forwards the current lifecycle state.

### Record and transaction

Each sector begins with one 128-byte record:

| Bytes | Owner | Content |
| --- | --- | --- |
| `0..12` | Envelope/device lifecycle | Magic, version, service/diagnostic fields, generation |
| `12..24` | UI settings header | Presence/configuration flags, bounded counts, ambient and startup entry ids |
| `24..56` | UI settings | Up to eight ordered launcher pin ids |
| `56..88` | UI settings | Up to eight enabled overlay ids |
| `88..120` | UI settings | Up to eight startup overlay ids |
| `120..124` | Reserved | Must remain erased in version 5 |
| `124..128` | Envelope | CRC32 over bytes `0..124` |

Writes erase and replace only the older/inactive `app_state` sector, read the record back, and accept
it only when generation, fields, and CRC decode exactly. The previous sector remains the recovery copy
until verification succeeds. UI writes debounce for 1.5 seconds, permit at most one accepted attempt
per five seconds, coalesce intervening mutations, and wait 30 seconds after a failed write before
retrying. Losing power before a deferred write deterministically retains the last committed settings.

On first version-5 boot, select the newest valid version-4 sector, preserve its lifecycle fields, add
default UI settings, and write generation plus one to the opposite sector. A failed migration leaves
the version-4 source intact and can retry. The earlier version-2/version-3 migration remains available
for direct upgrades from the single-image layout.

### Schema and resolution

Entry identities are the catalogue's stable `(namespace, local)` ids. The record holds:

- one optional ambient binding;
- an ordered list of launcher pins;
- a configured enabled-overlay set;
- one optional startup entry;
- a configured startup-overlay set.

At boot, de-duplicate each list and resolve every id against the compiled catalogue capability,
compatibility, health, residency, and registered surface. Unknown or unavailable ids are ignored. An
invalid ambient binding becomes the built-in ready ambient fallback. An invalid startup entry becomes
no startup entry, and unavailable startup overlays are omitted. An overlay must be both enabled and in
the startup set before it is created.

The shell constructs and activates the base ambient surface before applying any valid startup
composition. Settings resolution may enter only compatible, healthy, already registered providers; it
never installs provider code or changes residency. Entry or lifecycle failure returns to the base
ambient surface.

The ambient-picker and overlay-settings rows emit instance-owned settings intents through the same
bounded callback queue as navigation. The base refresh control is the current compiled overlay setting;
enabling or disabling it commits its shell/LVGL lifecycle first, then changes the deferred durable
setting. Pins and startup fields share the same schema even where product editing UI is not yet exposed.

### Volatile resume

Do not write navigation frames, active instances, provider generations, or modal state into this
record or `AppStateSnapshot`. Do not read RTC resume state while the linker and deep-sleep path do not
explicitly retain it. A cold boot therefore always starts from the base ambient path, and there is no
stale RTC value capable of causing a boot loop. A future RTC-resume decision must define retained
placement, age and compatibility identity, invalidation, and a base-ambient fallback separately.

## Consequences

- The accepted partition map and both OTA slot sizes remain unchanged.
- Lifecycle and UI settings are logically separate while sharing one physical atomic commit, matching
  the flash erase geometry.
- Every settings mutation costs one 4 KiB sector erase after debounce; coalescing and the minimum
  interval bound interactive wear.
- The envelope grows from 64 to 128 bytes and store verification uses bounded 128-byte stack buffers.
- Unknown future providers cannot block boot, but their unresolved settings are discarded when the
  sanitized state is next saved.
- Navigation resume remains absent until deep sleep has real retained-memory support.

## Alternatives considered

- **Separate lifecycle and settings records in each sector:** rejected because erasing either record
  erases the other owner and destroys independent atomicity.
- **One sector per owner:** rejected because neither owner would retain a valid copy during erase.
- **NVS:** rejected for this phase because it adds a second persistence transaction and migration
  authority while the fixed envelope already provides bounded recovery.
- **Store settings in OTA metadata or a firmware slot:** rejected because those regions have different
  boot/update owners.
- **Persist the navigation stack in flash:** rejected because it violates the lifetime boundary and
  would cause high-frequency wear.
- **Implement RTC resume now:** rejected because the current deep-sleep/linker configuration does not
  retain a chosen RTC region.
