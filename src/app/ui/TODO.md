# Hybrid UI Implementation Plan (1-bit Panel, Dithered Visuals)

## Scope
- Add a dedicated `ui` module for a hybrid pipeline:
  - vector-like rendering for controls
  - post-dither to 1-bit
  - output through existing BW Inkplate display path
- Preserve current task/event architecture.

## Current State
- Display output is BW (`BinaryColor`) with existing full/partial update behavior.
- Touch pipeline and wizard UI are functional and already integrated.
- Blue-noise assets and dithering logic already exist in rendering code.

## Goals
- Better visual quality for buttons/cards/shadows while staying 1-bit on panel.
- Incremental rollout without replacing the full app renderer at once.
- Keep deterministic refresh behavior suitable for e-paper.

## Phase 1: Module Skeleton
- Create `src/app/ui/mod.rs` and split responsibilities:
  - `model.rs` (UI state)
  - `layout.rs` (widget bounds)
  - `style.rs` (theme tokens)
  - `dither.rs` (thresholding and masks)
  - `renderer.rs` (hybrid render entrypoints)
- Keep public API minimal:
  - `render_ui_full(...)`
  - `render_ui_dirty(...)`

## Phase 2: Render Surface Strategy
- Start with a small grayscale work buffer for dirty regions (not full screen).
- Render visual primitives into the work buffer.
- Convert grayscale to 1-bit via blue-noise threshold.
- Blit into existing BW framebuffer through current driver API.

## Phase 3: First Screen Migration
- Implement one representative screen (for example a settings/menu card with buttons).
- Keep text rendering on existing font path first.
- Validate tap targets and interaction feedback against current touch pipeline.

## Phase 4: Dirty Rect and Refresh Policy
- Track dirty rectangles per interaction.
- Use partial update when region and state allow it.
- Fall back to full update when accumulated artifacts/ghosting risk increases.
- Add explicit policy constants in config for easy tuning.

## Phase 5: Integration and Hardening
- Route UI events from existing app event loop into UI module.
- Add performance logs: render ms, dither ms, flush ms.
- Add regression checks for touch responsiveness and redraw stability.

## Risks
- Grayscale work buffers can increase memory pressure without PSRAM allocator support.
- Over-aggressive partial updates can increase ghosting.
- If tiny-skia is used, text support and compile footprint must be controlled carefully.

## Exit Criteria
- At least one production screen uses hybrid renderer.
- Dithered controls/shadows are visibly improved over pure primitive BW rendering.
- No regressions in touch UX and task stability.
- Update latency remains acceptable for target e-paper interaction model.
