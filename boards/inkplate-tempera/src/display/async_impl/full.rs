use super::super::super::{
    DelayOps, FullRefreshTiming, I2cOps, InkplateHal, PanelRefreshError, PanelRefreshErrorStage,
    PanelRefreshFallback, PanelRefreshOutcome, PanelRefreshResult, Result,
    GRAYSCALE_FRAMEBUFFER_BYTES,
};
use core::future::Future;
use embassy_time::Instant;

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    pub async fn display_bw_async(&mut self, leave_on: bool) -> Result<(), I2C::Error> {
        if let Err(error) = self.eink_on_async().await {
            self.partial_ready = false;
            return Err(error);
        }

        let waveform = self.display_bw_waveform_async().await;
        let transaction = self.finalize_display_transaction(waveform, leave_on).await;

        if transaction.is_ok() {
            if let Some(previous) = self.framebuffer_bw_previous.as_deref_mut() {
                previous.copy_from_slice(self.framebuffer_bw);
                self.partial_ready = true;
            }
        } else {
            self.partial_ready = false;
        }

        transaction
    }

    /// Runs the production full-refresh transaction and reports coarse phase
    /// timings. The refresh probe uses this explicit entry point so normal
    /// callers do not pay measurement overhead.
    pub async fn display_bw_timed_async(
        &mut self,
        leave_on: bool,
    ) -> Result<FullRefreshTiming, I2C::Error> {
        let total_started_us = Instant::now().as_micros();
        let power_on_started_us = Instant::now().as_micros();
        if let Err(error) = self.eink_on_async().await {
            self.partial_ready = false;
            return Err(error);
        }
        let power_on_us = Instant::now()
            .as_micros()
            .saturating_sub(power_on_started_us);

        let waveform = self.display_bw_waveform_timed_async().await;
        let (waveform_result, timing) = match waveform {
            Ok(timing) => (Ok(()), Some(timing)),
            Err(error) => (Err(error), None),
        };
        let finalization_started_us = Instant::now().as_micros();
        let transaction = self
            .finalize_display_transaction(waveform_result, leave_on)
            .await;
        let finalization_us = Instant::now()
            .as_micros()
            .saturating_sub(finalization_started_us);

        let previous_copy_started_us = Instant::now().as_micros();
        if transaction.is_ok() {
            if let Some(previous) = self.framebuffer_bw_previous.as_deref_mut() {
                previous.copy_from_slice(self.framebuffer_bw);
                self.partial_ready = true;
            }
        } else {
            self.partial_ready = false;
        }
        let previous_copy_us = Instant::now()
            .as_micros()
            .saturating_sub(previous_copy_started_us);

        transaction.map(|()| {
            let mut timing = timing.expect("successful timed waveform must provide timings");
            timing.power_on_us = power_on_us;
            timing.finalization_us = finalization_us;
            timing.previous_copy_us = previous_copy_us;
            timing.total_us = Instant::now().as_micros().saturating_sub(total_started_us);
            timing
        })
    }

    /// Runs the established full-refresh transaction while briefly reopening a
    /// caller-owned client during the GPIO-only waveform body.
    pub async fn display_bw_cooperative_async<Open, OpenFuture, Close, CloseFuture>(
        &mut self,
        leave_on: bool,
        mut open_waveform_window: Open,
        mut close_waveform_window: Close,
    ) -> Result<(), I2C::Error>
    where
        Open: FnMut() -> OpenFuture,
        OpenFuture: Future<Output = ()>,
        Close: FnMut() -> CloseFuture,
        CloseFuture: Future<Output = bool>,
    {
        if let Err(error) = self.eink_on_async().await {
            self.partial_ready = false;
            return Err(error);
        }

        open_waveform_window().await;
        let waveform = self.display_bw_waveform_async().await;
        let window_closed = close_waveform_window().await;
        let transaction = self
            .finalize_cooperative_display_transaction(waveform, leave_on, window_closed)
            .await;

        if transaction.is_ok() {
            if let Some(previous) = self.framebuffer_bw_previous.as_deref_mut() {
                previous.copy_from_slice(self.framebuffer_bw);
                self.partial_ready = true;
            }
        } else {
            self.partial_ready = false;
        }

        transaction
    }

    pub async fn display_bw_reported_async(
        &mut self,
        leave_on: bool,
    ) -> PanelRefreshResult<I2C::Error> {
        self.run_full_refresh_reported(leave_on, None).await
    }

    pub(super) async fn run_full_refresh_reported(
        &mut self,
        leave_on: bool,
        fallback: Option<PanelRefreshFallback>,
    ) -> PanelRefreshResult<I2C::Error> {
        if let Err(source) = self.eink_on_async().await {
            self.partial_ready = false;
            return Err(PanelRefreshError::new(
                PanelRefreshErrorStage::PowerOn,
                source,
            ));
        }

        let waveform = self.display_bw_waveform_async().await;
        let transaction = self
            .finalize_display_transaction_reported(waveform, leave_on)
            .await;
        self.finish_binary_refresh(transaction.is_ok());
        transaction.map(|()| match fallback {
            Some(reason) => PanelRefreshOutcome::FullFallback { reason },
            None => PanelRefreshOutcome::Full,
        })
    }

    async fn display_bw_waveform_async(&mut self) -> Result<(), I2C::Error> {
        self.display_bw_waveform_inner_async::<false>()
            .await
            .map(|_| ())
    }

    async fn display_bw_waveform_timed_async(&mut self) -> Result<FullRefreshTiming, I2C::Error> {
        self.display_bw_waveform_inner_async::<true>().await
    }

    async fn display_bw_waveform_inner_async<const PROFILE: bool>(
        &mut self,
    ) -> Result<FullRefreshTiming, I2C::Error> {
        let mut timing = FullRefreshTiming::default();
        self.bw_initial_clean_phase::<PROFILE>(&mut timing).await?;
        self.bw_framebuffer_phase::<PROFILE>(&mut timing).await?;
        self.bw_settle_phase::<PROFILE>(&mut timing).await?;
        self.bw_final_clean_phase::<PROFILE>(&mut timing).await?;
        self.bw_terminal_vscan_phase::<PROFILE>(&mut timing).await?;
        Ok(timing)
    }

    /// Displays a panel-native 4-bit packed grayscale framebuffer using the
    /// Inkplate 4 TEMPERA's native eight-level (3-bit) waveform.
    pub async fn display_gray4_async(
        &mut self,
        framebuffer: &[u8],
        leave_on: bool,
    ) -> Result<(), I2C::Error> {
        let framebuffer: &[u8; GRAYSCALE_FRAMEBUFFER_BYTES] = framebuffer
            .try_into()
            .expect("Gray4 framebuffer must match the panel dimensions");
        if let Err(error) = self.eink_on_async().await {
            self.partial_ready = false;
            return Err(error);
        }

        let waveform = self.display_gray4_waveform_async(framebuffer).await;
        let transaction = self.finalize_display_transaction(waveform, leave_on).await;

        // A grayscale waveform never establishes a valid source image for the
        // binary partial-transition engine. A failed transaction also leaves
        // the physical panel contents uncertain.
        self.partial_ready = false;
        transaction
    }

    /// Runs the native grayscale transaction and reports coarse phase timings.
    /// The refresh probe uses this explicit entry point so normal callers do
    /// not pay measurement overhead.
    pub async fn display_gray4_timed_async(
        &mut self,
        framebuffer: &[u8],
        leave_on: bool,
    ) -> Result<FullRefreshTiming, I2C::Error> {
        let framebuffer: &[u8; GRAYSCALE_FRAMEBUFFER_BYTES] = framebuffer
            .try_into()
            .expect("Gray4 framebuffer must match the panel dimensions");
        let total_started_us = Instant::now().as_micros();
        let power_on_started_us = Instant::now().as_micros();
        if let Err(error) = self.eink_on_async().await {
            self.partial_ready = false;
            return Err(error);
        }
        let power_on_us = Instant::now()
            .as_micros()
            .saturating_sub(power_on_started_us);

        let waveform = self
            .display_gray4_waveform_inner_async::<true>(framebuffer)
            .await;
        let (waveform_result, timing) = match waveform {
            Ok(timing) => (Ok(()), Some(timing)),
            Err(error) => (Err(error), None),
        };
        let finalization_started_us = Instant::now().as_micros();
        let transaction = self
            .finalize_display_transaction(waveform_result, leave_on)
            .await;
        let finalization_us = Instant::now()
            .as_micros()
            .saturating_sub(finalization_started_us);

        self.partial_ready = false;
        transaction.map(|()| {
            let mut timing = timing.expect("successful timed waveform must provide timings");
            timing.power_on_us = power_on_us;
            timing.finalization_us = finalization_us;
            timing.total_us = Instant::now().as_micros().saturating_sub(total_started_us);
            timing
        })
    }

    async fn display_gray4_waveform_async(
        &mut self,
        framebuffer: &[u8; GRAYSCALE_FRAMEBUFFER_BYTES],
    ) -> Result<(), I2C::Error> {
        self.display_gray4_waveform_inner_async::<false>(framebuffer)
            .await
            .map(|_| ())
    }

    async fn display_gray4_waveform_inner_async<const PROFILE: bool>(
        &mut self,
        framebuffer: &[u8; GRAYSCALE_FRAMEBUFFER_BYTES],
    ) -> Result<FullRefreshTiming, I2C::Error> {
        let mut timing = FullRefreshTiming::default();
        let phase_started_us = if PROFILE {
            Instant::now().as_micros()
        } else {
            0
        };
        self.clean_grayscale_async(0, 5).await?;
        self.clean_grayscale_async(1, 15).await?;
        self.clean_grayscale_async(0, 15).await?;
        self.clean_grayscale_async(1, 15).await?;
        self.clean_grayscale_async(0, 15).await?;
        if PROFILE {
            timing.initial_clean_us = Instant::now().as_micros().saturating_sub(phase_started_us);
        }

        let phase_started_us = if PROFILE {
            Instant::now().as_micros()
        } else {
            0
        };
        for phase in 0..8 {
            self.vscan_start().await?;
            self.scan_grayscale_framebuffer_pass(framebuffer, phase);
            self.delay.delay_us(230);
        }
        if PROFILE {
            timing.framebuffer_us = Instant::now().as_micros().saturating_sub(phase_started_us);
        }

        let phase_started_us = if PROFILE {
            Instant::now().as_micros()
        } else {
            0
        };
        self.clean_grayscale_async(3, 1).await?;
        if PROFILE {
            timing.final_clean_us = Instant::now().as_micros().saturating_sub(phase_started_us);
        }

        let phase_started_us = if PROFILE {
            Instant::now().as_micros()
        } else {
            0
        };
        self.vscan_start().await?;
        if PROFILE {
            timing.terminal_vscan_us = Instant::now().as_micros().saturating_sub(phase_started_us);
        }
        Ok(timing)
    }
}
