//! Modest device phase profiling for one analog-clock compose frame.
//!
//! Inline metadata only: no globals, no statics, no buffers, no tasks, no
//! channels, no new dependencies, no per-pixel timing. The owning
//! [`super::RenderEngine`] holds one value; the wall clock is
//! [`embassy_time::Instant`], so each interval is a wall interval that can
//! include ISR/preemption time. The residual `gap_us` is wall minus active
//! and is therefore not claimed to be pure Embassy sleep or pure CPU.
//!
//! Scope: asset loading, the stationary dial base pass, held publication,
//! and panel time are never measured here. The wall interval starts when
//! [`super::RenderEngine`] actually arms a fresh frame and ends when that
//! frame completes, so queued/inactive waiting before the arm is excluded.
//! Dither output is labeled `dither_pack` because the streaming pass writes
//! packed bits through the bit writer directly; there is no separate
//! dither-vs-pack split to report.

use embassy_time::Instant;

/// Wall-clock microseconds right now on the device monotonic clock.
fn now_us() -> u64 {
    Instant::now().as_micros()
}

/// Per-frame phase accounting, stored inline on the render engine.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FrameProfile {
    epoch_minute: u64,
    rows: u32,
    pixels: u32,
    batches: u32,
    unchanged_pixels: u32,
    hand_pixels: u32,
    shadow_pixels: u32,
    wall_start_us: u64,
    wall_us: u64,
    active_us: u64,
    shade_us: u64,
    dither_pack_us: u64,
    restore_us: u64,
    started: bool,
}

impl FrameProfile {
    /// Zeroed profile; a real frame interval starts with [`reset`](Self::reset).
    pub const fn new() -> Self {
        Self {
            epoch_minute: 0,
            rows: 0,
            pixels: 0,
            batches: 0,
            unchanged_pixels: 0,
            hand_pixels: 0,
            shadow_pixels: 0,
            wall_start_us: 0,
            wall_us: 0,
            active_us: 0,
            shade_us: 0,
            dither_pack_us: 0,
            restore_us: 0,
            started: false,
        }
    }

    /// Start a fresh frame interval. Called only when the engine actually
    /// arms a fresh target, never on a duplicate retarget or a service tick.
    pub fn reset(&mut self, epoch_minute: u64) {
        self.reset_at(epoch_minute, now_us());
    }

    fn reset_at(&mut self, epoch_minute: u64, start_us: u64) {
        *self = Self {
            epoch_minute,
            wall_start_us: start_us,
            started: true,
            ..Self::new()
        };
    }

    /// One composed output row of `row_pixels` pixels finished.
    pub fn note_row(&mut self, row_pixels: u32) {
        self.rows = self.rows.saturating_add(1);
        self.pixels = self.pixels.saturating_add(row_pixels);
    }

    /// Classify the pixels selected by the existing restoration loop.
    pub fn note_footprint(&mut self, unchanged: u32, hand: u32, shadow: u32) {
        self.unchanged_pixels = self.unchanged_pixels.saturating_add(unchanged);
        self.hand_pixels = self.hand_pixels.saturating_add(hand);
        self.shadow_pixels = self.shadow_pixels.saturating_add(shadow);
    }

    pub fn unchanged_pixels(&self) -> u32 {
        self.unchanged_pixels
    }
    pub fn hand_pixels(&self) -> u32 {
        self.hand_pixels
    }
    pub fn shadow_pixels(&self) -> u32 {
        self.shadow_pixels
    }

    /// One `service_frame_rows` work call finished `active_call_us` of work.
    pub fn note_batch(&mut self, active_call_us: u64) {
        self.batches = self.batches.saturating_add(1);
        self.active_us = self.active_us.saturating_add(active_call_us);
    }

    /// One cached mask row call cost `us` (visibility and affected-pixel shading).
    pub fn add_shade(&mut self, us: u64) {
        self.shade_us = self.shade_us.saturating_add(us);
    }

    /// One `StreamingDither::process_row` call cost `us` (diffusion plus the
    /// packed-bit writes it issues through the bit writer).
    pub fn add_dither_pack(&mut self, us: u64) {
        self.dither_pack_us = self.dither_pack_us.saturating_add(us);
    }

    /// One cached-base restoration loop cost `us`.
    pub fn add_restore(&mut self, us: u64) {
        self.restore_us = self.restore_us.saturating_add(us);
    }

    /// Close the frame interval at completion and freeze `wall_us`.
    pub fn finish(&mut self) {
        self.finish_at(now_us());
    }

    fn finish_at(&mut self, end_us: u64) {
        if self.started {
            self.wall_us = end_us.saturating_sub(self.wall_start_us);
        }
    }

    pub fn epoch_minute(&self) -> u64 {
        self.epoch_minute
    }

    pub fn rows(&self) -> u32 {
        self.rows
    }

    pub fn pixels(&self) -> u32 {
        self.pixels
    }

    pub fn batches(&self) -> u32 {
        self.batches
    }

    pub fn wall_us(&self) -> u64 {
        self.wall_us
    }

    pub fn active_us(&self) -> u64 {
        self.active_us
    }

    pub fn shade_us(&self) -> u64 {
        self.shade_us
    }

    pub fn dither_pack_us(&self) -> u64 {
        self.dither_pack_us
    }

    pub fn restore_us(&self) -> u64 {
        self.restore_us
    }

    #[cfg(test)]
    pub fn started(&self) -> bool {
        self.started
    }

    /// Wall time not attributed to `service_frame_rows` work: inter-batch
    /// scheduling residual, never a pure-sleep or pure-CPU claim.
    pub fn gap_us(&self) -> u64 {
        self.wall_us.saturating_sub(self.active_us)
    }
}

#[cfg(test)]
mod tests {
    use super::FrameProfile;

    #[test]
    fn reset_starts_fresh_interval() {
        let mut profile = FrameProfile::new();
        profile.reset_at(42, 1_000);
        profile.note_row(600);
        profile.note_footprint(590, 6, 4);
        profile.note_batch(50);
        profile.add_shade(7);
        profile.finish_at(1_200);
        assert_eq!(profile.epoch_minute(), 42);
        assert_eq!(profile.rows(), 1);
        assert_eq!(profile.unchanged_pixels(), 590);
        assert_eq!(profile.hand_pixels(), 6);
        assert_eq!(profile.shadow_pixels(), 4);
        assert_eq!(profile.pixels(), 600);
        assert_eq!(profile.batches(), 1);
        assert_eq!(profile.wall_us(), 200);
        assert_eq!(profile.gap_us(), 150);

        profile.reset_at(43, 5_000);
        assert!(profile.started());
        assert_eq!(profile.epoch_minute(), 43);
        assert_eq!(profile.rows(), 0);
        assert_eq!(profile.unchanged_pixels(), 0);
        assert_eq!(profile.hand_pixels(), 0);
        assert_eq!(profile.shadow_pixels(), 0);
        assert_eq!(profile.pixels(), 0);
        assert_eq!(profile.batches(), 0);
        assert_eq!(profile.wall_us(), 0);
        assert_eq!(profile.active_us(), 0);
        assert_eq!(profile.shade_us(), 0);
        assert_eq!(profile.dither_pack_us(), 0);
        assert_eq!(profile.restore_us(), 0);
        assert_eq!(profile.gap_us(), 0);
    }

    #[test]
    fn aggregation_saturates_instead_of_wrapping() {
        let mut profile = FrameProfile::new();
        profile.reset_at(7, 0);
        profile.add_shade(u64::MAX);
        profile.add_shade(1);
        assert_eq!(profile.shade_us(), u64::MAX);
        profile.add_dither_pack(u64::MAX);
        profile.add_restore(u64::MAX);
        profile.note_batch(u64::MAX);
        profile.note_batch(u64::MAX);
        assert_eq!(profile.active_us(), u64::MAX);
        assert_eq!(profile.batches(), 2);
        profile.finish_at(u64::MAX);
        assert_eq!(profile.wall_us(), u64::MAX);
        assert_eq!(profile.gap_us(), 0);
    }

    #[test]
    fn gap_never_underflows_when_active_exceeds_wall() {
        let mut profile = FrameProfile::new();
        profile.reset_at(9, 1_000);
        profile.note_batch(500);
        profile.finish_at(1_100);
        assert_eq!(profile.wall_us(), 100);
        assert_eq!(profile.gap_us(), 0);
    }
}
