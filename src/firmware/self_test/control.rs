use super::model::{
    set_status, SessionInterrupt, SessionOutcome, CODE_CANCELED, CODE_INVALID_TARGETS, CODE_OK,
    CODE_UNSUPPORTED_TARGETS, STATE_CANCELED, STATE_DONE, STATE_FAILED, STATE_IDLE, STATE_RUNNING,
    STEP_CANCELED, STEP_COMPLETE, STEP_IDLE, STEP_START, TARGET_DISPLAY, TARGET_IMU, TARGET_SD,
    TARGET_TOUCH, TARGET_WIFI,
};
use super::sd_checks::run_sd_checks;
use super::wifi::run_wifi_check;

use crate::firmware::{
    app_state::{AppStateDiagControl, DiagKind},
    config::DIAG_CONTROL_EVENTS,
};

#[embassy_executor::task]
pub(crate) async fn diagnostics_task() {
    set_status(STATE_IDLE, STEP_IDLE, CODE_OK, 0);
    let mut queued_start: Option<(DiagKind, u8)> = None;

    loop {
        let (kind, targets) = match queued_start.take() {
            Some(request) => request,
            None => wait_for_start_request().await,
        };

        let outcome = run_session(kind, targets).await;
        match outcome {
            SessionOutcome::Done(code) => {
                set_status(STATE_DONE, STEP_COMPLETE, code, targets);
            }
            SessionOutcome::Failed(code) => {
                set_status(STATE_FAILED, STEP_COMPLETE, code, targets);
            }
            SessionOutcome::Stopped => {
                set_status(STATE_IDLE, STEP_IDLE, CODE_OK, 0);
            }
            SessionOutcome::Restart {
                kind: next_kind,
                targets: next_targets,
            } => {
                set_status(STATE_CANCELED, STEP_CANCELED, CODE_CANCELED, targets);
                queued_start = Some((next_kind, next_targets));
            }
        }
    }
}
async fn wait_for_start_request() -> (DiagKind, u8) {
    loop {
        match DIAG_CONTROL_EVENTS.receive().await {
            AppStateDiagControl::Stop => {
                set_status(STATE_IDLE, STEP_IDLE, CODE_OK, 0);
            }
            AppStateDiagControl::Start { kind, targets } => {
                return (kind, targets.as_persisted());
            }
        }
    }
}

async fn run_session(kind: DiagKind, targets: u8) -> SessionOutcome {
    if let Some(outcome) = session_interrupt_outcome(kind, targets) {
        return outcome;
    }
    if let Err(code) = validate_targets(targets) {
        return SessionOutcome::Failed(code);
    }

    set_status(STATE_RUNNING, STEP_START, CODE_OK, targets);

    if (targets & TARGET_SD) != 0 {
        if let Some(outcome) = run_sd_checks(kind, targets).await {
            return outcome;
        }
    }

    if (targets & TARGET_WIFI) != 0 {
        if let Some(outcome) = run_wifi_check(kind, targets).await {
            return outcome;
        }
    }

    SessionOutcome::Done(CODE_OK)
}

fn validate_targets(targets: u8) -> Result<(), u8> {
    if targets == 0 {
        return Err(CODE_INVALID_TARGETS);
    }
    if (targets & (TARGET_DISPLAY | TARGET_TOUCH | TARGET_IMU)) != 0 {
        return Err(CODE_UNSUPPORTED_TARGETS);
    }
    Ok(())
}

pub(super) fn session_interrupt_outcome(kind: DiagKind, targets: u8) -> Option<SessionOutcome> {
    poll_session_interrupt(kind, targets).map(session_outcome_from_interrupt)
}
pub(super) fn poll_session_interrupt(
    active_kind: DiagKind,
    active_targets: u8,
) -> Option<SessionInterrupt> {
    let mut latest = None;
    while let Ok(control) = DIAG_CONTROL_EVENTS.try_receive() {
        latest = Some(control);
    }

    match latest {
        None => None,
        Some(AppStateDiagControl::Stop) => Some(SessionInterrupt::Stopped),
        Some(AppStateDiagControl::Start { kind, targets }) => {
            let requested_targets = targets.as_persisted();
            if kind == active_kind && requested_targets == active_targets {
                None
            } else {
                Some(SessionInterrupt::Restart {
                    kind,
                    targets: requested_targets,
                })
            }
        }
    }
}

pub(super) fn session_outcome_from_interrupt(interrupt: SessionInterrupt) -> SessionOutcome {
    match interrupt {
        SessionInterrupt::Stopped => SessionOutcome::Stopped,
        SessionInterrupt::Restart { kind, targets } => SessionOutcome::Restart { kind, targets },
    }
}
