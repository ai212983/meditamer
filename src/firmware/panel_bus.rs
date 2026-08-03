use super::{imu, touch::tasks};

/// Establishes an acknowledged quiet window for every independent client of
/// the I2C bus shared with the e-paper PMIC and SPV expander pin.
pub(crate) async fn suspend_clients() {
    imu::suspend_imu_acquisition().await;
    tasks::suspend_touch_acquisition().await;
}

/// Restores touch first so a lift latched during the panel transaction is read
/// before background IMU sampling resumes on the shared bus.
pub(crate) async fn resume_clients(reset_touch_pipeline: bool) {
    tasks::resume_touch_acquisition(reset_touch_pipeline).await;
    imu::resume_imu_acquisition().await;
}

/// Reopens only touch sampling while the panel is running its GPIO waveform.
/// IMU stays suspended; the shared-I2C mutex serializes any inter-frame access.
pub(crate) async fn open_touch_waveform_window() {
    tasks::request_touch_acquisition_resume(false).await;
}

/// Closes the waveform window before panel shutdown uses shared I2C again.
pub(crate) async fn close_touch_waveform_window() {
    tasks::suspend_touch_acquisition().await;
}
