# Shanshui Generator Implementation Plan

## Objective

Implement a deterministic, no_std Shanshui landscape renderer for the Inkplate 600x600 ESP32 target, integrated into the current display runtime without regressing clock mode stability.

## Why This Plan

The source design brief is strong on architecture, but not implementation detail. This plan translates it into concrete repository changes, phase gates, and hardware validation criteria.

## Current Baseline (Repository-Specific)

- Runtime target is `esp-hal` on `esp32` (`xtensa-esp32-none-elf`).
- Display pipeline today is framebuffer-based 1-bit (`src/drivers/inkplate/mod.rs` + `src/drivers/inkplate/display.rs`).
- Existing procedural renderers are:
  - `src/firmware/graphics/suminagashi.rs` (fixed-point marbling + threshold dithering)
  - `src/firmware/graphics/sumi_sun.rs` (fixed-point sun disk rendering)
- Current visual mode switch is driven by `display_task` in `src/firmware/runtime/display_task.rs` using `DisplayMode` from `src/firmware/types/modes.rs`.
- Existing timing telemetry records marble redraw duration in `src/firmware/render/visual.rs` via `LAST_MARBLE_REDRAW_MS` / `MAX_MARBLE_REDRAW_MS`.

## Scope

- Add a new Shanshui renderer module and wire it into main display mode flow.
- Keep the existing Inkplate HAL framebuffer path unchanged for first delivery.
- Use fixed-point math in per-pixel hot paths.
- Implement layered terrain, fog/void, and deterministic tree placement.
- Add Atkinson diffusion using a compact rolling error buffer, then convert to binary output for the existing panel update path.
- Add on-device validation procedure and measurable performance gates.

## Non-Goals (Phase 1)

- No rewrite of Inkplate low-level waveform/update code.
- No dependency on external PSRAM.
- No full grayscale e-ink transport path rewrite.
- No dynamic allocation in render hot path.

## Target Architecture

## Module Layout

- `src/firmware/graphics/shanshui.rs` (or `src/firmware/graphics/shanshui.rs` if split):
  - Fixed-point aliases/constants (reuse `fixed::types::I16F16` initially).
  - Coordinate mapping and deterministic seed mixing.
  - Terrain SDF evaluator (background/mid/foreground layers).
  - Cunfa texturing functions:
    - domain-warped noise (Pima-like)
    - ridged multifractal (Fupi-like)
  - Fog/water/void blending.
  - Stateless tree placement via hashed columns and implicit branch tests.
  - `render_rows_bw(...)` API with callback signature matching existing row renderers.
- `src/firmware/mod.rs`:
  - Export graphics modules under `firmware::graphics`.
- `src/firmware/types/modes.rs` and `src/firmware/runtime/display_task.rs`:
  - Extend `DisplayMode` to include `Shanshui`.
  - Add render dispatch integration and mode toggling behavior.
- `docs/development/hardware-test-matrix.md`:
  - Add Shanshui-specific soak and visual checks.

## Data and State Model

- Keep scene evaluation stateless per pixel.
- Permit minimal state only for diffusion:
  - rolling error buffers for two future rows plus current row window.
  - target memory cap: <= 8 KB additional SRAM.
- No frame-sized grayscale buffers.

## Implementation Phases

## Phase 0: Design Freeze (1 day)

- Finalize fixed-point format (`I16F16` for v1).
- Finalize public render API and integration constants.
- Define deterministic seed contract (`same seed -> same pixels`).
- Define first-pass visual style constants and compile-time toggles.

Deliverables:

- This plan checked in and approved.
- Constant table block added to new `shanshui` module header.

## Phase 1: Renderer Scaffold and Integration (1-2 days)

- Create `shanshui` module skeleton with:
  - `build_params(seed, size)` (if needed)
  - `render_shanshui_rows_bw(width, height, y_start, y_end, seed, put_black_pixel)`
- Integrate new mode in `src/firmware/runtime/display_task.rs`:
  - add `DisplayMode::Shanshui`
  - wire into `render_active_mode(...)`
  - preserve existing Clock and Suminagashi behavior.
- Add `FORCE_SHANSHUI_MODE` or equivalent compile-time toggle for bring-up.

Exit criteria:

- Compiles and renders a simple deterministic mountain silhouette.
- No regressions in existing mode persistence logic.

## Phase 2: Terrain SDF + Cunfa Textures (2-3 days)

- Implement layered SDF mountains:
  - 3 layers: far/mid/near.
  - smooth union (`smin`) for overlap.
- Add low-frequency macro shape modulation.
- Add two texture channels:
  - domain-warped noise for soft striations.
  - ridged multifractal for hard rock facets.
- Blend texture intensity by local slope estimate from SDF gradient.

Exit criteria:

- Visual separation between soft and hard mountain regions.
- Stable deterministic output across repeated runs with same seed.

## Phase 3: Trees, Fog, and Water/Void (2-3 days)

- Tree placement:
  - hash column ID to deterministic spawn and morphology.
  - implicit trunk/branch distance tests (capsule-like).
  - dot foliage mask via high-frequency cellular/hash noise approximation.
- Fog:
  - distance-to-terrain based attenuation with low-frequency perturbation.
- Water/void:
  - lower-region masking with flow-like noise ridges, preserving white negative space.

Exit criteria:

- Trees appear without persistent geometry buffers.
- Foreground void remains mostly clean (Liu Bai preserved).

## Phase 4: Quantization + Atkinson Rolling Diffusion (2 days)

- Add linear-to-display transfer step (gamma LUT or tuned approximation).
- Add Atkinson diffusion in streaming row renderer:
  - propagate `error >> 3` to forward neighbors.
  - keep bounded rolling buffers only.
- Emit final binary pixel callback for existing framebuffer path.

Exit criteria:

- Dithering produces cleaner ink-like clusters than static threshold path.
- Additional SRAM remains within target cap.

## Phase 5: Performance Tuning and Telemetry (1-2 days)

- Add timing metrics around Shanshui render loop in `src/firmware/render/visual.rs`.
- Tune constants for acceptable latency on device.
- Avoid expensive operations in hot path:
  - remove divisions where possible
  - prefer shifts and multiply-by-constants
  - reduce octave count where visually acceptable.

Exit criteria:

- Documented render-time distribution from hardware runs (at least 30 frames).
- No watchdog resets or panic in render loop.

## Phase 6: Hardware Validation and Soak (1-2 days)

- Extend soak scripts or add targeted script for repeated Shanshui redraw.
- Run:
  - 100 forced Shanshui full refreshes
  - mixed mode toggling stress (Clock <-> Suminagashi <-> Shanshui)
- Record failures, max render time, and reset count.

Exit criteria:

- Zero panic/reboot in the validation window.
- Visual checklist passes on real panel.

## Detailed Work Items

- [ ] Add `src/firmware/graphics/shanshui.rs` with deterministic seed mixer and fixed-point utilities.
- [ ] Implement base SDF landscape function and 3-layer composition.
- [ ] Implement ridge and warp texture evaluators.
- [ ] Implement local gradient estimator and ink-density modulation.
- [ ] Implement deterministic tree column hashing and implicit branch tests.
- [ ] Implement fog and water/void masking.
- [ ] Implement row-stream Atkinson diffusion with rolling buffers.
- [ ] Integrate `DisplayMode::Shanshui` in `src/firmware/runtime/display_task.rs`.
- [ ] Add render-time counters/log markers for Shanshui path.
- [ ] Update `docs/development/hardware-test-matrix.md` with Shanshui checks.

## Acceptance Criteria

- Functional:
  - New Shanshui mode renders full 600x600 scenes on hardware.
  - Same seed and build produce identical output.
  - Existing Clock mode still refreshes correctly.
- Performance:
  - Render time and max render time are logged and documented.
  - No runtime instability during 100-refresh soak.
- Visual:
  - Distinct layered mountains.
  - Visible texture differences (soft vs hard faces).
  - Deterministic tree presence.
  - Preserved white negative space in lower/water domain.

## Risks and Mitigations

- Risk: render latency too high on ESP32.
  - Mitigation: reduce octave count, reduce supersampling, simplify gradient estimates, precompute small LUTs.
- Risk: dithering state increases memory unexpectedly.
  - Mitigation: fixed-size static buffers with compile-time bounds checks.
- Risk: visual clutter destroys Liu Bai effect.
  - Mitigation: strict water-domain density clamp and explicit white-space quota target.
- Risk: mode-switch regressions.
  - Mitigation: keep mode integration minimal and add mixed-mode soak run.

## Open Decisions

- Whether Shanshui replaces Suminagashi or ships as an additional mode long-term.
- Whether Gray4 output path is needed after v1 binary mode.
- Final acceptable render-time budget per full refresh on target hardware.

## Suggested Next Task

Implement Phase 1 only (scaffold + mode integration + simple silhouette) and validate hardware bring-up before starting texture complexity.
