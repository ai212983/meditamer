# ADR-0015: Split the tree into platform, board, and product axes in one workspace

- Status: Accepted
- Author: Claude/Opus5
- Date: 2026-08-19
- Amended: 2026-08-20 (Tier 1 and Tier 2 scope, and the migration order, all corrected by
  executing them — see the dated sections after "Alternatives considered")
- References: [Retain the `sdcard` package boundary](0012-sdcard-package-boundary.md),
  [UI shell and application structure](0007-ui-and-application-structure.md),
  [App catalogue and launcher](0008-app-catalogue-and-launcher.md),
  [Compiled-only UI catalogue](0013-compiled-only-ui-catalogue.md),
  [Single-production SD recovery updater](0014-single-production-sd-recovery-updater.md)

## Context

A second device has entered the picture: a Waveshare ESP32-S3-RLCD-4.2 (ESP32-S3, Xtensa LX7,
300×400 reflective LCD) alongside the Inkplate 4 TEMPERA (ESP32, Xtensa LX6, 600×600 e-paper).
It hosts **Medinote**, not a second Meditamer — a distinct product with its own UI and on-device
experience, expected to share infrastructure — SD, Wi-Fi/upload, BLE, OTA — and the launcher
*mechanism*, but not the launcher's contents or the meditation domain.

The tree today is a single `meditamer` package with no `[workspace]` section; `packages/*` and
`tools/*` are path dependencies carrying their own `Cargo.lock` files. `src/` is 51,518 lines
across 358 files. Cut against the two seams that matter: hardware-specific code is ~9,500 lines
(18%) — `src/platform/` 3,236 plus 26 hardware-coupled `firmware/` files; product UI is ~2,300
lines (`ui/screen` 1,700, `ui/overlay` 447, `ui/widget` 166); the ~39,700-line remainder is
infrastructure and mechanism.

`src/firmware/ui/shell/` (5,302 lines) is already product-neutral **by construction**. Its
`SurfaceRegistry<PROVIDER_CAPACITY, SURFACE_CAPACITY>` is generic over providers, surfaces,
tokens, and generations, and contains **zero** references to any concrete screen. ADR-0007/0008/0013
produced an app-shell SDK rather than a Meditamer launcher.

The remaining question is which infrastructure can leave the product. The coupling substrate is
`observability::`, `service_mode::`, `psram::`, `app_state::`, and the 22 `pub(crate) static`
channels in `firmware/config/channels.rs`; `observability::` alone appears in 51 `firmware/` files.
Counted by file, the candidates look similar and discouraging — `net` 23/52 files, `storage` 22/54.
Counted by **API shape**, they diverge sharply:

| | distinct symbols | writes (`set_`/`record_`) | reads | global channels used |
| --- | --- | --- | --- | --- |
| `net` | 35 | 25 | 10 | 4 |
| `storage` | 41 | 20 | 21 | 10 |

`net`'s ten reads are `service_mode::{upload_enabled, upload_http_listener_enabled}`, four
logging items (`LOG_DOMAIN_WIFI`, `LOG_DOMAIN_REASSOC`, `log_filter_enabled`, `snapshot`), the
`WifiScanPhase` type, and three `psram` memory-diagnostic calls. It references `app_state` not at
all. Its four channels — `WIFI_CREDENTIALS_UPDATES`, `WIFI_RUNTIME_POLICY_UPDATES`,
`NET_CONFIG_SET_UPDATES`, `NET_CONTROL_COMMANDS` — are its own inbound command surface, not
product channels; it touches none of `APP_EVENTS`, `SD_*`, `UI_*`, `TAP_TRACE_SAMPLES`, or
`WALL_CLOCK_*`. The dependency is therefore a narrow, near-unidirectional port: a telemetry sink
plus two policy predicates.

`storage` is a different shape. Twenty-one reads include `psram::{ExternalValue, LargeByteBuffer,
alloc_large_byte_buffer, BufferAllocError}` — allocator types in its data path, not diagnostics —
plus `app_state::read_app_state_snapshot`, four `service_mode` policy calls, and product-shaped
telemetry *types* embedded in its signatures (`NetPipelineGate`, `SdUploadRoundtripPhase`,
`UploadHttpPhaseMetrics`). Its ten channels span both SD and Wi-Fi-config domains.

ADR-0012 examined both of these under a one-product premise. It retained `packages/sdcard`
*provisionally*, passing four of five gates and failing only the fifth — that nothing outside the
firmware crate consumed it yet. It **rejected** extracting `net`/Wi-Fi on gate 2. That rejection
named the right modules, but the measurements above show it over-weighted file count relative to
API shape, and the "global config channels" it cited are in fact net's own.

The build configuration is single-chip in three places. `.cargo/config.toml` sets
`ESP_HAL_CONFIG_USE_RWDATA_LD_HOOK=true`, selecting the esp32-only
`config/linker/esp32/rwdata_hook.x`; Cargo's `[env]` table is global per invocation and **cannot**
be scoped per target. This was observed empirically here — an `xtensa-esp32s3-none-elf` crate
under `.scratch/` inherited the setting and failed to link until it was overridden locally.
`scripts/build/build.sh` parameterises `FIRMWARE_TARGET_TRIPLE` but hardcodes
`xtensa-esp32-elf-gcc` in `configure_lvgl_toolchain()`. Root `Cargo.toml` pins `features =
["esp32"]` on seven crates plus the `esp32` PAC and `esp-wifi-sys-esp32`.

`vendor/` carries seven patched crates, three under active investigation for an open
scheduler-corruption defect (ADR-0014's second addendum). `esp-rtos`, `xtensa-lx-rt`, and
`esp-backtrace` already carry `esp32s3` feature blocks, so both devices consume the same trees.

## Decision

We keep one repository and introduce one Cargo workspace organised on **three axes**:
`platform/` (product-neutral infrastructure and mechanism), `boards/` (hardware and drivers), and
`products/` (UI, UX, domain). The two products are `meditamer` (Inkplate 4 TEMPERA) and `medinote`
(Waveshare ESP32-S3-RLCD-4.2). Board and product stay separate axes though 1:1 today, so either
product's UI can be brought up on either board during development. `tools/` stays outside the
workspace as host-only crates.

Platform crates carry **no namespace prefix**, following the convention already in place — all ten
existing crates (`sdcard`, `bundle`, `otadata`, `rtc`, `hostctl`, and the `tools/` set) are
unprefixed. The layer is expressed by directory, not by crate name: `platform/{board, shell,
render, netstack, runtime}` joins `platform/{sdcard, bundle, otadata, rtc}`, which move from
`packages/` unchanged. The single rename is `net` to `netstack`, since a bare `net` reads too
generically at a call site.

We extract in tiers ordered by measured API shape. **Tier 1** — `ui/shell` and `ui/lvgl`, 8,732
lines — moves immediately and needs no decoupling. (Half of that claim did not survive contact;
see "Tier 1 as built" below.) **Tier 2** — `net`, becoming `platform/netstack`
— moves *with* its telemetry rather than behind a port for it. Of net's 35 global symbols, 26 are
net-only, and 25 of those are `set_`/`record_` writes into static atomics (`WIFI_LINK_CONNECTED`,
`WIFI_IPV4`, `UPLOAD_HTTP_LISTENING`, the `NET_PIPELINE_*` counters) that nothing reads except
`observability::snapshot`. Those counters are netstack's own state, parked in a product module for
organisational reasons only. They move into `platform/netstack`, which exposes a plain snapshot
struct that Meditamer's `observability::snapshot` composes. `set_upload_http_listener` — the one
write shared with `firmware/storage` — moves the same way and is called inward from the product.
Logging (`log_filter_enabled`, the `LOG_DOMAIN_*` constants) is infrastructure and moves to
`platform/runtime`.

That leaves one genuine inbound dependency: `service_mode::{upload_enabled,
upload_http_listener_enabled}` — two policy predicates — which netstack takes as a `NetPolicy` port
the product implements, alongside `set_radio_handoff_admission_open` outbound. Net's four inbound
channels migrate with it as its own command surface.

`packages/sdcard` becomes a platform workspace member unchanged — ADR-0012 already verified it has
zero Meditamer coupling. The firmware side, `firmware/storage/sd_task/**`, **stays with the
product**, and this is a correction of framing rather than a deferral: ADR-0012 established that
the generic FAT and probe mechanics belong in the package while wire responses, session dispatch,
and upload-session bridging belong in firmware. Its 21 reads and 10 cross-domain channels are that
product policy, correctly placed. There is no third tier of blocked work, because the
platform-worthy half of storage is already a package.

This ADR supersedes ADR-0012. It reverses that ADR's rejection of a `net` extraction on the
strength of the API-shape measurements above and of the changed premise — coupling to product
globals that ADR-0012 recorded as "by design" is a liability once a second product exists. It
carries forward ADR-0012's `sdcard` disposition and resolves its open fifth gate, since a second
product is the second consumer whose absence left that boundary's value partly theoretical.

Medinote board support is scoped as a product commitment only after the ADR-0014 scheduler defect
is understood; until then the ESP32-S3 board remains a debug vehicle.

## Consequences

### Positive

- Tier 1 is close to free and is the highest-value move: 8,732 lines of launcher and render
  mechanism become shared with no decoupling work.
- The `net` port injection removes the failure mode this structure most needed to avoid — a
  platform that cannot carry Wi-Fi, forcing Medinote to duplicate it or import Meditamer
  wholesale. Both products get one Wi-Fi/upload implementation.
- The inbound port is two predicates, not the 35-symbol surface a file-count reading suggested, and
  the hot telemetry writes stay direct inlined atomic stores. No dispatch cost lands on
  `set_wifi_link_connected` (17 sites) or `set_upload_http_listener` (18). This was the design's
  main open risk, and measuring it removed the risk rather than sizing it: a port for the write side
  would have forced `dyn` onto the hottest calls, because `net/runtime.rs` uses
  `#[embassy_executor::task]`, which a generic parameter cannot thread through.
- `NetPolicy` makes the Wi-Fi connect/retry ladder testable off-device for the first time, which is
  the independent-testability benefit ADR-0012 named as valuable but left unscheduled.
- ADR-0012's open gate 5 for `packages/sdcard` resolves.
- Refactors that move the platform/product line stay atomic while that line is still being
  discovered.
- Separating board from product lets product-2 UI work begin on Inkplate hardware before any
  ESP32-S3 driver exists, and lets the S3 board be exercised with a known-good UI to separate
  driver faults from UI faults.

### Negative

- Moving `ESP_HAL_CONFIG_*` and the LVGL/bindgen variables into per-board build scripts makes
  `scripts/build/build.sh` mandatory; a bare `cargo build` at the root will no longer produce a
  correct image. This is permanent, and forced by Cargo's inability to scope `[env]` per target.
- Moving the counters into netstack inverts an existing dependency: `firmware/storage` and
  `observability::snapshot` will call into `platform/netstack` rather than the reverse. That is the
  correct direction, but it is still churn across roughly 29 call sites, and
  `observability::snapshot`'s single unified struct becomes a composition of product and platform
  parts rather than one flat record.
- Splitting `net` from `firmware/storage/sd_task` puts a crate boundary through the upload path,
  which currently shares `psram` buffer types across that seam. The upload session bridging will
  need those buffers passed as plain slices or via a platform allocator trait.
- The repository keeps a product name while housing two products — an accepted wart; renaming
  costs remotes, CI, and every docs link for no functional gain.
- Unprefixed platform crates keep call sites short and match the existing ten, but a `use shell::…`
  does not reveal which layer it refers to. With two products in one workspace this is a real
  readability cost, accepted to avoid renaming four existing crates against a settled convention.
- One workspace lockfile means unified resolution across two chips; feature unification between
  `esp32` and `esp32s3` builds needs watching, and `sdcard`'s `host-tests` feature and
  `[target.'cfg(target_os = "none")']` pattern must be preserved rather than flattened.
- `src/lib.rs`'s `extern crate self as esp_println;` shadowing hack must be resolved before shared
  crates exist; unavoidable work in Tier 1.
- Crate count rises: more manifests, slower clean builds, `pub(crate)` churn to `pub`.
- The quality gates widen the moment the workspace exists. `scripts/build/build.sh`'s clippy step
  already passes `--workspace`, which was a no-op against a single member and now lints every
  member under `-D warnings`. Step 1 surfaced one latent failure this way
  (`clippy::manual_is_multiple_of` in `packages/sdcard/src/probe/write_multi.rs`, fixed in place).
  Each further extraction moves code from the product crate into a member the gate now covers, so
  expect more of these — a benefit in the long run, but real friction per step.
- Committing to the ESP32-S3 board as a product target inherits ADR-0014's unresolved
  scheduler-corruption defect, which is reproducible on Inkplate and not yet root-caused.

## Alternatives considered

- **Leave `net` product-side, as ADR-0012 decided:** rejected. The decision was sound under one
  product, but 25 of the 35 symbols are writes into counters netstack should own outright, and what
  remains is two policy predicates over four channels net already owns — a relocation plus a
  two-method port, not the semantic redesign the file count suggested. Leaving
  it would strand Wi-Fi, upload, and BLE in the product layer and make the platform unable to carry
  the infrastructure that motivated it.
- **Extract `firmware/storage/sd_task` alongside `net`:** rejected. Its 21 reads include allocator
  types in the data path and product state snapshots, and its channels span SD and Wi-Fi-config
  domains. ADR-0012's finding that session and wire formatting are correctly firmware-side still
  holds; extracting it would relocate coupling into a crate boundary rather than remove it.
- **Prefix the platform crates (`medi-*`, or a neutral codename such as `slate-*`):** rejected. A
  prefix would make the layer visible at every call site, which has genuine value with two products,
  and a neutral codename would survive a future product outside the current family. Both were
  outweighed by renaming four existing crates against a ten-crate unprefixed convention — and
  `medi-` would tie the platform to that family anyway. Reconsider if a product outside it appears.
- **Two product repositories plus a shared "common" repository:** rejected. The shared surface is
  roughly 80% of the tree, making the common repository the primary artifact and the products thin
  satellites. It also imposes cross-repository version skew on `vendor/`, under active bisection,
  and on `tools/hostctl`, which serves both devices. This becomes right once the platform crates
  stabilise; it is wrong while the boundary is still moving.
- **Keep one package and select the product with Cargo features:** rejected. Feature-gated product
  selection compiles both products into one namespace with no enforced boundary, and feature
  unification means nominally disabled code stays coupled through shared types and globals.
- **Defer all structural work until Medinote is specified:** rejected. Tier 1 is free *now*
  precisely because `ui/shell` has no product content in it; every screen added before extraction
  raises the cost.
- **Do nothing; let Medinote fork the tree:** rejected. A fork duplicates `vendor/` patch
  maintenance across two trees while a defect is being bisected in it, and guarantees the launcher
  mechanism diverges permanently despite being already product-neutral.

## Tier 1 as built (2026-08-20)

Executing the extraction corrected the Tier 1 estimate above. The prerequisite and the shell half
landed; the render half did not, for a reason the original measurement could not see.

**Prerequisite — `platform/console`.** `src/esp_println.rs` moved out first. The crate had aliased
itself with `extern crate self as esp_println;` so that `esp_println::println!` resolved to its own
UART arbitration rather than the upstream crate, with the real crate renamed to
`esp-println-upstream`. Exactly one crate per build can do that, so nothing could be extracted until
it was gone. 254 call sites across 51 files now say `console::println!`. Firmware `.text` fell by
18,080 bytes: `try_print` was a crate-local `pub(crate) fn` that inlined into every call site and is
now a single 299-byte cross-crate function. No IRAM hazard — the six `#[esp_hal::ram]` functions are
panel scan-out code and log nothing, and the upstream writer was always flash-resident.

**Done — `platform/shell`** (5,302 lines). Moved essentially as-is: every import stayed inside
shell, and its only dependencies are `core` and `heapless`. It builds and tests natively on the
host, which revealed that its **46 tests had never once run** — the root crate sets
`[lib] test = false`, so the `#[cfg(test)]` modules were dead code, addressing themselves through a
`crate::shell::*` path that no longer resolved. All 46 pass.

**Not done — `platform/render`.** `ui/lvgl` is not product-neutral. `backend.rs`, `io.rs`, and
`mod.rs` (982 of 3,430 lines) import the concrete Meditamer screens
(`ui::screen::{ambient_view, gesture_test, home, launcher, overlay_settings}`) and
`ui::overlay::base_overlays`. The other 2,448 lines — `backend/{cycle, frame, init, navigation,
overlay}`, `dither/*`, `intent_bridge` — carry no product reference. The earlier "1/12 files touch
globals" reading measured coupling to the *global substrate* and missed coupling to product
*modules*, which is the same class of error the `net` file-count reading made. Splitting it needs a
design decision, not a move: the screen imports should become provider registrations through the
`SurfaceRegistry` the shell already offers, so the backend takes screens as data. That is Tier 2
work, tracked separately.

One cost worth recording: shell's 337 `pub(crate)` items were promoted wholesale to `pub`, which is
coarser than the bounded contract ADR-0012's third gate asks for. It immediately turned on the
public-API lint set — eight `new_without_default` and `len_without_is_empty` errors that had been
dormant, fixed by adding the `Default` impls and `is_empty` methods a library of this shape should
have had. The surface should be narrowed to what consumers actually use once `platform/render`
settles and the real call pattern is visible.

## Tier 2 surface, remeasured (2026-08-20)

Surveying `net` before extracting it corrected the Tier 2 scope the same way executing Tier 1
corrected Tier 1's. The Decision's "one genuine inbound dependency: two policy predicates" counted
only the four substrate modules (`observability`, `service_mode`, `psram`, `app_state`) and the 22
global channels. Expanding every grouped `use crate::firmware::{…}` in `net/` surfaced two more
first-party dependencies that no earlier count had reached: `firmware::storage` and
`firmware::update`.

The good news is how concentrated they are. **Every product call in `net/` lives in one file,
`net/runtime.rs`**, and is six distinct symbols:

| Symbol | Sites | Shape |
| --- | --- | --- |
| `storage::upload::sd_upload_session_active` | 3 | read |
| `storage::upload::active_http_connections` | 2 | read |
| `storage::upload::active_sd_roundtrips` | 2 | read |
| `storage::upload::abort_sd_upload` | 1 | command |
| `storage::upload::run_http_server` | 1 | **supplies a future net awaits** |
| `update::transport_quiet` | 1 | read |

The reverse direction is a single symbol — `storage/upload/http/mem_diag.rs` calls
`net::wifi::wifi_rx_buffer_stats` — which is netstack's own statistic and moves with it, leaving the
product reading the platform. (An earlier reading of this boundary as bidirectional through
`net::Stack` and `net::tcp::TcpSocket` was wrong: those are the tails of `embassy_net::Stack` and
`embassy_net::tcp::TcpSocket`, which `storage` imports directly.)

So the port is roughly nine symbols, not two: six reads, two outbound calls, and one supplied task.
Eight of those are ordinary trait methods. `run_http_server` is not. It returns a future that
`net/runtime.rs` awaits inside an `#[embassy_executor::task]`, and an embassy task cannot be
generic, so the product cannot hand netstack a future through a generic parameter, and boxing one
would require an allocator on this path.

The resolution is to invert ownership rather than widen the port: the `#[embassy_executor::task]`
shell stays product-side and calls a generic `netstack::supervise(…)` that takes the product's
serving future as a type parameter, instead of netstack owning the task and reaching outward for
the server. That is a real design change to the Wi-Fi supervision ladder — the same machinery
ADR-0012 flagged as intricate, and the subsystem nearest the unresolved ADR-0014 scheduler defect.
Tier 2 is therefore correctly sized as design-then-extract, not relocate-plus-port, and should not
be attempted in the same pass as a mechanical move.

## Tier 2, the inversion (2026-08-20)

The ownership inversion is done and building; the file move is not. `net` no longer calls into the
product at all — the supervisor is generic over the product, and the `#[embassy_executor::task]`
that drives it lives product-side in `firmware/net_host.rs`.

The port split into two mechanisms along the call graph, rather than one trait:

- **Async work** — `serve` and `abort_upload` — is a generic `NetHost` bound. It cannot be `dyn`
  (async methods are not dyn-compatible without an allocator on this path), but it is needed in
  only two functions, so the generic threads a short way and stops.
- **Sync reads** — `active_http_connections`, `active_sd_roundtrips`, `upload_session_active`,
  `transport_quiet` — are `fn` pointers installed once at startup. They are consumed inside
  `resource_snapshot`, which has thirteen call sites across the supervision ladder; making all of
  those generic to reach four counter reads would have been more invasive than the coupling it
  removes.

`net::runtime::network_owner_task` became `net::runtime::run_network_owner<H: NetHost>`, and
`firmware::net_host::network_owner_task` is the product task that calls it with `MeditamerNetHost`.
That task is the seam: everything below it is product-neutral. Verified across all three build
configurations — default, `factory-updater` with `--no-default-features`, and the all-features
clippy gate.

**Still blocking the move to `platform/netstack`.** Expanding the grouped imports again turned up a
dependency none of the earlier counts reached — `firmware::ble::{phase1s_ownership,
Phase1sOwnership}`, four sites in `runtime.rs` under `ble-foundation`. That is the fourth time a
widened search found first-party coupling that the previous search missed, and the honest reading is
that the substrate-module list this ADR's Context is built on was never validated as complete. Any
remaining estimate in this document should be treated as a lower bound until a whole-module import
census replaces it.

Four coupling classes remain between `net` and the product, each needing its own decision rather
than a mechanical move:

| Dependency | Disposition |
| --- | --- |
| `observability` (25 counters) | move into `platform/netstack`; product composes the snapshot |
| `types::{WifiCredentials, WifiRuntimePolicy, WIFI_*}` | move with netstack |
| `config::{4 WIFI_*/NET_* channels}` | move with netstack |
| `psram` (3 diagnostics), `service_mode` (3), `ble` (2) | extend the port, or invert like the above |

## Whole-module import census (2026-08-20)

Every estimate above was built from greps for known module names. Four times running, widening the
search found first-party coupling the previous search had missed, so the estimates were replaced
with a census: every `use` path in `src/` with grouped braces expanded and `super::` chains resolved
against each file's own module path, **plus inline fully-qualified paths in code bodies**. That last
part matters — `crate::firmware::ble::phase1s_ownership()` is never imported, only called at its
full path, which is how it stayed invisible to line-oriented searches and to a first pass of the
census itself.

`net`'s complete outbound set is `types(8)`, `ble(4)`, `config(4)`, `psram(2)`, `observability(1)`,
`service_mode(1)`. The `ble` edge had never appeared in any earlier count.

**The census found seven module cycles, one of which blocks Tier 2:**

| Cycle | Symbols | Bearing on ADR-0015 |
| --- | --- | --- |
| `ble` ↔ `net` | 4 / 4 | **blocks the netstack move** |
| `touch` ↔ `types` | 8 / 8 | product-internal |
| `app_state` ↔ `config` | 6 / 1 | product-internal |
| `app_state` ↔ `types` | 1 / 3 | product-internal |
| `app_state` ↔ `scheduling` | 1 / 3 | product-internal |
| `config` ↔ `imu` | 1 / 3 | product-internal |
| `display` ↔ `storage` | 1 / 1 | product-internal |

`net` reaches `ble` for `phase1s_ownership` and its three `Phase1sOwnership` variants; `ble` reaches
back for `exclusive_lease_matches`, `phase1s_exclusive_ownership_confirmed`, `residency_snapshot`,
and `wifi::net_status_snapshot`. This is radio-ownership arbitration between the Wi-Fi supervisor
and the BLE stack — a real shared concern, not an accident, and Cargo will not accept it as a crate
cycle.

That reframes Tier 2's remaining work. `netstack` cannot move alone, because BLE and Wi-Fi jointly
arbitrate one radio.

**Decided: Wi-Fi and BLE do not merge into one crate.** Collapsing them would build a platform crate
out of two subsystems that are separate everywhere except the arbitration, and would carry the
`ble` ↔ `net` cycle across the platform boundary rather than removing it. The arbitration is
extracted instead, as the thing both depend on — roughly what `net/handoff.rs` already is, promoted
to a crate owning radio ownership, the handoff state machine, and the lease/ack protocol. `netstack`
and the BLE stack then each depend on it and neither on the other. It is the more expensive of the
two options and the correct one: it breaks the cycle instead of hiding it, and it gives Medinote the
same arbitration without inheriting Meditamer's Wi-Fi supervisor.

Sequencing follows: extract the arbitration crate first, rerun the census to confirm the `ble` ↔
`net` cycle is gone, and only then move `netstack`.

The census script is worth keeping: rerunning it before each extraction is far cheaper than
discovering an edge mid-move for a fifth time.

## platform/render, as far as it goes (2026-08-20)

Running the census at `ui`-submodule granularity before touching anything corrected the render
estimate the same way the earlier ones were corrected — and this time the correction was found
before the work rather than during it.

`ui/lvgl` holds two more cycles: `lvgl` ↔ `screen` (8/6) and `lvgl` ↔ `overlay` (4/1). They
decompose cleanly, because the product-to-lvgl direction is `intent_bridge` and `io`'s gesture
types, which is product→platform and correct once lvgl moves. Only lvgl→screen/overlay is backwards.

But the "2,448 clean lines" this document claimed for `ui/lvgl` was wrong twice over:

- `backend/{cycle, frame, init, navigation, overlay}` open with `use super::*`. They are
  impl-continuations of `backend.rs`, not independent modules, and inherit its closed `SurfaceModel`
  enum over Meditamer's six screens.
- `io.rs` is **board**-coupled, not product-coupled: it writes through `types::InkplateDriver`.
  `lvgl/mod.rs` hardcodes `WIDTH`/`HEIGHT` at 600, and `dither.rs` does the same.

So `platform/render` is seeded with what genuinely moves — `dither` (zero dependencies) and
`intent_bridge` (core, embassy-sync, lightvgl-sys, and the `shell` crate), 453 lines. Seven product
files under `screen/`, `overlay/`, and `widget/` now depend on it in the correct direction.

**What blocks the rest is `platform/board`, not a registration design.** `backend.rs` needs the
`SurfaceModel` inversion *and* board-independent geometry *and* a display trait to replace
`InkplateDriver`; `io.rs` needs the last two. The ADR's migration order put `platform/board` third,
after render. That is the wrong order: **board must come before render.** Two of render's three
blockers are board concerns, and `dither`'s hardcoded 600x600 means even the part that did move is
not yet neutral — it compiles for Medinote's 300x400 panel only by being wrong.

Also fixed while here: `shell` was never registered in `scripts/host-suites.tsv`, so its 46
newly-live tests were not gated by anything. Both new crates are registered now, and the host-tests
lane covers 49 suites. `tools/touch_replay/tests/lvgl_dither.rs` existed only to `#[path]`-include
`dither.rs` so its inline tests would run inside a `std` crate; the tests moved with the code and
that shim is gone.

## Correction: `dither` was board code (2026-08-20)

The commit that seeded `platform/render` described `dither` as product- and board-neutral. It is
not, and this section corrects the record rather than quietly moving the file.

`blit_l8` packs an LVGL L8 buffer into the destination column-major (`ROW_BYTES * x`), bottom-up,
eight rows per byte along Y (`(HEIGHT - 1 - y) % 8`). That is the ED038TH2 panel's framebuffer
layout, not a rendering algorithm; the only general thing in the module was a `luminance < 128`
threshold. It now lives at `src/platform/inkplate/panel_blit.rs`, beside the panel constants it
reads. `DirtyArea` — a plain rectangle with a `union` — was the genuinely neutral half and stays in
`render::geometry`.

**The reason it was misfiled is worth recording, because it is the same reason four earlier
estimates in this document were wrong.** Each was made by asking what a module *imports*.
`dither.rs` imported nothing at all, which is exactly why it looked like the safest thing to move.
Import graphs capture structural coupling and are blind to semantic coupling: a hardcoded memory
layout, a hardcoded 600x600, a threshold tuned for one panel's contrast. `scripts/module_census.py`
has the same blind spot by construction, so it settles *whether* a boundary can be drawn, never
*where* it should be.

The practical rule: before moving code to `platform/`, read it for constants and layout assumptions,
not just its `use` statements. A module with no imports deserves more suspicion, not less — it has
nothing to declare its assumptions with.

Splitting the crate also surfaced a real dependency error: `render` pulled `lightvgl-sys` for every
consumer, so a host harness wanting only the rectangle type dragged all of LVGL in. `geometry` is
plain arithmetic and is now reachable with `default-features = false`, behind an `lvgl` feature that
gates `intent_bridge`.

The five blit tests survive the round trip. They ran on host only because
`tools/touch_replay/tests/` `#[path]`-included them; moving them into the firmware crate, which sets
`[lib] test = false`, would have silently killed them again. `tools/touch_replay/tests/panel_blit.rs`
re-hosts them and supplies the panel geometry, until `boards/inkplate-tempera` is a crate that can
test itself.

## `platform/board` does not exist, and should not yet (2026-08-20)

Attempting it produced a better result than building it would have.

The obvious shape — one `board` crate selecting a panel through mutually exclusive Cargo features
(`inkplate-tempera`, `waveshare-rlcd42`) with a `compile_error!` guard — was written, and the
all-features clippy gate rejected it immediately. That is not a gate misconfiguration to work
around. Cargo features are additive by contract, `--all-features` is entitled to enable every one of
them at once, and any crate whose correctness depends on exactly one of N features being active is
mis-shaped. The repository's own gate proved it within one run.

The right shape is the one this ADR's Decision already names: **per-board crates under `boards/`**,
each exporting its own geometry, with the product depending on the one it targets. Nothing selects;
the dependency graph does. That cannot be built usefully against one board, and a `Panel` trait
written now would be shaped by the Inkplate alone — ADR-0012's fifth gate again, where a boundary
earns its keep only when a second consumer validates it. Medinote's 300x400 panel is that consumer,
and the crate should be created when it arrives.

What the attempt did land, which was the actual goal:

- **`ui/lvgl/io.rs` no longer names any board type.** It held `AtomicPtr<InkplateDriver>` purely to
  reach `framebuffer_bw_mut()` in the flush path. It now tracks the framebuffer directly, as a thin
  pointer plus length — `*mut dyn Trait` cannot live in an `AtomicPtr`, and this needs no trait at
  all. `io::begin` takes `&mut [u8]`; its one caller passes `display.framebuffer_bw_mut()`.
- **Panel geometry has one definition.** `ui/lvgl/mod.rs` restated `600` alongside
  `platform::inkplate`'s `E_INK_WIDTH`/`E_INK_HEIGHT`; it now reads them from the driver that owns
  them. When `boards/` exists, that import is the single line that moves.

So `render`'s three blockers are down to two, both in `backend.rs`: the closed `SurfaceModel` enum
over Meditamer's screens, and LVGL draw-buffer sizing that wants geometry in `const` position from
a board the platform layer must not name.

## `platform/arbitration`: the cycle is broken (2026-08-20)

The `ble` ↔ `net` cycle is gone. The census now reports six module cycles instead of seven, and the
two modules reference each other nowhere.

Reading the code rather than the import graph is what made it tractable. Both halves of the cycle
turned out to be **the same question asked in opposite directions** — "do you hold the radio?" The
BLE side assembled its answer from four separate reaches into the supervisor
(`exclusive_lease_matches`, `residency_snapshot`, `wifi::net_status_snapshot`,
`phase1s_exclusive_ownership_confirmed`); the supervisor asked BLE for `phase1s_ownership` and its
three `Phase1sOwnership` variants. Neither needed the other's internals. Both needed an arbiter.

`platform/arbitration` is that arbiter, and has no dependencies:

- `handoff` — the pure state machine, moved from `net/handoff.rs` (804 lines, no `use` statements).
  Four identifiers carried Meditamer's vocabulary into a model that never decided on them:
  `http_connections`, `sd_roundtrips`, `sd_sessions` are carried payload and became
  `service_connections`, `storage_roundtrips`, `storage_sessions`; `http_listener` does enter a
  predicate and became `service_listening`; and `phase1s_exclusive_ownership_confirmed` lost a
  project-phase name it had no business exporting.
- `claim` — each claimant publishes its own state and reads the composite. The supervisor publishes
  the exclusive lease, task residency, link, listener, and quiesce policy; BLE publishes its probe
  ownership after every `PHASE1D_STATE` transition. `exclusive_ownership_confirmed(boot, epoch)` is
  the single question that replaced BLE's four reaches.

`Ownership` defaults to `Unknown`, so a supervisor that has heard nothing never concludes the radio
is free.

Two things worth recording. First, the compiler caught a genuine behavioural gap mid-refactor: the
read path was rewired before the write path existed, which would have left `ble_ownership()`
permanently `Unknown` — safe, but wrong. Dead-code errors on the old accessors are what surfaced it.
Second, registering the crate as a host suite brought **15 handoff tests to life that had never
run** — they were inside a crate with `[lib] test = false` — and they immediately failed on an
incomplete rename in this very change.

Five `#[path]` harnesses broke across this work and each was repaired by depending on the extracted
crate instead of shadowing firmware source. That is the pattern to expect from every remaining
extraction, and each repair leaves the harness simpler than it was.

**This is not hardware-verified.** It changes radio-ownership arbitration, which is the machinery
that decides when Wi-Fi may be torn down for BLE — precisely the area ADR-0014's unresolved
scheduler defect lives next to. The gates prove it compiles and that the model's own tests pass; they
do not prove a real handoff still works on the device. It should be exercised on hardware before
being relied on.
