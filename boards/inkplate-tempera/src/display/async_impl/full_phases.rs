//! Binary full-refresh waveform phases.
//!
//! Phase helpers extracted from `display_bw_waveform_inner_async` so the
//! waveform sequencer stays under the function-complexity gates. Ordering,
//! cancellation (`?`), profiling accounting, and refresh timing are identical:
//! each helper owns one phase's `FullRefreshTiming` slot, and the shared
//! `vscan_start_us` / `source_scan_us` / `pass_delay_us` accumulators grow in
//! the same call order as the inline sequence did. The hot GPIO scan loops
//! stay in `full_scan.rs` under their existing RAM placement; these helpers
//! are cold control flow and carry no placement attribute.

use super::super::super::{
    DelayOps, FullRefreshTiming, I2cOps, InkplateHal, Result, LUT2, LUTB,
    PANEL_FULL_INTER_PASS_DELAY_US,
};
use embassy_time::Instant;

/// Reads the profiling clock, or returns zero when profiling is compiled out.
/// Keeps the `PROFILE=false` path free of clock reads, as before.
#[inline(always)]
fn profile_now<const PROFILE: bool>() -> u64 {
    if PROFILE {
        Instant::now().as_micros()
    } else {
        0
    }
}

/// Attributes elapsed time since `started_us` to `slot` when profiling.
#[inline(always)]
fn accumulate_since<const PROFILE: bool>(slot: &mut u64, started_us: u64) {
    if PROFILE {
        *slot = slot.saturating_add(Instant::now().as_micros().saturating_sub(started_us));
    }
}

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    /// Runs the five initial clean passes and records `initial_clean_us`.
    pub(super) async fn bw_initial_clean_phase<const PROFILE: bool>(
        &mut self,
        timing: &mut FullRefreshTiming,
    ) -> Result<(), I2C::Error> {
        let phase_started_us = profile_now::<PROFILE>();
        if PROFILE {
            self.clean_full_refresh_profiled_async(0, 5, timing).await?;
            self.clean_full_refresh_profiled_async(1, 15, timing)
                .await?;
            self.clean_full_refresh_profiled_async(0, 15, timing)
                .await?;
            self.clean_full_refresh_profiled_async(1, 15, timing)
                .await?;
            self.clean_full_refresh_profiled_async(0, 15, timing)
                .await?;
        } else {
            self.clean_full_refresh_async(0, 5).await?;
            self.clean_full_refresh_async(1, 15).await?;
            self.clean_full_refresh_async(0, 15).await?;
            self.clean_full_refresh_async(1, 15).await?;
            self.clean_full_refresh_async(0, 15).await?;
        }
        if PROFILE {
            timing.initial_clean_us = Instant::now().as_micros().saturating_sub(phase_started_us);
        }
        Ok(())
    }

    /// Runs the ten LUTB framebuffer passes and records `framebuffer_us`.
    pub(super) async fn bw_framebuffer_phase<const PROFILE: bool>(
        &mut self,
        timing: &mut FullRefreshTiming,
    ) -> Result<(), I2C::Error> {
        let phase_started_us = profile_now::<PROFILE>();
        for _ in 0..10 {
            let pass_started_us = profile_now::<PROFILE>();
            self.vscan_start().await?;
            accumulate_since::<PROFILE>(&mut timing.vscan_start_us, pass_started_us);
            let pass_started_us = profile_now::<PROFILE>();
            self.scan_full_binary_pass(&LUTB);
            accumulate_since::<PROFILE>(&mut timing.source_scan_us, pass_started_us);
            if PANEL_FULL_INTER_PASS_DELAY_US != 0 {
                let pass_started_us = profile_now::<PROFILE>();
                self.delay.delay_us(PANEL_FULL_INTER_PASS_DELAY_US);
                accumulate_since::<PROFILE>(&mut timing.pass_delay_us, pass_started_us);
            }
        }
        if PROFILE {
            timing.framebuffer_us = Instant::now().as_micros().saturating_sub(phase_started_us);
        }
        Ok(())
    }

    /// Runs the single LUT2 settle pass and records `settle_us`.
    pub(super) async fn bw_settle_phase<const PROFILE: bool>(
        &mut self,
        timing: &mut FullRefreshTiming,
    ) -> Result<(), I2C::Error> {
        let phase_started_us = profile_now::<PROFILE>();
        let pass_started_us = profile_now::<PROFILE>();
        self.vscan_start().await?;
        accumulate_since::<PROFILE>(&mut timing.vscan_start_us, pass_started_us);
        let pass_started_us = profile_now::<PROFILE>();
        self.scan_full_binary_pass(&LUT2);
        accumulate_since::<PROFILE>(&mut timing.source_scan_us, pass_started_us);
        if PANEL_FULL_INTER_PASS_DELAY_US != 0 {
            let pass_started_us = profile_now::<PROFILE>();
            self.delay.delay_us(PANEL_FULL_INTER_PASS_DELAY_US);
            accumulate_since::<PROFILE>(&mut timing.pass_delay_us, pass_started_us);
        }
        if PROFILE {
            timing.settle_us = Instant::now().as_micros().saturating_sub(phase_started_us);
        }
        Ok(())
    }

    /// Runs the two final clean passes and records `final_clean_us`.
    pub(super) async fn bw_final_clean_phase<const PROFILE: bool>(
        &mut self,
        timing: &mut FullRefreshTiming,
    ) -> Result<(), I2C::Error> {
        let phase_started_us = profile_now::<PROFILE>();
        if PROFILE {
            self.clean_full_refresh_profiled_async(2, 1, timing).await?;
            self.clean_full_refresh_profiled_async(3, 1, timing).await?;
        } else {
            self.clean_full_refresh_async(2, 1).await?;
            self.clean_full_refresh_async(3, 1).await?;
        }
        if PROFILE {
            timing.final_clean_us = Instant::now().as_micros().saturating_sub(phase_started_us);
        }
        Ok(())
    }

    /// Runs the terminal vscan hold and records `terminal_vscan_us`.
    pub(super) async fn bw_terminal_vscan_phase<const PROFILE: bool>(
        &mut self,
        timing: &mut FullRefreshTiming,
    ) -> Result<(), I2C::Error> {
        let phase_started_us = profile_now::<PROFILE>();
        self.vscan_start().await?;
        if PROFILE {
            timing.terminal_vscan_us = Instant::now().as_micros().saturating_sub(phase_started_us);
        }
        Ok(())
    }
}
