Status: Proposed
Last-reviewed: 2026-10-01

# Gallery as a Meditamer app

## Goal

Wire the standalone SD-card image gallery
(`targets/meditamer-inkplate/src/bin/gallery.rs`) as a fifth launchable
Meditamer app, alongside Ambient View and Analog Clock, and establish the
per-app module structure the flat `screen/` layout needs before the next app
after Gallery.

## Success Criteria

- Gallery appears in the launcher catalogue and launches onto its own
  `SystemRoot` surface; swipe advances slides, tap does nothing destructive.
- Ambient View, Analog Clock, Gesture Diagnostics, and Overlay Toggles behave
  exactly as before (no rank, glyph, or ambient-fallback changes).
- New code passes the repo's own gates: host suites, firmware build/clippy,
  orphan-modules, SLOC ratchet, and link checks.
- `targets/.../bin/gallery.rs` keeps working untouched for raw-glass checks.

## Approach

One `mod` per app under `products/meditamer/src/firmware/ui/apps/`; each app
module owns its shell identity (`ENTRY_ID`, `SURFACE_ID`, `SurfaceSpec`,
`DESCRIPTOR`) and its `create()` wrapper, so `backend.rs` keeps one match arm
per operation but the logic lives with the app. This also fixes today's
inversion where `apps.rs` imports clock IDs out of `screen::analog_clock`.
Each move is a real module boundary with its own imports and privacy (never
`include!` of a hand-written file, never `part-NN` sharding).

Key decisions:

- **Fifth `MeditamerApp` on the base provider** (`ProviderId(1)`), not its own
  provider. All four current apps share it, and `apps.rs` already anticipates
  either topology; staying on base leaves provider capacity unchanged.
- **Reuse the frame format and swipe thresholds, not the bin code.** The bin
  blocks on raw-controller polling and pushes raw panel buffers with no LVGL;
  the app must react to LVGL gesture events and paint into a canvas.
- **Loader follows the product storage pattern** (`storage/clock_assets`,
  ambient pack-loader channel ops), not `updater/fat_io` verbatim.
- **`LAUNCHABLE` only, not `AMBIENT`.** Gallery is interactive; it must not
  become an ambient fallback. No scheduling deadline (static image, unlike
  Ambient/Clock).
- **IDs:** `SurfaceId(13)`, `EntryId(1, 7)`, `GlyphRef(7)`, `default_rank: 4`.
  Surfaces 1-12, entry locals 1-6, and glyphs 1-6 are all taken (Clock is
  surface 11 / entry 6). Verify `CATALOGUE_CAPACITY` fits one more entry.
- **Phase 1 moves nothing existing.** Only `apps/gallery/` is new; the
  `screen/` -> `apps/<name>/` + `chrome/` moves are a deferred, behavior-free
  Phase 2 so this diff stays reviewable.

Rejected: a flat `screen/gallery.rs` (fifth flat file plus new arms at every
match site — the problem restated); giving Gallery ambient capability (idle
fallback showing arbitrary slides was never requested).

## Steps

1. **DRAM + capacity check.** Read `references/memory/meditamer-inkplate/budget.md` before
   sizing the slide buffer; confirm `dram_seg` headroom and catalogue
   capacity for one more entry. Record the numbers in the change.
2. **`apps/gallery/model.rs`.** Pure slide list, index advance/wrap, and MDFR
   header parse (`MDFR`, version, format/width/height). No LVGL, no crate-path
   dependency — host-testable via `#[path]` like `analog_clock/model.rs`.
3. **`apps/gallery/loader.rs`.** SD-to-PSRAM streaming with channel ops only
   (ambient pack-loader shape); surfaces loading/failed placeholders so the
   screen stays navigable until the first slide lands.
4. **`apps/gallery/screen.rs`.** LVGL canvas screen; swipe-to-advance mapped
   from the bin's thresholds (`SWIPE_MIN_DX/DY`) into `LvglGestureEvent`
   handling, following `gesture_test.rs` event shape.
5. **`apps/gallery/mod.rs` + registry.** Descriptor, `Screen` type, `create()`;
   add `MeditamerApp::Gallery`, grow `MeditamerApps<4>` to `<5>`, extend
   `BASE_SURFACES` 12 to 13, add `SurfaceRefs::gallery`, `SurfaceModel::Gallery`
   arms (`enter`, `root_widget`, `destroy`), and a `gallery_shared` cache
   threaded through `lvgl_surface_runtime` next to `ambient_shared`.
6. **Catalogue + ranks.** Gallery entry flows in via `apps.iter()` in
   `build_product_catalogue`; confirm launcher ordering and persisted
   settings still resolve.
7. **Phase 2 (separate change, listed only):** move each existing screen
   subtree under `apps/<name>/`, rename app-less `screen/` to `chrome/`.

Files touched in Phase 1: `ui/apps.rs` (registry), `ui/lvgl/backend.rs`
(surfaces, model, dispatch), `ui/lvgl/backend/init.rs` (wiring),
`ui/apps/gallery/` (new, four files). The gallery bin, overlays, and Wi-Fi /
upload paths are explicitly untouched.

## Validation Plan

- `scripts/host-test.sh test shell` — registry/navigator/catalogue unaffected.
- `scripts/host-test.sh test ui-shell` — LVGL harness incl. new gallery model
  cases (advance, wrap, bad-header rejection).
- `scripts/ci/check_software_baseline.sh firmware-builds` and
  `firmware-clippy` — product firmware still builds, strict clippy clean.
- Pre-commit gates as run by lefthook: `scripts/ci/check_orphan_modules.py`,
  `scripts/ci/lint_code_analysis.sh` (SLOC ratchet: no new hard-ceiling
  offender; gallery files stay individually small), and
  `scripts/ci/check_markdown_links.sh` for this plan.
- On device: flash the default `meditamer` artifact via
  `targets/meditamer-inkplate/build.sh`, launch Gallery from the launcher,
  swipe both directions, confirm Ambient/Clock still launch. Highest-risk
  check is the swipe mapping (raw-controller thresholds vs LVGL gesture
  space) — verify on glass, not just the harness.
- No Wi-Fi/upload changes, so `guides/wifi-regression-gate.md` is out of
  scope; say so in the change.

## Risks / Open Questions

- **PSRAM pressure.** A full-panel slide buffer is large; if the DRAM check
  fails, fall back to streaming the visible slide only (loader already shaped
  for this) rather than caching neighbors.
- **Gesture feel.** The bin's 80px/60px thresholds were tuned for raw samples;
  LVGL gesture coordinates may need retuning after the on-glass check.
- **Slide source.** Specimens live in `targets/meditamer-inkplate/specimens/`;
  the on-device asset path (e.g. `/assets/GALLERY/`) and whether
  `tools/ambient_sky`-style packing applies is left to implementation.
- **Open:** should Gallery remember its slide index across launches? Default
  is no (always enter on slide 0); persisted position needs an `app_state`
  field and is out of scope unless requested.
