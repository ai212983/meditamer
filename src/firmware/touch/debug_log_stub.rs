use crate::firmware::types::{
    SerialUart, TouchEvent, TouchTraceSample, TouchWizardSwipeTraceSample,
};

#[derive(Default)]
pub(crate) struct TouchWizardSessionLog;

impl TouchWizardSessionLog {
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) fn on_session_event(
        &mut self,
        _event: crate::firmware::types::TouchWizardSessionEvent,
    ) {
    }

    pub(crate) fn on_swipe_sample(&mut self, _sample: TouchWizardSwipeTraceSample) {}

    pub(crate) fn on_touch_event(&mut self, _event: TouchEvent) {}

    pub(crate) fn on_touch_sample(&mut self, _sample: TouchTraceSample) {}

    pub(crate) fn settle_pending_end(&mut self) -> bool {
        false
    }

    pub(crate) async fn write_dump(&mut self, _uart: &mut SerialUart) {}
}

pub(crate) async fn uart_write_all(
    uart: &mut SerialUart,
    bytes: &[u8],
) -> Result<(), embedded_io_async::ErrorKind> {
    const TX_POLL_SLICE_BYTES: usize = 32;

    let mut written = 0;
    while written < bytes.len() {
        let end = (written + TX_POLL_SLICE_BYTES).min(bytes.len());
        let count = uart
            .write_async(&bytes[written..end])
            .await
            .map_err(|_| embedded_io_async::ErrorKind::Other)?;
        if count == 0 {
            return Err(embedded_io_async::ErrorKind::WriteZero);
        }
        written += count;
        embassy_futures::yield_now().await;
    }
    Ok(())
}

pub(crate) async fn write_touch_wizard_swipe_trace_sample(
    _uart: &mut SerialUart,
    _sample: TouchWizardSwipeTraceSample,
) {
}

pub(crate) async fn write_touch_event_trace_sample(_uart: &mut SerialUart, _event: TouchEvent) {}

pub(crate) async fn write_touch_trace_sample(_uart: &mut SerialUart, _sample: TouchTraceSample) {}
