use super::super::super::{
    DelayOps, I2cOps, InkplateHal, PanelRefreshError, PanelRefreshErrorStage, PanelRefreshFallback,
    PanelRefreshOutcome, PanelRefreshResult, PartialGateDrainTiming, Result,
};
use super::super::{
    panel_partial_scan_rows, panel_partial_transition_stats, prepare_panel_partial_transition,
};
use core::future::Future;

struct WaveformWindow<Open, Close> {
    open: Open,
    close: Close,
}

impl<Open, Close> WaveformWindow<Open, Close> {
    const fn new(open: Open, close: Close) -> Self {
        Self { open, close }
    }
}

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    pub async fn display_bw_partial_async(&mut self, leave_on: bool) -> Result<(), I2C::Error> {
        if !self.partial_ready {
            return self.display_bw_async(leave_on).await;
        }

        self.display_bw_partial_forced_async(leave_on).await
    }

    /// Runs the known-good partial transaction with touch sampling available
    /// only during the GPIO waveform. The result remains the real driver result
    /// so callers can account for failure without changing waveform code.
    pub async fn display_bw_partial_cooperative_async<Open, OpenFuture, Close, CloseFuture>(
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
        if !self.partial_ready {
            return self
                .display_bw_cooperative_async(leave_on, open_waveform_window, close_waveform_window)
                .await;
        }

        let Some(transition) = self.partial_transition.as_deref_mut() else {
            return self
                .display_bw_cooperative_async(leave_on, open_waveform_window, close_waveform_window)
                .await;
        };
        let Some(previous) = self.framebuffer_bw_previous.as_deref() else {
            return self
                .display_bw_cooperative_async(leave_on, open_waveform_window, close_waveform_window)
                .await;
        };
        prepare_panel_partial_transition(previous, self.framebuffer_bw, transition);

        if let Err(error) = self.eink_on_async().await {
            self.partial_ready = false;
            return Err(error);
        }

        open_waveform_window().await;
        let waveform = self.display_bw_partial_waveform_async().await;
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

    /// Scans through the final changed gate, drains the remaining gate chain
    /// with neutral source data, and omits the reference cleanup passes. Panel
    /// power finalization remains controlled by `leave_on`.
    pub(crate) async fn display_bw_partial_gate_drain_no_cleanup_cooperative_async<
        Open,
        OpenFuture,
        Close,
        CloseFuture,
    >(
        &mut self,
        leave_on: bool,
        open_waveform_window: Open,
        close_waveform_window: Close,
    ) -> Result<PartialGateDrainTiming, I2C::Error>
    where
        Open: FnMut() -> OpenFuture,
        OpenFuture: Future<Output = ()>,
        Close: FnMut() -> CloseFuture,
        CloseFuture: Future<Output = ()>,
    {
        self.display_bw_partial_gate_drain_timed_cooperative_async(
            leave_on,
            WaveformWindow::new(open_waveform_window, close_waveform_window),
        )
        .await
    }

    async fn display_bw_partial_gate_drain_timed_cooperative_async<
        Open,
        OpenFuture,
        Close,
        CloseFuture,
    >(
        &mut self,
        leave_on: bool,
        mut waveform_window: WaveformWindow<Open, Close>,
    ) -> Result<PartialGateDrainTiming, I2C::Error>
    where
        Open: FnMut() -> OpenFuture,
        OpenFuture: Future<Output = ()>,
        Close: FnMut() -> CloseFuture,
        CloseFuture: Future<Output = ()>,
    {
        if !self.partial_ready {
            return self
                .display_bw_partial_gate_drain_fallback_async(leave_on, waveform_window)
                .await;
        }

        let Some(transition) = self.partial_transition.as_deref_mut() else {
            return self
                .display_bw_partial_gate_drain_fallback_async(leave_on, waveform_window)
                .await;
        };
        let Some(previous) = self.framebuffer_bw_previous.as_deref() else {
            return self
                .display_bw_partial_gate_drain_fallback_async(leave_on, waveform_window)
                .await;
        };
        let Some(scan_rows) = panel_partial_scan_rows(previous, self.framebuffer_bw) else {
            return Ok(PartialGateDrainTiming::no_change());
        };
        prepare_panel_partial_transition(previous, self.framebuffer_bw, transition);

        if let Err(error) = self.eink_on_async().await {
            self.partial_ready = false;
            return Err(error);
        }

        (waveform_window.open)().await;
        let waveform = self
            .display_bw_partial_gate_drain_waveform_timed_async(scan_rows)
            .await;
        (waveform_window.close)().await;
        let (waveform_result, timing) = match waveform {
            Ok(value) => (Ok(()), Some(value)),
            Err(error) => (Err(error), None),
        };
        let transaction = self
            .finalize_display_transaction(waveform_result, leave_on)
            .await;

        if transaction.is_ok() {
            if let Some(previous) = self.framebuffer_bw_previous.as_deref_mut() {
                previous.copy_from_slice(self.framebuffer_bw);
                self.partial_ready = true;
            }
        } else {
            self.partial_ready = false;
        }

        match transaction {
            Ok(()) => Ok(timing.expect("successful timed waveform must provide timings")),
            Err(error) => Err(error),
        }
    }

    async fn display_bw_partial_gate_drain_fallback_async<Open, OpenFuture, Close, CloseFuture>(
        &mut self,
        leave_on: bool,
        waveform_window: WaveformWindow<Open, Close>,
    ) -> Result<PartialGateDrainTiming, I2C::Error>
    where
        Open: FnMut() -> OpenFuture,
        OpenFuture: Future<Output = ()>,
        Close: FnMut() -> CloseFuture,
        CloseFuture: Future<Output = ()>,
    {
        let fallback = self
            .display_bw_cooperative_async(leave_on, waveform_window.open, waveform_window.close)
            .await;
        match fallback {
            Ok(()) => Ok(PartialGateDrainTiming::full_fallback()),
            Err(error) => Err(error),
        }
    }

    /// Runs a partial refresh and reports the operation that actually reached
    /// the panel, including a full-refresh fallback or a no-change skip.
    pub async fn display_bw_partial_reported_async(
        &mut self,
        leave_on: bool,
    ) -> PanelRefreshResult<I2C::Error> {
        if !self.partial_ready {
            return self
                .run_full_refresh_reported(
                    leave_on,
                    Some(PanelRefreshFallback::BaselineUnavailable),
                )
                .await;
        }

        let Some(transition) = self.partial_transition.as_deref_mut() else {
            return self
                .run_full_refresh_reported(
                    leave_on,
                    Some(PanelRefreshFallback::TransitionBufferUnavailable),
                )
                .await;
        };
        let Some(previous) = self.framebuffer_bw_previous.as_deref() else {
            return self
                .run_full_refresh_reported(
                    leave_on,
                    Some(PanelRefreshFallback::PreviousFramebufferUnavailable),
                )
                .await;
        };
        let stats = panel_partial_transition_stats(previous, self.framebuffer_bw);
        prepare_panel_partial_transition(previous, self.framebuffer_bw, transition);
        if stats.changed_pixels == 0 {
            return Ok(PanelRefreshOutcome::NoChange);
        }

        if let Err(source) = self.eink_on_async().await {
            self.partial_ready = false;
            return Err(PanelRefreshError::new(
                PanelRefreshErrorStage::PowerOn,
                source,
            ));
        }

        let waveform = self.display_bw_partial_waveform_async().await;
        let transaction = self
            .finalize_display_transaction_reported(waveform, leave_on)
            .await;
        self.finish_binary_refresh(transaction.is_ok());
        transaction.map(|()| PanelRefreshOutcome::Partial {
            changed_bytes: stats.changed_bytes,
            changed_pixels: stats.changed_pixels,
        })
    }

    /// Runs the fast partial waveform even when no full refresh established the
    /// previous framebuffer in this boot. The previous buffer starts white, so
    /// callers accept that black pixels left by an older image may persist until
    /// a later cleanup refresh.
    pub async fn display_bw_partial_forced_async(
        &mut self,
        leave_on: bool,
    ) -> Result<(), I2C::Error> {
        let Some(transition) = self.partial_transition.as_deref_mut() else {
            return self.display_bw_async(leave_on).await;
        };
        let Some(previous) = self.framebuffer_bw_previous.as_deref() else {
            return self.display_bw_async(leave_on).await;
        };
        prepare_panel_partial_transition(previous, self.framebuffer_bw, transition);

        if let Err(error) = self.eink_on_async().await {
            self.partial_ready = false;
            return Err(error);
        }

        let waveform = self.display_bw_partial_waveform_async().await;
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
}
