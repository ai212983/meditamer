# Product and Target Axis Completion Implementation Ledger

- Status: Done — all five phases passed; parent closed 2026-09-07
- Last-reviewed: 2026-09-07
- Started: 2026-08-22
- Plan: [Product and target axis completion](product-target-axis-completion-completed-2026-09-07.md)
- Phase 4 design: [Shared runtime design](product-target-axis-phase-4-shared-runtime-completed-2026-09-07.md)
- Decision: [ADR-0023 — Platform, board, product, and target axes](../../architecture/0023-platform-board-product-target-axes.md)

## Ledger contract

The plan owns scope, phase ordering, and exit criteria. This ledger records implementation state,
source baselines, deviations, validation commands, and evidence produced while executing it.

The phase summary is mutable. Every state change cites an append-only evidence entry. Evidence
entries retain their original text; corrections use a new ID and name the entry they supersede.

Phase states are `Queued`, `Ready`, `In progress`, `Passed`, and `Needs revalidation`. A phase reaches
`Passed` when its plan exit criteria and applicable validation have exact evidence here.

## Phase status

| Phase | State | Prerequisite | Evidence | Next action |
| --- | --- | --- | --- | --- |
| 1. Build roots and guards | Passed | Accepted topology | E-0001, E-0002 | — |
| 2. Medinote/Waveshare separation | Passed | Phase 1 | E-0003 | — |
| 3. Meditamer/Inkplate separation | Passed | Phase 1 | E-0004 | — |
| 4. Shared runtime extraction | Passed | Phases 2–3 | E-0007, E-0008, E-0010 | — |
| 5. Surface contraction and residuals | Passed | Phase 4 implementation | E-0004 (F6 resolved early), E-0008, E-0009 | — |

## Evidence entries — append only

### E-0001 — Implementation ledger opened

- Date/phase: 2026-08-22; planning baseline before phase 1.
- Source baseline: `b82ad405dcf9b7ee44349917693293ef56729f06`; documentation worktree adds the
  accepted plan amendment, plan rename, and this ledger.
- Result: the accepted product/board/target topology has a sequenced implementation plan and an
  evidence ledger.
- Validation: scoped Markdown links, Markdown LOC, and whitespace checks passed for the plan and
  ledger.
- Evidence class: documentation baseline.
- Next action: begin phase 1 with the product and target Cargo graph design.

### E-0002 — Phase 1 build roots and guards

- Date/phase: 2026-08-22; Phase 1 (Build roots and guards), executed in full.
- Source baseline: `b82ad405dcf9b7ee44349917693293ef56729f06`; worktree adds the two product library
  roots, the two target build roots, and the repository-check extensions below (staged, not yet
  committed).
- Scope: added `products/meditamer` (crate `meditamer-product`) and `products/medinote` (crate
  `medinote`) as empty scaffold libraries and shared-workspace members; added `targets/meditamer-inkplate`
  and `targets/medinote-waveshare` as independent firmware build roots (excluded from the shared
  workspace, own `Cargo.lock`, own toolchain wiring), each a bring-up binary that boots its chip's RTOS
  and exercises `arbitration`/`shell`/`console` plus its new product and (for the Inkplate target) board
  crate — deliberately not yet the shipped firmware; the root `meditamer`/`updater` binaries and
  `boards/waveshare-rlcd42`'s own binary stay the compatibility entry points per the plan. Extended
  `scripts/ci/check_orphan_modules.py` (`FIRST_PARTY_PREFIXES` and both `git_paths` call sites),
  `scripts/ci/check_rust_loc.sh`, and `lefthook.yml`'s rustfmt/orphan-modules/code-analysis globs to
  cover `products/` and `targets/`, mirroring exactly where `boards/` already appears in each (code-size
  and module-reachability checks now see both target graphs; `lint_code_analysis.sh`'s own `-p` list
  was left untouched, since it does not cover `boards/`/`platform/` either — a pre-existing scope
  narrower than the other checks, unrelated to this plan). Registered `meditamer-product` and `medinote`
  in `scripts/host-suites.tsv`. Added `.gitignore` entries for `products/*/target`, `products/*/Cargo.lock`
  (workspace members, same convention as `platform/*`), and `targets/*/target` (independent build roots,
  same convention as `boards/*`).
- Deviations found and resolved while executing:
  - `targets/meditamer-inkplate` shares its target triple (`xtensa-esp32-none-elf`) with the repo root,
    whose `[target.xtensa-esp32-none-elf]` rustflags select `-Tmeditamer-linkall.x` via a relative
    `-Lconfig/linker/esp32` search path that only resolves from the root package's own workspace root.
    A local `.cargo/config.toml` override does not drop it: Cargo *joins* `rustflags` arrays across
    ancestor config files rather than letting a closer file replace them, confirmed by observing both
    `-T` flags on one linker command line. `build.sh` sets `RUSTFLAGS` instead, which takes full
    precedence over config-file rustflags rather than joining with them, and links against plain
    `linkall.x`. `targets/medinote-waveshare` does not hit this: its target triple
    (`xtensa-esp32s3-none-elf`) is not defined in the root's config, so its own
    `[target.xtensa-esp32s3-none-elf]` table in a local `.cargo/config.toml` is additive, not a merge
    conflict — matching `boards/waveshare-rlcd42`'s existing pattern.
  - `targets/medinote-waveshare`'s freshly generated `Cargo.lock` resolved `esp32s3` PAC crate `0.35.3`,
    which removed/renamed items (`USB0`, `ext_wakeup1_sel`, `ext_wakeup1_status_clr`) that `esp-hal`
    `1.1.1` uses, breaking the build. `boards/waveshare-rlcd42`'s committed `Cargo.lock` still resolves
    `0.35.2` — not pinned in its manifest either, simply never regenerated since. Pinned
    `esp32s3 = "=0.35.2"` directly in `targets/medinote-waveshare/Cargo.toml` to the known-working
    version rather than touching the board's own lockfile.
- Validation:
  - `cargo metadata --locked` resolved clean for the shared root workspace (13 members, including both
    new product crates) and, separately, for both `targets/*/Cargo.toml` (each its own implicit
    workspace, per `cargo metadata --manifest-path`).
  - `cargo test -p meditamer-product -p medinote` and `cargo clippy -p meditamer-product -p medinote
    --all-targets -- -D warnings`: both crates pass, on host, inside the `archbox` distrobox.
  - `scripts/ci/check_orphan_modules.py`: 641 tracked Rust files, 57 target roots, 29 manifests, zero
    unreachable (up from 627/47 recorded in ADR-0015's "`packages/` retired" section; the new files
    were staged first — the script's default `git ls-files` invocation does not see untracked worktree
    content, which the first run of this check silently missed before staging).
  - `scripts/ci/check_rust_loc.sh`: 569 files checked, same pre-existing warnings/advisories as before,
    no new ones.
  - `scripts/ci/check_script_surface.py`: clean (47/47).
  - `cargo fmt --all --check` (root workspace) and `cargo fmt --check` against both target manifests:
    clean.
  - Pointed at the ESP toolchain in the `archbox` distrobox
    (`~/.rustup/toolchains/esp/xtensa-esp-elf/esp-15.2.0_20250920/xtensa-esp-elf/bin` on `PATH`):
    `targets/meditamer-inkplate/build.sh` and `targets/medinote-waveshare/build.sh` both link a release
    bring-up binary end to end; `cargo clippy -- -D warnings` passes for both against their real
    `xtensa-esp32-none-elf`/`xtensa-esp32s3-none-elf` targets. Regression-checked in the same
    environment: `scripts/build/build.sh release` (root, unaffected — cached, 0.88s) and
    `boards/waveshare-rlcd42/build.sh` (unaffected, links clean) both still build.
  - Not run: `scripts/ci/check_markdown_links.sh` (the `archbox` container has no `lychee` installed —
    a pre-existing tooling gap, not touched by this change).
- Evidence class: source-verified, host- and cross-toolchain-verified; not hardware-verified (neither
  bring-up binary has been flashed to a device — consistent with ADR-0015's board-extraction note that
  hardware verification needs a session with device access).
- Next action: Phase 2 (Medinote/Waveshare separation) and Phase 3 (Meditamer/Inkplate separation) are
  both unblocked and independent of each other; start with an inventory of the code each phase moves,
  per the plan's own next-action text.

### E-0003 — Phase 2 Medinote/Waveshare separation

- Date/phase: 2026-08-22; Phase 2 (Medinote/Waveshare separation), executed in full. User direction:
  continue through the plan's remaining phases without hardware verification (deferred to a later
  session with device access).
- Source baseline: E-0002's tree (staged, uncommitted).
- Scope: `boards/waveshare-rlcd42` lost its `[[bin]]` and became a `[lib]`, matching
  `boards/inkplate-tempera` — `git mv`'d `main.rs` and `wall_clock.rs` out, split `ui.rs` in place
  (`panel_lvgl.rs` stayed board-owned: the LVGL flush bridge names the concrete `St7305` type and its
  framebuffer geometry; `screen.rs` moved to `products/medinote`: screen construction and content name
  no panel type at all). `products/medinote` gained `presentation` (four formatting/calc functions, now
  host-tested for the first time — they lived inside a `#![no_main]` binary crate before, which cannot
  run `cargo test`), `config` (cadence and deployment-default constants), and `screen` (behind a new
  `lvgl` feature, so a plain `cargo test` still needs no C toolchain). `targets/medinote-waveshare`
  absorbed the real boot sequence, peripheral wiring, RTC-RAM diagnostics, and task spawning from the
  moved `main.rs`, composing `waveshare-rlcd42` (board), `medinote` with `features = ["lvgl"]` (product),
  and `boards/waveshare-rlcd42/shtc3` directly (packages-directory retirement precedent: reached through
  the target, not re-exported by the board). Its own `lvgl/lv_conf.h` is a local copy of the board's,
  not a cross-directory relative reach, matching how every other build root here owns its config rather
  than sharing one.
- Deviations and judgment calls:
  - The Phase 1 bring-up self-test (`exercise_arbitration`/`exercise_shell`, proving `console`/`shell`/
    `arbitration` linked on Xtensa LX7) was deleted rather than kept alongside the real firmware: the
    shell registry calls in `cycle_task`/`sensor_task`/`ui_task` are the real thing it rehearsed, and
    `arbitration` had no organic caller left once it was gone — Medinote has no radio to arbitrate yet.
    `arbitration` is dropped from this target's dependencies entirely.
  - Cadence policy (`DEEP_SLEEP`, `SLEEP_INTERVAL_S`, `SAMPLE_INTERVAL_S`) and deployment defaults
    (`KNOWN_EPOCH`, `OFFSET_MINUTES`) moved to `products/medinote::config`, following the plan's own
    "cadence policy" wording for Phase 2's scope, even though only the target's task-wiring code reads
    them. `CONSOLE_SETTLE_MS`/`DIAGNOSTIC_HOLD_MS`/`RETAIN_PANEL_ACROSS_SLEEP` stayed target-level:
    they document board/hardware-timing judgment calls (pad-hold behavior on deep sleep, USB CDC
    re-enumeration) inseparable from the peripheral-setup code they sit beside, not product policy.
  - `esp32s3` PAC crate needed pinning to `=0.35.2` in `boards/waveshare-rlcd42/Cargo.toml` too (E-0002
    found this for the target's own manifest already): the board's freshly-regenerated `Cargo.lock`
    (deleted to confirm the crate resolves standalone) hit the same 0.35.3 regression.
  - Clippy found three real findings in code moved unchanged from the original bin crate, never
    caught before because `boards/waveshare-rlcd42` was never clippy-gated as a library target: two
    `missing_safety_doc` (added `# Safety` sections to `panel_lvgl::{init, set_active_panel}`), one
    `deref_addrof` in the LVGL buffer-size calculation (replaced `size_of_val(&*(&raw const
    DRAW_BUFFER))` with `size_of::<[u8; WIDTH * DRAW_LINES]>()`, which also needs no `unsafe`). A fourth,
    `absurd_extreme_comparisons` on `DIAGNOSTIC_HOLD_MS > 0` (the constant compiles to zero in this
    build; it is meant to be hand-edited positive for local debugging), got a scoped `#[allow]` instead
    of a rewrite, since the comparison is genuinely meant to become non-trivial at times. Trimmed
    `esp-backtrace`/`esp-rtos` from the board's `[patch.crates-io]` after clippy reported them unused —
    a lib with no panic handler or RTOS of its own doesn't pull them in.
- Validation:
  - `cargo test -p medinote`: 10/10 pass, including 8 new tests for the moved `presentation` functions
    (battery-curve clamping/interpolation, HH:MM wraparound, negative-temperature formatting) that had
    no coverage at all before this phase.
  - `cargo clippy -p medinote --all-targets -- -D warnings`: clean, on host.
  - `scripts/ci/check_orphan_modules.py`: 644 tracked files, 56 target roots, 29 manifests, zero
    unreachable (up from 641/57/29 after E-0002; net after the `ui.rs` split and the two moved files).
  - `scripts/ci/check_rust_loc.sh`: 572 files; `targets/medinote-waveshare/src/main.rs` still crosses
    the 1000-line high-attention advisory (1021 lines, down from the original 1241) — pre-existing and
    advisory-only, not a new condition, and further splitting it is not this phase's scope.
  - `scripts/ci/check_script_surface.py`: clean (47/47), after updating a stale `main.rs` cross-reference
    in `boards/waveshare-rlcd42/scripts/reply_time_request.sh`'s comment to point at
    `targets/medinote-waveshare/src/wall_clock.rs`.
  - `cargo fmt --all --check` (root) and `--check` against both the board's and the target's manifests:
    clean.
  - `cargo test --manifest-path boards/waveshare-rlcd42/shtc3/Cargo.toml --locked`: 5/5 pass, unaffected
    regression check.
  - In the `archbox` distrobox with the xtensa toolchain on `PATH`: `boards/waveshare-rlcd42/build.sh`
    (now building a library, not a binary) and `targets/medinote-waveshare/build.sh` (now the real
    firmware, LVGL bindgen wiring added to match) both link clean; `cargo clippy --release -- -D
    warnings` passes for both against their real `xtensa-esp32s3-none-elf` target, after the fixes
    above.
- Evidence class: source-verified, host- and cross-toolchain-verified; not hardware-verified by user
  direction — deferred to a later session with device access.
- Next action: Phase 3 (Meditamer/Inkplate separation) is the only remaining independent phase; Phase 4
  needs it complete first.

### E-0004 — Phase 3 Meditamer/Inkplate separation

- Date/phase: 2026-08-22; Phase 3 (Meditamer/Inkplate separation), executed in full, by user direction
  to continue through the plan's remaining phases without hardware verification.
- Source baseline: E-0003's tree (staged, uncommitted).
- Scope, in one sentence: the root package's entire `src/firmware/` (~35,000 lines, every module) moved
  to `products/meditamer/src/firmware/` unchanged in substance, except `system.rs`/`system/tasks.rs`
  (chip startup, Embassy task wiring) and `net_host.rs`/`serial_uart.rs` (their only two consumers, both
  moved with them), which moved to `targets/meditamer-inkplate` instead; the root package's
  `[[bin]]`/`[lib]`/`[dependencies]`/`[features]`/`[profile.*]` retired entirely, making root `Cargo.toml`
  a virtual workspace manifest with no package of its own.
- Reconnaissance: an Explore agent inventoried every `src/firmware` module (file-by-file, hardware refs,
  `#[embassy_executor::task]` signatures, cross-module coupling) before the move, largely confirming a
  single-crate move was viable — Meditamer's product code is pervasively `esp_hal`-typed at nearly every
  task-function signature (unlike Medinote's, which stayed cleanly separable in Phase 2), so a
  finer-grained product/board split was out of scope for a move at this size; the report is preserved in
  this session's transcript, not committed, and is worth re-running before any future attempt to split
  `products/meditamer` further. It also surfaced two files (`net_host.rs`, `serial_uart.rs`) the initial
  bulk move had left in the product with their only consumer already in the target; both were relocated
  before the build was attempted.
- Material decisions:
  - `flash.rs` stays in `products/meditamer` despite owning `esp_hal::peripherals::FLASH` directly:
    `app_state/store.rs` (product) and the updater (target) both need it, and the plan's own dependency
    flow (`targets -> products/boards -> platform`) forbids a product depending on a target. This is the
    plan's F4 follow-up made concrete, not resolved by this phase.
  - `update.rs` (OTA slot/status/health-task logic) stayed in `products/meditamer` as product policy;
    only its *boot-time wiring* (the `initialize_boot_state()` call, the `firmware_health_task` spawn) is
    what the plan's "firmware update wiring" phrase names as target-owned, and that wiring already lives
    in `system.rs`/`system/tasks.rs`.
  - The factory updater (`src/updater/`, `src/bin/updater.rs`, ~1,400 lines) moved to
    `targets/meditamer-inkplate` as a second `[[bin]]`, not to the product: it is a separate deployable
    recovery artifact, calls into `meditamer_product::firmware::{update, flash}` as an ordinary
    cross-crate dependency, and touches no meditation-domain content. `#[path = "../updater/mod.rs"]`
    inside `src/bin/updater.rs` keeps it self-contained rather than adding a `[lib]` neither binary
    otherwise needs.
  - Every `pub(crate)` in the moved tree became `pub` (a repo-wide sed, not file-by-file), matching the
    precedent ADR-0015 set extracting `platform/shell` ("promoted wholesale to pub... narrowed... once
    the real call pattern is visible") -- `system.rs`/`system/tasks.rs`/`net_host.rs` now reach every
    task entry point, driver type, and channel from a different crate. Narrowing this surface is Phase
    5's F5 disposition, not attempted here.
  - `src/firmware/assets/suminagashi_blue_noise_600.bin` (the blue-noise asset F6 named) was confirmed
    genuinely unreferenced anywhere in the repository and deleted outright, resolving F6 early rather
    than carrying dead weight into Phase 5.
  - `config/events.toml` and `assets/fonts` stayed at the repo root rather than moving into
    `products/meditamer`: `test-support/host/{event_engine,ui_shell}_host_harness` already reach them
    from their own nested locations, and moving them would have forked the two copies.
  - `esp_bootloader_esp_idf::esp_app_desc!()` -- the ESP-IDF bootloader's app descriptor, previously
    emitted once by the shared root `[lib]` and linked into both binaries -- needed an explicit call in
    *each* of `main.rs` and `bin/updater.rs` once they stopped sharing a lib crate. Missed on the first
    build; caught by the ELF section comparison below (`.flash.appdesc` was silently absent), not by any
    compiler diagnostic -- an artifact-identity regression a clean build gives no signal for on its own.
  - `meditamer-product = { path = ..., default-features = false }` in the target's manifest: without it,
    the product's own `default = [...]` feature set stays enabled regardless of the target's
    `--no-default-features`, since dependency defaults are independent unless explicitly disabled. Also
    caught after the fact -- the `minimal` profile build "succeeded" but hadn't actually shrunk the
    product half of the binary, visible only by comparing `minimal` and `default` binary sizes and
    noticing `minimal` wasn't smaller where it should have been.
  - `products/meditamer` cannot be built or tested standalone against a host target, and is not meant to
    be: `console`'s `esp-println-upstream` dependency carries no chip/transport feature by design (the
    final binary selects it), so a bare `cargo check`/`test` against the product crate's own manifest
    panics in `esp-println`'s build script with no feature selected. It resolves correctly only as part
    of `targets/meditamer-inkplate`'s build, where the target's own `esp-println-upstream` declaration
    (with concrete features) unifies across the shared dependency graph. The `meditamer-product` row was
    removed from `scripts/host-suites.tsv` for the same reason it was added stale in Phase 1 evidence --
    Phase 3 changed the crate from an empty host-testable scaffold into real, xtensa-only firmware
    content, and the registry needs to reflect what actually runs, not what the scaffold once was.
  - `[profile.*]` (including `ble-release` and per-crate `esp-radio`/`esp-storage`/
    `esp-radio-rtos-driver` opt-level overrides) moved from the root manifest to
    `targets/meditamer-inkplate/Cargo.toml` outright, not just referenced: profiles are workspace-scoped,
    and `targets/*` is excluded from the root workspace, so the root's copy would never have reached it.
  - Same-triple `rustflags` (confirmed in E-0002 to *join* rather than override across ancestor
    `.cargo/config.toml` files) meant this target's `build.sh` needed the same `RUSTFLAGS`-env-var
    override E-0002 used for the Phase 1 bring-up binary, but now pointing at the *real*
    `meditamer-linkall.x`/DRAM-budget linker script via an absolute `-L$repo_root/config/linker/esp32`
    path (a relative one does not resolve -- Cargo runs the linker with this crate's own directory as
    its working directory, not the repo root, confirmed empirically). No local `.cargo/config.toml`: the
    crate inherits the root's `[env]` table (`ESP_HAL_CONFIG_USE_RWDATA_LD_HOOK=true`, etc.) unchanged,
    which is what the real firmware needs, unlike the Phase 1 placeholder that deliberately opted out of
    it.
- Validation:
  - `scripts/ci/check_orphan_modules.py`: 643 tracked files, 55 target roots, 29 manifests, zero
    unreachable.
  - `scripts/ci/check_rust_loc.sh`: 572 files; same pre-existing warnings/advisories, now reported from
    their `products/meditamer/...` paths, no new ones.
  - `scripts/ci/check_script_surface.py`: clean (47/47).
  - `cargo fmt --all --check` (root) and `--check` against `targets/meditamer-inkplate`,
    `targets/medinote-waveshare`, and `boards/waveshare-rlcd42` manifests: clean, after `cargo fmt --all`
    picked up line-length changes the wholesale `pub(crate)` -> `pub` promotion and the longer
    `meditamer_product::firmware::` path prefix produced (shorter/longer lines crossing rustfmt's wrap
    threshold in both directions).
  - Every `test-support/host/*_host_harness` crate whose `#[path]` shims point into the moved tree --
    `event_engine_host_harness`, `app_state_store_host_harness`, `net_status_host_harness`,
    `ui_shell_host_harness`, `ble_transport_host_harness` -- and `tools/touch_replay` (8 test binaries,
    128 individual tests): all pass, `cargo test` and strict `cargo clippy -- -D warnings` alike, on host,
    confirming the repointed paths resolve to the same files with the same content.
  - Regression, unaffected by this phase: `cargo test -p medinote` (10/10), `boards/waveshare-rlcd42`
    and `targets/medinote-waveshare` `build.sh` (both link clean) -- Phase 2's tree untouched by Phase
    3's changes to the root manifest and `products/meditamer`.
  - In the `archbox` distrobox with the xtensa toolchain: `targets/meditamer-inkplate/build.sh` links a
    real release binary end to end for the `default`, `minimal`, and `ble-release` profiles, and the
    `updater` binary builds under `--no-default-features --features factory-updater`; `cargo clippy
    --release -- -D warnings` passes clean for `default`, `minimal`, and `--all-features`.
  - **ELF section comparison against the pre-Phase-3 build**, matching the rigor ADR-0015's own Inkplate
    board extraction used: the last root-package release binary (built during E-0003's regression check,
    saved outside the repo before any Phase 3 file moved) versus the new `targets/meditamer-inkplate`
    default-profile release binary, both via `xtensa-esp32-elf-size -A`. First pass found
    `.flash.appdesc` entirely absent (the missed `esp_app_desc!()` call above); after both fixes, total
    size is 2,124,429 bytes against a 2,118,572-byte baseline (+0.28%), `.text`/`.rodata` shifted by a few
    KiB in offsetting directions (plausible codegen variance from crossing a new crate boundary, not a
    missing-content signal), and every other section -- `.data`, `.bss`, `.rwtext`, `.rwtext.wifi`,
    `.rodata.wifi`, `.stack`, `.vectors`, `.dram2_uninit` -- matches the baseline within noise, the same
    standard the board-extraction ELF diff used ("shifted by noise-level amounts").
- Evidence class: source-verified, host- and cross-toolchain-verified, ELF-size-verified against the
  pre-migration build; not hardware-verified by user direction -- deferred to a later session with device
  access. The two artifact-identity/feature-forwarding defects this entry names were caught by the size
  comparison and a profile-size sanity check respectively, not by any compiler diagnostic -- both are the
  kind of regression a clean build gives no signal for on its own, which is the argument for keeping that
  comparison in the validation routine for any future change of this shape.
- Next action: Phase 4 (shared runtime extraction) is unblocked -- both products now have their own copy
  of `net` in the right place. Recommend re-running a whole-module import census
  (`scripts/module_census.py`, per ADR-0015's own repeated finding that grep-based estimates undercount)
  against `products/meditamer/src/firmware/net` before starting the `platform/netstack` move, since
  ADR-0015's own Tier 2 record shows every earlier estimate of that boundary was corrected by a wider
  search.

### E-0005 — Phase 4 reconnaissance: extraction plan, not yet executed

- Date/phase: 2026-08-22; Phase 4 (shared runtime extraction) reconnaissance only. Deliberately stopped
  short of code motion -- see "Why this phase stops here" below.
- Source baseline: E-0004's tree (staged, uncommitted).
- Census: `scripts/module_census.py`'s hardcoded `SRC = pathlib.Path('src')` no longer resolved anything
  (`src/firmware` moved in Phase 3); repointed to `products/meditamer/src` and reran. Confirmed clean:
  `net -> types(8), config(4), psram(2), observability(1), service_mode(1)`, and none of those five
  modules' own outbound edges reach back into `net` -- no cycle, matching ADR-0015's "the cycle is
  broken" finding for `ble` <-> `net` specifically, now re-verified for `net`'s complete dependency set
  against the post-Phase-3 tree rather than assumed to still hold.
- The extraction plan this reconnaissance produced, precise enough to execute directly:
  - `net/{mod.rs, host.rs, runtime.rs, runtime/off_resources.rs, wifi.rs, wifi/**}` (6,851 lines) moves
    to a new `platform/netstack` crate wholesale.
  - Four channels move with it: `config::{NET_CONFIG_SET_UPDATES, NET_CONTROL_COMMANDS,
    WIFI_CREDENTIALS_UPDATES, WIFI_RUNTIME_POLICY_UPDATES}` (confirmed as `net/wifi.rs`'s complete
    channel import list -- no others).
  - The WIFI-prefixed types move with it: `WifiCredentials`, `WifiRuntimePolicy`, `WIFI_SSID_MAX`,
    `WIFI_PASSWORD_MAX`, `WIFI_DHCP_TIMEOUT_MAX_MS`, `NetConfigSet`, `NetControlCommand`, and
    `types/wifi.rs`'s other 217 lines generally (not individually re-verified symbol by symbol -- the
    file's name and this module's own count already match ADR-0015's estimate closely enough that a
    file-level move is very likely correct, but the receiving crate's build is the actual proof).
  - `observability::recorders::wifi` (270 lines, confirmed entirely `record_wifi_*`/`set_wifi_*`
    read/write calls with the single exception below) moves with it; `observability::snapshot()`
    becomes a composition of a `platform/netstack` snapshot and a `products/meditamer` one, matching
    ADR-0015's own anticipation of this ("the single unified struct becomes a composition of product and
    platform parts").
  - `set_upload_http_listener` is the one write ADR-0015 named as shared with `firmware/storage`; it
    moves with netstack and is called inward from the product, same as the ADR decided.
  - The remaining two coupling classes need a port, not a move -- `net/host.rs`'s existing `ProductState`
    (fn-pointer struct, already installed via `AtomicPtr` at startup by
    `targets/meditamer-inkplate/src/net_host.rs`) is the established pattern to extend, not a new
    mechanism to invent:
    - `service_mode`'s three symbols (`upload_enabled`, `upload_http_listener_enabled`,
      `set_radio_handoff_admission_open`) are genuinely product-shared, not net-private --
      `service_mode`'s fan-in is `net, serial, storage` -- so they become three more `ProductState`
      fn-pointer fields (two reads, one write), installed the same way the existing four are.
    - `psram`'s two diagnostic calls in `net/runtime.rs`'s `resource_snapshot()` (`allocator_memory_snapshot()`,
      read only for `.free_internal_bytes`, and `probe_internal_block_above_reserve(usize)`, which
      returns a 5-field plain-data struct -- `free_before_bytes`, `block_bytes`, `reserve_bytes`,
      `free_after_bytes`, `stable`, all primitives) become two more `ProductState` fn-pointer fields.
      `platform/netstack` defines its own 5-field plain struct for the second one rather than depending
      on `products/meditamer::firmware::psram::InternalBlockProbe` -- the shape is pure arithmetic data,
      not a real type-sharing need, and owning it locally keeps the port's only cross-crate type
      dependency at zero.
  - No `ble` port needed: confirmed by the census (no `net` <-> `ble` edge in either direction), matching
    ADR-0015's "platform/arbitration: the cycle is broken" finding.
- Why this phase stops here rather than executing the move: ADR-0015 states its own methodology
  explicitly for this exact boundary -- "Tier 2 is therefore correctly sized as design-then-extract, not
  relocate-plus-port, and should not be attempted in the same pass as a mechanical move." Phases 1-3 in
  this session were each closer to the "mechanical move" end of that spectrum (even Phase 3's scale was
  one direction of motion with clear boundaries once the census confirmed no split was needed); Phase 4
  is the "design-then-extract" case the ADR is warning about, touching shared telemetry state and a
  radio-admission write path adjacent to ADR-0014's still-unresolved scheduler defect. The design above
  is complete and, on its own terms, low-risk (it extends an existing, already-proven port pattern rather
  than inventing one), but executing it and re-deriving the same build/clippy/census/ELF-size verification
  depth used for Phases 1-3 is a full additional unit of work, not a continuation of Phase 3's, and
  deserves its own pass with a clear head rather than being run at the tail of this one.
- Evidence class: design-verified against current source (every symbol named above was grepped and read
  in the post-Phase-3 tree, not assumed from the ADR's older line numbers); no code motion attempted, so
  nothing here is build- or test-verified.
- Next action: execute the plan above -- create `platform/netstack`, move the files and channels/types
  named, extend `ProductState` with the six new fields, update `net_host.rs`'s installation call and
  `storage/upload/http/mem_diag.rs`'s reverse `wifi_rx_buffer_stats` reference, then apply the same
  validation routine E-0004 used (build all profiles, clippy default and all-features, orphan-modules,
  rust-loc, fmt, and an ELF-size comparison against this session's binaries). Phase 5 (F1-F5) stays
  queued until this lands -- F1 (network operational truth) is what this extraction delivers.

### E-0006 — Phase 4 shared runtime design approved

- Date/phase: 2026-08-23; Phase 4 design amendment, no code motion.
- Source baseline: `4faca2f37c342607925da9aae16441591ed0ede3`; the documentation worktree adds
  the standalone Phase 4 design and links it from the parent plan and this ledger.
- User direction: Meditamer and Medinote are both evolving products whose internals should be shared
  unless a distinct owner is justified. The user approved the recommended shared configuration and
  persistence boundary.
- Supersession: this entry supersedes E-0005's proposed extraction design, including its four-channel
  scope and callback-count next action. E-0005 remains authoritative reconnaissance evidence for the
  current module census, source inventory, and absence of a `net` cycle.
- Result: [the Phase 4 design](product-target-axis-phase-4-shared-runtime-completed-2026-09-07.md) makes network runtime,
  all six configuration channels, the complete Wi-Fi type domain, validation, codec, persistence
  protocol, operational state, and network telemetry shared platform functionality. Only physical
  persistence I/O stays behind a target-selected storage backend. Shared log filtering moves to
  `platform/runtime`; `platform/netstack` is selected for ESP32 or ESP32-S3 by the final target.
- Ports: product service work remains behind the established allocation-free `NetHost` pattern;
  product network-enable policy and allocator diagnostics cross plain synchronous ports. Listener
  policy, listener/admission truth, and hot network telemetry move with netstack rather than becoming
  product callbacks.
- Validation: the current census still reports `net -> types(8), config(4), psram(2),
  observability(1), service_mode(1)` with no return edge. Ledger-inclusive scoped link validation
  passed 15/15 links; the design is 189 lines, below the 220-line advisory; whitespace checks passed.
  The whole-repository
  link check still reports 23 stale pre-Phase-4 source paths in five existing live documents after the
  Phase 3 move; none is introduced by this design.
- Evidence class: source-grounded and documentation-validated; not build-, target-, or
  hardware-verified because implementation has not started.
- Next action: execute the linked design in its recorded migration order, beginning with fresh module
  census and identified Meditamer ELF/DRAM baselines. Phase 4 remains `Ready`; Phase 5 remains queued.

### E-0007 — Phase 4 shared runtime extraction executed for Meditamer/Inkplate

- Date/phase: 2026-08-23; Phase 4 (shared runtime extraction), code motion executed for the
  Meditamer/Inkplate side in full; Medinote/Waveshare wiring and hardware validation deferred (see
  "Why this stops here" below).
- Source baseline: `a006fe1` (Phase 3's tree; the last code commit before this entry) plus the
  uncommitted E-0006 design amendment already in the worktree at session start.
- Scope: created `platform/runtime` (shared log-domain filter mask, moved from
  `observability::recorders::log_filter` and `observability::counters`' `LOG_DOMAIN_*`/
  `LOG_FILTER_MASK_*`) and `platform/netstack` (moved `net/{mod.rs, host.rs, runtime.rs,
  runtime/off_resources.rs, wifi.rs, wifi/**}` wholesale -- 6,851 lines -- plus the complete
  configuration domain, the credential persistence protocol, and network telemetry). Both are shared
  workspace members with no chip feature of their own, following `platform/console`'s
  feature-unification pattern; `netstack` is behind Meditamer's existing `asset-upload-http` optional
  dependency, unchanged from how `embassy-net` was already gated.
- Ownership boundary realized exactly as designed:
  - `netstack::config` -- `WifiCredentials`, `WifiRuntimePolicy` (+ validation/sanitization),
    `NetConfigSet`, `NetControlCommand`, `WifiConfigRequest`/`Response`/`ResultCode`, all six
    channels, the `ssid=...`/`password=...` codec (moved from
    `storage/sd_task/wifi_config.rs`), and a new `store_credentials`/`classify_persistence_response`/
    `format_netcfg_status_line` persistence-service module consolidating what
    `serial/io/{netcfg,netcfg_persistence}.rs` used to do inline. Meditamer's
    `storage/sd_task/wifi_config.rs` is now the `CredentialStorePort` backend only: SD medium
    prep/read/write/error-mapping, calling the shared codec rather than owning it.
  - `netstack::telemetry` -- Wi-Fi connect/reassociation/link/RSSI counters and the network-pipeline
    counters (DHCP wait, gate reasons, listener on/off, accept timing), split out of the product's
    former mixed `observability::recorders::{wifi, upload_net}` on the exact boundary the design
    names: HTTP request/body/SD-roundtrip timing stayed in the product's `observability` module.
    `products/meditamer::firmware::observability::snapshot()` now composes
    `netstack::telemetry::snapshot()` with the product's own counters into the same `Snapshot`
    struct with the same field names, so `serial/metrics/net.rs`'s serial output needed no changes.
  - `netstack::host` -- extends the existing `NetHost`/`ProductState` port (unchanged: `serve`,
    now-renamed `abort_service_work`, the four existing sync reads) with one product policy callback
    (`upload_enabled`, mapped to Meditamer's `service_mode::upload_enabled`) and two allocator
    diagnostic callbacks returning netstack-owned plain-data types (`AllocatorMemorySnapshot`,
    `InternalBlockProbe`) a target adapter converts into, so netstack depends on no product allocator
    type. Listener enable state, its change sequence, and radio-handoff admission became
    netstack-owned atomics with plain read functions (`host::listener_enabled`,
    `host::set_listener_enabled`, `host::radio_handoff_admission_open`) rather than product
    callbacks, exactly as the design specifies; Meditamer's `storage/upload/http/*` files and
    `serial/command_dispatch.rs` read/write them directly (product depending on platform, the
    accepted direction) instead of through the retired `service_mode::{upload_http_listener_enabled,
    upload_http_listener_set_seq, set_upload_http_listener_enabled, radio_handoff_admission_open,
    set_radio_handoff_admission_open}`, which are deleted.
  - `targets/meditamer-inkplate/src/net_host.rs` is the composition point: implements `NetHost`,
    builds the extended `ProductState`, and converts `psram::{AllocatorMemorySnapshot,
    InternalBlockProbe}` to netstack's mirrored plain-data types field by field.
- Deviations and material decisions found while executing:
  - **A genuine pre-existing defect**, unrelated to this phase, was found and fixed:
    `storage/sd_task/receive/wifi.rs` (part of an alternate, apparently-uncalled-under-default-cfg
    request-dispatch path parallel to `runtime_loop.rs`'s `SdTaskRuntime`) had zero `use` statements
    for the several dozen names its function bodies referenced, and its functions' signatures/call
    sites were missing the `fat_engine: &mut FatEngine` parameter its callees (`handlers.rs`,
    `wifi_config.rs`) require -- confirmed with a minimal `rustc` repro that Rust does not cascade a
    parent module's `use` bindings into a child file module, so this could not have type-checked as
    committed. Fixed by adding the missing imports and threading `fat_engine` through
    `receive_request_with_wifi{,_powered,_unpowered}`/`process_wifi_request`, matching
    `dispatch.rs`'s actual call signature. Left as a standalone fix rather than folded silently into
    the extraction, per the ledger's own evidence-class discipline.
  - **NETCFG SET body parsing stayed product-side**: the plan's "reusable NETCFG parsing... move[s]
    with the configuration service" was read narrowly. `serial/parser/basic/network.rs`'s
    `parse_netcfg_set_command` depends on `serial::parser::basic::util`'s generic byte-slice helpers
    (`find_subslice`, `parse_u64_ascii`, `trim_ascii_whitespace`), which are product-owned serial
    infrastructure shared by every other command parser, not net-specific; moving the JSON-body
    parser into netstack would require netstack to depend on those product utilities, violating the
    `targets -> products/boards -> platform` direction. What *did* move: policy sanitization
    (`WifiRuntimePolicy::sanitized`, already part of the type), response classification, request
    sequencing, and status formatting (`netstack::config::format_netcfg_status_line`, replacing the
    hand-built `NETCFG {...}` JSON assembly `serial/io/netcfg.rs` used to do inline). The persistence
    correlation logic (`classify_persistence_response`) was further split into its own zero-import
    leaf file (`config/persistence/classify.rs`) specifically so
    `test-support/host/ble_transport_host_harness` could keep shimming it directly.
  - **The first allocator diagnostic port is a full snapshot, not a bare byte count**: initial recon
    (E-0005) suggested `radio-handoff` memory-floor logic only reads `.free_internal_bytes`, but
    `netstack::wifi::helpers::log_radio_mem_diag_with_trigger`'s low-memory recovery logging reads
    fourteen fields from the allocator's overall status. `netstack::host::AllocatorMemorySnapshot`
    carries exactly those fourteen (mirroring `psram::AllocatorMemorySnapshot` minus seven
    `min_internal_alloc_*` fields nothing in netstack reads), not the product's full twenty-one-field
    type, keeping the port's cross-crate type dependency at zero as the design requires.
  - **`net/runtime.rs` renamed to `owner.rs`** inside netstack: the module name `runtime` would
    collide with the new `runtime` crate dependency at the same (crate-root) scope from any file that
    needs both (`wifi.rs`'s log-filter macros need `runtime::log_filter_enabled` and the local
    network-owner module in the same file). The module was already privately named and never part of
    netstack's public surface (only specific re-exports are `pub use`'d at the crate root), so the
    rename has no external effect; "owner" matches the module's own vocabulary
    (`NetworkOwnerMachine`, "network owner").
  - **`observability` keeps re-exporting the log-domain registry**: rather than force ~20 call sites
    across `storage/upload/http/**`, `storage/sd_task/**`, and `serial/metrics/**` to switch from
    `observability::LOG_DOMAIN_*`/`log_filter_enabled` to `runtime::*`, `observability.rs` re-exports
    `runtime`'s items under the same names. Single source of truth (`platform/runtime`) with a
    compatibility surface, not a duplicate implementation.
  - **Medinote/Waveshare wiring not attempted this pass**: `targets/medinote-waveshare` has no
    global allocator today (confirmed by grep -- no `esp_alloc`/`#[global_allocator]` anywhere in
    that target, the board crate, or `platform/board`), and `esp-radio`'s Wi-Fi feature needs one.
    Standing one up requires sizing a heap region against that board's own DRAM map, which has no
    equivalent to `docs/reference/dram/dram-budget.md` today and is real target-specific engineering,
    not a mechanical consequence of this extraction. Separately, `targets/medinote-waveshare/src/
    main.rs` is a deep-sleep, wake-sample-sleep device (`main` returns `!` after one bounded wake
    cycle including deep sleep re-entry): `netstack::run_network_owner` is a continuously-running
    restartable epoch supervisor, and spawning it inside that wake cycle without a bounded,
    sleep-compatible lifecycle would be inventing new product behavior, not proving a contract. Left
    as this phase's clear next action rather than improvised unreviewed and unverifiable on real
    hardware.
- Validation, in the repository's `archbox` distrobox with the xtensa toolchain on `PATH`:
  - `scripts/ci/check_orphan_modules.py`: 651 tracked files (up from 643 pre-Phase-4), 57 target
    roots (+2: `platform/netstack`, `platform/runtime`), 31 manifests (+2), zero unreachable.
  - `scripts/ci/check_rust_loc.sh`: 580 files (net/wifi.rs's former 6,851 lines relocated, not
    newly written); `platform/netstack/src/owner.rs` (935 lines, the former `net/runtime.rs`) is a
    new entry in the >=600-line warning band -- a relocation, not a new condition. No new
    high-attention (>1000-line) file.
  - `scripts/ci/check_script_surface.py`: clean (47/47).
  - `cargo fmt --all --check` (root) and `--check` against `targets/meditamer-inkplate`: clean.
  - `targets/meditamer-inkplate`: `cargo clippy --target xtensa-esp32-none-elf
    -Zbuild-std=core,alloc -- -D warnings` clean for `default`, `--all-features`,
    `--no-default-features` (minimal), and `--no-default-features --features factory-updater --bin
    updater`; repeated with `--all-targets` (test binaries too) for `default` and
    `--no-default-features`, also clean.
  - `./build.sh release default|minimal`, `./build.sh ble-release default` (which applies
    `--features ble-foundation`), and a `factory-updater` release build of the `updater` binary all
    link real release artifacts end to end.
  - `cargo test -p runtime` (2/2, new host coverage for the log-filter mask), `-p arbitration`
    (15/15) and `-p medinote` (10/10) unaffected-regression, all pass.
  - `test-support/host/net_status_host_harness`: repointed its `#[path]` shims from
    `products/meditamer/src/firmware/observability/{counters,snapshot,types,recorders/{helpers,
    upload_net,wifi}}.rs` to `platform/netstack/src/telemetry/{counters,helpers,listener,snapshot,
    types,wifi}.rs` (flat, mirroring netstack's own layout so the shimmed files' `super::` imports
    resolve unchanged); 1/1 test passes.
  - `test-support/host/ble_transport_host_harness`: repointed its `netcfg_persistence.rs` shim to
    the new `platform/netstack/src/config/persistence/classify.rs` leaf file; 17/17 tests pass.
  - Regression, unaffected by this phase: `boards/waveshare-rlcd42/build.sh` and
    `targets/medinote-waveshare/build.sh` both link clean (cached, 0.59s each).
  - **ELF section comparison** against a from-source Phase-3 baseline (`git worktree add --detach
    /tmp/meditamer-baseline a006fe1`, built with the identical `release`/`default` profile and
    toolchain, rather than reusing E-0004's narrative baseline, so every section could be diffed
    exactly): total size 2,127,216 bytes vs. 2,118,744 bytes baseline (+8,472 bytes, +0.40%). Every
    DRAM-resident section matches within noise: `.data` -60 B, `.bss` +0 B, `.dram2_uninit` +0 B,
    `.rwtext` +12 B, `.rwtext.wifi` +0 B, `.stack` +48 B. The growth is entirely flash-resident --
    `.text` +2,784 B, `.rodata` +5,688 B -- consistent with crossing new crate boundaries (extra
    monomorphization and symbol duplication across `netstack`/`runtime`), the same pattern and a
    smaller relative delta than E-0004's own accepted +0.28% crossing the product/target boundary.
    No DRAM budget or channel-depth increase to reconcile: no channel changed depth, all six moved
    channels kept their original capacities.
  - Not run: the live [Wi-Fi regression gate](../../guides/wifi-regression-gate.md) and Medinote
    hardware coverage -- both need device access this session does not have, consistent with every
    prior phase's hardware-verification deferral.
- Evidence class: source-verified, host- and cross-toolchain-verified, ELF-size-verified against a
  freshly built pre-Phase-4 baseline; not hardware-verified. Medinote/Waveshare wiring is not
  attempted, for the concrete blocking reasons recorded above, not merely deferred.
- Next action: give `targets/medinote-waveshare` a global allocator and a bounded, sleep-compatible
  network-owner lifecycle, then wire its `NetHost`/`ProductState` adapter and an in-memory
  `CredentialStorePort` backend against `esp32s3`, exercising the same public contracts Meditamer
  now does. Run the Wi-Fi regression gate and record Medinote hardware coverage in the [hardware test
  matrix](../../reference/hardware-test-matrix.md) once device access is available. Phase 4 stays `In
  progress` until both land; Phase 5 stays queued.

### E-0008 — Medinote networking removed from the Phase 4 gate

- Date/phase: 2026-08-23; Phase 4 scope clarification and Phase 5 readiness update.
- Source baseline: `4faca2f37c342607925da9aae16441591ed0ede3` plus the staged E-0007 extraction
  worktree.
- User direction: Medinote has not reached the point where it needs Wi-Fi. Its missing wiring is
  expected and must not block completion of the shared-runtime extraction.
- Supersession: this entry supersedes only E-0007's stated next action and its conclusion that
  Medinote wiring is required for Phase 4 to pass. E-0007 remains authoritative for the extraction,
  validation, and the target-specific engineering that future Medinote networking would require.
- Result: Phase 4 now proves the shared owner with the active Meditamer consumer and retains the live
  Meditamer Wi-Fi regression gate. Medinote adopts `platform/netstack` only through a future accepted
  product networking plan that defines its service, lifecycle, allocator budget, persistence, and
  hardware coverage.
- Next action: run the Meditamer Wi-Fi regression gate when hardware is available. Phase 5 is ready
  now: inventory the broad `meditamer_product::firmware` surface and Phase 3-4 compatibility
  re-exports, classify each active consumer by domain owner, and contract the API in that order.

### E-0009 — Phase 5 surface contraction executed

- Date/phase: 2026-08-23; Phase 5 (contract surfaces and resolve residual ownership), executed in
  full for the F5 disposition (public crate surface).
- Source baseline: E-0007/E-0008's staged tree (still uncommitted).
- Scope: narrowed `products/meditamer/src/firmware`'s visibility from the wholesale `pub` E-0004's
  Phase 3 move produced back down to exactly what `targets/meditamer-inkplate` (both binaries, every
  feature combination) actually reaches, using the compiler as the census rather than tracing call
  sites by hand.
- Method: reverted every crate-internal `pub` declaration in `products/meditamer/src/firmware` to
  `pub(crate)` (a line-anchored script matching `^\s*pub ` so as not to touch already-scoped
  `pub(crate)`/`pub(super)`/`pub(in ...)` or doc comments; 1,156 lines across 113 files), then
  compiled `targets/meditamer-inkplate` against `--all-features` and read every `E0603`/`E0616`/
  `E0624`/`E0364`/`E0365` privacy error the compiler produced. Each error's own "note: ... is defined
  here" span gave the exact file/line to restore; when that note pointed at a re-export site instead
  of the underlying declaration (multi-hop `pub(crate) use` chains, `#[embassy_executor::task]`
  attribute lines), the fix walked to the real definition. Repeated to a fixed point (11 rounds),
  then repeated again separately for `--no-default-features` (minimal) and
  `--no-default-features --features factory-updater --bin updater`, since each feature/binary
  combination reaches a different subset of the crate and the compiler only reports privacy errors
  for code it actually type-checks in that combination.
- Result: of the 1,156 reverted declarations, 210 (18%) are genuinely reached from
  `targets/meditamer-inkplate` and stayed `pub`; the remaining 953 (82%) are crate-private,
  compiler-verified never to be needed outside `products/meditamer`. `meditamer_product::firmware`
  is no longer a blanket-`pub` facade -- its public surface is now exactly what the target's
  `system.rs`, `system/tasks.rs`, `net_host.rs`, and `updater/**` name, module by module.
- Deviations and material decisions:
  - **Re-export chains need every hop promoted, not just the endpoints**: several errors
    (`sd_task`, the `imu`/`touch` task functions) were `E0364`/`E0365` ("only public within the
    crate, and cannot be re-exported outside") even after the referenced function itself was `pub`,
    because an intermediate `pub(crate) use inner::item;` one module layer down capped the
    re-export's effective visibility, or the intervening `mod` declaration was fully private (no
    modifier at all, stricter than `pub(crate)`, and never touched by the revert since it never had
    `pub` to begin with). Fixed by promoting every hop in the chain (`storage::sd_task`,
    `imu::tasks` and its three leaf modules, `touch::tasks`'s already-`pub` re-export whose two leaf
    modules' individual functions were still `pub(crate)`).
  - **`psram::AllocatorMemorySnapshot`'s field-level split from Phase 4 (E-0007) held exactly**:
    the fourteen fields `net_host.rs`'s port adapter reads came back `pub` from the compiler-driven
    process on their own; the seven `min_internal_alloc_*` correlation-diagnostic fields nothing
    outside the crate reads stayed `pub(crate)`, confirming without re-deriving it that Phase 4's
    narrower `netstack::host::AllocatorMemorySnapshot` mirror was scoped correctly.
  - **Five additional narrowing opportunities the compiler surfaced only as lint warnings, not hard
    errors**, fixed anyway since they are exactly Phase 5's subject: `DiagKind`, `DiagTargets`,
    `TouchSampleFrame`, `TouchStatus`, and `Gpio36Action` were each `pub(crate)` while used as a
    field/variant type inside a struct or enum the revert had already restored to `pub`
    (`private_interfaces`-family warnings, "type X is more private than the item that uses it").
    Promoted each type declaration itself (not necessarily every method or field on it) to clear the
    warning. `config::channels`'s glob re-export narrowed from `pub use channels::*` to
    `pub(crate) use channels::*` after Phase 4 (E-0007) moved every Wi-Fi channel out, leaving
    nothing in it that needs external visibility (the compiler's own "glob import doesn't reexport
    anything with visibility `pub`" warning, not an error).
  - **`Snapshot::wifi_ipv4` stays as a known, unfixed `dead_code` warning**: `cargo clippy
    --all-features -- -D warnings` reports it unread even after every other finding above was
    cleared. Left alone deliberately -- removing a field is a behavior/API decision, not a
    visibility one, and outside this phase's narrowing scope; recorded here rather than silently
    passed over.
  - **`-D warnings` does not gate `meditamer-product` as a dependency of this invocation**:
    confirmed empirically (forced a fresh compile via `touch` on the source files, still exit 0 with
    the five warnings above visible but not failing the build) -- extra rustc flags passed to
    `cargo clippy` inside `targets/meditamer-inkplate` apply to that crate's own compilation, not to
    `products/meditamer` as a path dependency in the same invocation. This matches how every prior
    phase's clippy commands were run and reported clean; it is a pre-existing characteristic of the
    validation routine, not something this phase changed, and the reason the five warnings above
    needed manual review rather than surfacing as build failures.
  - **No ELF comparison**: visibility modifiers (`pub`/`pub(crate)`) are erased entirely by codegen
    and have no effect on emitted code, unlike Phase 4's crate-boundary code motion; running the same
    `xtensa-esp32-elf-size -A` diff E-0004/E-0007 used would be a redundant check for this class of
    change, not a stronger one.
- Validation, in the repository's `archbox` distrobox with the xtensa toolchain on `PATH`:
  - `cargo clippy --target xtensa-esp32-none-elf -Zbuild-std=core,alloc -- -D warnings` clean for
    `default`, `--all-features`, `--no-default-features` (minimal), `--features
    wifi-debug-slim-app --no-default-features` (slim), `--features telemetry-defmt`, `--features
    ble-foundation`, and `--no-default-features --features factory-updater --bin updater`; repeated
    with `--all-targets` for `default`, `--all-features`, and minimal, also clean.
  - `./build.sh release default|minimal` and `./build.sh ble-release default` all link real release
    artifacts end to end.
  - `scripts/ci/check_orphan_modules.py`: 651 tracked files, 57 target roots, 31 manifests, zero
    unreachable -- identical counts to E-0007 (Phase 5 changes visibility keywords only, no files
    moved or added).
  - `scripts/ci/check_rust_loc.sh`: 580 files, same warning/advisory set as E-0007 plus `ble/mod.rs`
    shifting from 726 to 729 lines (reformatting from the visibility-keyword-length changes, not new
    content); no new high-attention file.
  - `scripts/ci/check_script_surface.py`: clean (47/47).
  - `cargo fmt --all --check` (root) and `--check` against `targets/meditamer-inkplate`: clean, after
    `cargo fmt --all` absorbed the wrap-threshold shifts several `pub(crate)` -> `pub` promotions
    produced.
  - `cargo test -p medinote -p runtime -p arbitration` (10/10, 2/2, 15/15) and
    `test-support/host/{net_status,ble_transport}_host_harness` (1/1, 17/17): all pass, unaffected --
    Phase 5 does not touch any file these reach via `#[path]` shims or the shared workspace.
  - Regression, unaffected by this phase: `boards/waveshare-rlcd42` and `targets/medinote-waveshare`
    (untouched by this phase's changes to `products/meditamer`).
- Evidence class: source-verified, host- and cross-toolchain-verified across every build profile and
  binary this crate produces; not hardware-verified (no behavior changed, only compile-time
  visibility, so this class of change carries a materially lower hardware-regression risk than
  Phase 4's, but the live Wi-Fi regression gate from E-0007/E-0008 remains outstanding regardless).
- Next action: run the Meditamer Wi-Fi regression gate when hardware is available (E-0007/E-0008's
  outstanding item, unaffected by this phase). Phase 5's F5 disposition is complete; F2-F4 remain
  their recorded "keep as-is, revisit when a consumer needs it" dispositions, not further action
  items. Phase 5 passes on that basis.

Future entries record the source baseline, phase and scope, material decisions or deviations, exact
commands and results, produced artifacts, hardware evidence when applicable, and the next gate.

### E-0010 — Phase 4 hardware gate and closeout

- Date/phase: 2026-09-07; Phase 4 completion using retained device evidence.
- Supersession: closes the pending Inkplate Wi-Fi gate in E-0007/E-0008 and
  E-0009's next action. Their historical implementation and validation evidence
  remains unchanged. Medinote qualification remains owned by the parity plans.
- Artifact: Inkplate ELF SHA-256
  `cf94739581b7a376126de460394b1bef2ae4debf0a39ccec2de458e2eda9c4b0`,
  identified in [hardware matrix §2O](../../reference/hardware-test-matrix.md#2o-shared-network-and-http-service-2026-09-07).
- Device evidence:
  `logs/shared-storage-inkplate/final-inkplate-20260907_160605/wifi-regression/report.json`,
  run `20260907_160713`, reports `final_status=passed`: discovery, one-cycle and
  three-cycle acceptance passed; `panic_detected=false` and
  `unexpected_reboot_detected=false`. Soak was skipped. This is retained
  identified-artifact evidence, not a new hardware run during closeout.
- Source spot-check: no `net` directory remains under `products/meditamer`;
  `platform/netstack` and `platform/runtime` manifests have no product/board
  dependencies; the Inkplate target consumes netstack with ESP32 selection.
  E-0007 remains the source of the extraction's full host/build/ELF validation;
  this closeout does not claim those suites were freshly rerun on later work.
- Result: Phase 4 is Passed and its design plan is Done. F1 network operational
  truth is delivered. Later upload-service policy qualification and broader
  parity acceptance remain open under their own plans.
- Next action: final closeout review of the parent, whose five phases now pass.

### E-0011 — Product and target axis final closeout

- Date: 2026-09-07; user-authorized final closeout.
- Completion review: phases 1–3 establish product/board/target roots and ownership
  (E-0002–E-0004); Phase 4 supplies the shared runtime ports and hardware gate
  (E-0007, E-0010); Phase 5 resolves public surfaces and inherited dispositions
  (E-0009). All five phase-summary entries are Passed.
- Result: parent and ledger are Done and archived together. This relies on the
  recorded phase evidence; it does not claim a new full-suite run against the
  accumulated working tree. Connectivity, storage and UI qualification beyond
  the extraction scope retain their active plan owners.
