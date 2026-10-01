//! PSRAM-owned mask context, row scratch, and per-frame work counters.
//! Imported dial artwork has one stationary region class; no metadata plane.

use allocator_api2::vec::Vec;
use analog_clock::{
    prepare_mask_row, shade_cached_row_with_masks, CachedRowBuffers, ClockScene, DitherRegion,
    Footprint, Hands, MaskContext, MaskRowBuffers,
};
use embassy_time::Instant;

use super::{psram_vec, ClockFailure, CLOCK_WIDTH, GRAY_CACHE_LEN, MASK_SCRATCH_LEN};

pub(super) struct RowOutputs<'a> {
    pub gray: &'a mut [u8],
    pub regions: &'a mut [DitherRegion],
    pub footprint: &'a mut [Footprint],
}

/// The engine retains only a Vec handle; this value and its context live in PSRAM.
pub(super) struct ShadowWorkspace {
    context: Option<MaskContext>,
    counts: [Vec<u8, esp_alloc::ExternalMemory>; 3],
    base_regions: Vec<DitherRegion, esp_alloc::ExternalMemory>,
    rows: u32,
    candidate: u64,
    retained: u64,
    alpha_queries: u64,
    mask_us: u64,
    material_us: u64,
    paint_queries: u64,
    tap_iterations: u64,
}

pub(super) fn allocate() -> Result<Vec<ShadowWorkspace, esp_alloc::ExternalMemory>, ClockFailure> {
    let mut owned = Vec::new_in(esp_alloc::ExternalMemory);
    owned
        .try_reserve_exact(1)
        .map_err(|_| ClockFailure::Workspace)?;
    let width = CLOCK_WIDTH as usize;
    owned.push(ShadowWorkspace {
        context: None,
        counts: [
            psram_vec(width, 0)?,
            psram_vec(width, 0)?,
            psram_vec(width, 0)?,
        ],
        base_regions: psram_vec(width, DitherRegion::Background)?,
        rows: 0,
        candidate: 0,
        retained: 0,
        alpha_queries: 0,
        mask_us: 0,
        material_us: 0,
        paint_queries: 0,
        tap_iterations: 0,
    });
    Ok(owned)
}

impl ShadowWorkspace {
    pub(super) fn arm(&mut self, scene: &ClockScene, hands: &Hands<'_>) {
        self.context = Some(MaskContext::for_frame(scene, hands));
        self.rows = 0;
        self.candidate = 0;
        self.retained = 0;
        self.alpha_queries = 0;
        self.mask_us = 0;
        self.material_us = 0;
        self.paint_queries = 0;
        self.tap_iterations = 0;
    }

    pub(super) fn render_row(
        &mut self,
        scene: &ClockScene,
        hands: &Hands<'_>,
        y: u32,
        base_gray: &[u8],
        out: RowOutputs<'_>,
    ) -> bool {
        let Some(context) = self.context.as_ref() else {
            return false;
        };
        let width = CLOCK_WIDTH as usize;
        if base_gray.len() != width
            || out.gray.len() != width
            || out.regions.len() != width
            || out.footprint.len() != width
        {
            return false;
        }
        let [dial, lower, upper] = &mut self.counts;
        let start_us = Instant::now().as_micros();
        let work = prepare_mask_row(context, hands, y, dial, lower, upper);
        let material_start_us = Instant::now().as_micros();
        self.mask_us += material_start_us.saturating_sub(start_us);
        if work.stats.pixels != CLOCK_WIDTH {
            return false;
        }
        shade_cached_row_with_masks(
            scene,
            hands,
            context,
            y,
            CachedRowBuffers {
                base_gray,
                base_regions: &self.base_regions,
                full_gray: out.gray,
                full_regions: out.regions,
                footprint: out.footprint,
            },
            MaskRowBuffers {
                dial_open: dial,
                lower_open: lower,
                upper_open: upper,
            },
            work.affected_range,
        );
        self.material_us += Instant::now().as_micros().saturating_sub(material_start_us);
        let (lo, hi) = work.affected_range;
        self.rows += 1;
        self.candidate += (hi - lo) as u64;
        self.retained += (width - (hi - lo)) as u64;
        self.alpha_queries += u64::from(work.stats.alpha_queries);
        self.paint_queries += u64::from(work.paint_queries);
        self.tap_iterations += u64::from(work.tap_iterations);
        true
    }

    pub(super) fn report(&self, epoch: u64) {
        let taps = self.context.as_ref().map_or(0, MaskContext::taps_n);
        console::println!(
            "CLOCK_MASKS epoch={} rows={} candidate={} retained={} alpha_queries={} taps={} count_scratch_bytes={} gray_cache_bytes={} stationary_region_bytes={} context_bytes={}",
            epoch, self.rows, self.candidate, self.retained, self.alpha_queries, taps,
            MASK_SCRATCH_LEN, GRAY_CACHE_LEN, self.base_regions.len(), core::mem::size_of::<MaskContext>(),
        );
        console::println!(
            "CLOCK_MASK_PHASES epoch={} mask_us={} material_us={} paint_queries={} tap_iterations={}",
            epoch, self.mask_us, self.material_us, self.paint_queries, self.tap_iterations,
        );
    }
}
