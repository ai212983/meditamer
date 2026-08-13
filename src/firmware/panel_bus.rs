use super::{imu, touch::tasks};

const KEEP_TOUCH_SUSPENDED_DURING_WAVEFORM: bool =
    option_env!("MEDITAMER_TOUCH_CLOSED_DURING_PANEL_WAVEFORM").is_some();

pub(crate) const fn touch_waveform_window_mode() -> &'static str {
    if KEEP_TOUCH_SUSPENDED_DURING_WAVEFORM {
        "closed"
    } else {
        "open"
    }
}

/// Establishes an acknowledged quiet window for every independent client of
/// the I2C bus shared with the e-paper PMIC and SPV expander pin.
pub(crate) async fn suspend_clients() {
    imu::suspend_imu_acquisition().await;
    tasks::suspend_touch_acquisition().await;
    // Acquisition can already have queued a contact frame. Pause the
    // higher-priority pipeline as well so no touch work runs between the
    // timing-sensitive full-frame scan passes.
    tasks::suspend_touch_pipeline().await;
}

/// Restore the pipeline before acquisition so any pre-suspend frame is handled
/// before a lift latched during the panel transaction is sampled.
pub(crate) async fn resume_clients(reset_touch_pipeline: bool) {
    tasks::resume_touch_pipeline().await;
    tasks::resume_touch_acquisition(reset_touch_pipeline).await;
    imu::resume_imu_acquisition().await;
}

/// Release a long-running non-panel transaction without making its command
/// response wait for the first post-resume touch-controller sample.
pub(crate) fn try_request_clients_resume(reset_touch_pipeline: bool) -> bool {
    let pipeline = tasks::try_request_touch_pipeline_resume();
    let acquisition = tasks::try_request_touch_acquisition_resume(reset_touch_pipeline);
    let imu = imu::try_request_imu_acquisition_resume();
    pipeline && acquisition && imu
}

/// Reopen touch processing only while the panel runs its GPIO waveform. The
/// shared-I2C mutex serializes inter-frame touch reads with vscan setup.
pub(crate) async fn open_touch_waveform_window() {
    if KEEP_TOUCH_SUSPENDED_DURING_WAVEFORM {
        return;
    }
    tasks::resume_touch_pipeline().await;
    tasks::request_touch_acquisition_resume(false).await;
}

/// Close touch before panel finalization accesses the shared PMIC/expander bus.
pub(crate) async fn close_touch_waveform_window() {
    if KEEP_TOUCH_SUSPENDED_DURING_WAVEFORM {
        return;
    }
    tasks::suspend_touch_acquisition().await;
    tasks::suspend_touch_pipeline().await;
}
