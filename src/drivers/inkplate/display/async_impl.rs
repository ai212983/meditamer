mod full;
mod full_scan;
mod partial;
mod partial_scan;

use super::super::{
    panel_lifecycle::{
        display_transaction_finalization, merge_transaction_results, DisplayTransactionFinalization,
    },
    DelayOps, I2cOps, InkplateHal, PanelRefreshError, PanelRefreshErrorStage, Result,
};

impl<I2C, D> InkplateHal<I2C, D>
where
    I2C: I2cOps,
    D: DelayOps,
{
    async fn finalize_display_transaction(
        &mut self,
        operation: Result<(), I2C::Error>,
        leave_on: bool,
    ) -> Result<(), I2C::Error> {
        match display_transaction_finalization(operation.is_ok(), leave_on) {
            DisplayTransactionFinalization::ParkPoweredPanel => {
                self.park_panel_scan();
                operation
            }
            DisplayTransactionFinalization::ShutDownPanel => {
                let shutdown = self.eink_off_async().await;
                merge_transaction_results(operation, shutdown)
            }
        }
    }

    async fn finalize_display_transaction_reported(
        &mut self,
        operation: Result<(), I2C::Error>,
        leave_on: bool,
    ) -> core::result::Result<(), PanelRefreshError<I2C::Error>> {
        match display_transaction_finalization(operation.is_ok(), leave_on) {
            DisplayTransactionFinalization::ParkPoweredPanel => {
                self.park_panel_scan();
                operation.map_err(|source| {
                    PanelRefreshError::new(PanelRefreshErrorStage::Waveform, source)
                })
            }
            DisplayTransactionFinalization::ShutDownPanel => {
                let shutdown = self.eink_off_async().await;
                match operation {
                    Err(source) => Err(PanelRefreshError::new(
                        PanelRefreshErrorStage::Waveform,
                        source,
                    )),
                    Ok(()) => shutdown.map_err(|source| {
                        PanelRefreshError::new(PanelRefreshErrorStage::PowerOff, source)
                    }),
                }
            }
        }
    }

    fn finish_binary_refresh(&mut self, succeeded: bool) {
        if succeeded {
            if let Some(previous) = self.framebuffer_bw_previous.as_deref_mut() {
                previous.copy_from_slice(self.framebuffer_bw);
                self.partial_ready = true;
                return;
            }
        }
        self.partial_ready = false;
    }
}
