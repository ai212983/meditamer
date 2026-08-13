use super::super::super::{
    DelayOps, I2cOps, InkplateHal, PanelRefreshError, PanelRefreshErrorStage, PanelRefreshFallback,
    PanelRefreshOutcome, PanelRefreshResult, Result, GRAYSCALE_FRAMEBUFFER_BYTES,
};
use core::future::Future;

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
        CloseFuture: Future<Output = ()>,
    {
        if let Err(error) = self.eink_on_async().await {
            self.partial_ready = false;
            return Err(error);
        }

        open_waveform_window().await;
        let waveform = self.display_bw_waveform_async().await;
        close_waveform_window().await;
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
        self.clean_async(0, 5).await?;
        self.clean_async(1, 15).await?;
        self.clean_async(0, 15).await?;
        self.clean_async(1, 15).await?;
        self.clean_async(0, 15).await?;

        for _ in 0..10 {
            self.vscan_start().await?;
            self.scan_full_framebuffer_pass();
            self.delay.delay_us(230);
        }

        self.vscan_start().await?;
        self.scan_full_settle_pass();
        self.delay.delay_us(230);

        self.clean_async(2, 1).await?;
        self.clean_async(3, 1).await?;
        self.vscan_start().await?;
        Ok(())
    }

    /// Displays a panel-native 4-bit packed grayscale framebuffer using the
    /// Inkplate 4 TEMPERA's native eight-level (3-bit) waveform.
    pub async fn display_gray4_async(
        &mut self,
        framebuffer: &[u8],
        leave_on: bool,
    ) -> Result<(), I2C::Error> {
        debug_assert_eq!(framebuffer.len(), GRAYSCALE_FRAMEBUFFER_BYTES);
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

    async fn display_gray4_waveform_async(&mut self, framebuffer: &[u8]) -> Result<(), I2C::Error> {
        self.clean_async(0, 5).await?;
        self.clean_async(1, 15).await?;
        self.clean_async(0, 15).await?;
        self.clean_async(1, 15).await?;
        self.clean_async(0, 15).await?;

        for phase in 0..8 {
            self.vscan_start().await?;
            self.scan_grayscale_framebuffer_pass(framebuffer, phase);
            self.delay.delay_us(230);
        }

        self.clean_async(3, 1).await?;
        self.vscan_start().await?;
        Ok(())
    }
}
