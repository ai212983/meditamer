//! Correlated desired-state control; timed-out resume waits retain intent.
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_time::{Duration, Timer};

use crate::firmware::bounded_control::{Control, ControlRequest, SuspendAck};

static CONTROL: Control<CriticalSectionRawMutex> = Control::new();
const CONTROL_TIMEOUT: Duration = Duration::from_secs(2);

pub async fn suspend_environment_acquisition() -> SuspendAck {
    let Some(request) = CONTROL.request_suspend() else {
        return SuspendAck::Exhausted;
    };
    // Close transport admission before yielding, including when the caller
    // later times out. The retained control request still quiesces the task.
    super::super::config::ENVIRONMENT_REQUESTS.close();
    CONTROL
        .wait_suspended(request, Timer::after(CONTROL_TIMEOUT))
        .await
}

pub async fn resume_environment_acquisition() -> bool {
    CONTROL.resume(false, Timer::after(CONTROL_TIMEOUT)).await
}

pub fn try_request_environment_acquisition_resume() -> bool {
    CONTROL.request_resume(false);
    true
}

pub(super) async fn receive_command() -> ControlRequest {
    CONTROL.receive().await
}

pub(super) fn try_receive_command() -> Option<ControlRequest> {
    CONTROL.try_receive()
}

/// Cleanup has already finished. Repeated suspension reports its same outcome;
/// only a current Running request releases the participant's suspended loop.
pub(super) async fn handle_control_command(command: ControlRequest, cleanly: bool) {
    CONTROL.hold_suspended(command, cleanly).await;
}

pub(crate) fn control_snapshot() -> (
    crate::firmware::bounded_control::ControlRequest,
    Option<crate::firmware::bounded_control::ControlAck>,
) {
    CONTROL.snapshot()
}
