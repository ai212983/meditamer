use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::{Duration, Timer};

use crate::firmware::bounded_control::{Control, ControlRequest, SuspendAck};

static CONTROL: Control<CriticalSectionRawMutex> = Control::new();
const CONTROL_TIMEOUT: Duration = Duration::from_secs(2);

pub async fn suspend_imu_acquisition() -> SuspendAck {
    CONTROL.suspend(Timer::after(CONTROL_TIMEOUT)).await
}

pub async fn resume_imu_acquisition() -> bool {
    CONTROL.resume(false, Timer::after(CONTROL_TIMEOUT)).await
}

pub fn try_request_imu_acquisition_resume() -> bool {
    CONTROL.request_resume(false);
    true
}

pub(super) async fn receive_command() -> ControlRequest {
    CONTROL.receive().await
}

pub(super) async fn handle_control_command(command: ControlRequest) {
    // The acquisition task calls this after its in-flight read has completed.
    CONTROL.hold_suspended(command, true).await;
}
