use super::control::{
    poll_session_interrupt, session_interrupt_outcome, session_outcome_from_interrupt,
};
use super::model::{
    set_status, SdWaitOutcome, SessionOutcome, CODE_OK, CODE_SD_PROBE_FAILED,
    CODE_SD_RWVERIFY_FAILED, CODE_SD_TIMEOUT, DIAG_POLL_MS, DIAG_SD_TIMEOUT_MS,
    NEXT_SD_DIAG_REQUEST_ID, SD_DIAG_RWVERIFY_LBA, STATE_RUNNING, STEP_SD_PROBE, STEP_SD_RWVERIFY,
};
use core::sync::atomic::Ordering;

use embassy_time::{with_timeout, Duration};

use crate::firmware::{
    app_state::DiagKind,
    config::{SD_DIAG_RESULTS, SD_REQUESTS},
    types::{SdCommand, SdRequest, SdResult},
};

pub(super) async fn run_sd_checks(kind: DiagKind, targets: u8) -> Option<SessionOutcome> {
    if let Some(outcome) = session_interrupt_outcome(kind, targets) {
        return Some(outcome);
    }

    set_status(STATE_RUNNING, STEP_SD_PROBE, CODE_OK, targets);
    let probe = match sd_wait_result_or_outcome(SdCommand::Probe, kind, targets).await {
        Ok(result) => result,
        Err(outcome) => return Some(outcome),
    };
    if !probe.ok {
        return Some(SessionOutcome::Failed(CODE_SD_PROBE_FAILED));
    }

    if let Some(outcome) = session_interrupt_outcome(kind, targets) {
        return Some(outcome);
    }

    set_status(STATE_RUNNING, STEP_SD_RWVERIFY, CODE_OK, targets);
    let verify = match sd_wait_result_or_outcome(
        SdCommand::RwVerify {
            lba: SD_DIAG_RWVERIFY_LBA,
        },
        kind,
        targets,
    )
    .await
    {
        Ok(result) => result,
        Err(outcome) => return Some(outcome),
    };
    if !verify.ok {
        return Some(SessionOutcome::Failed(CODE_SD_RWVERIFY_FAILED));
    }

    None
}

async fn sd_wait_result_or_outcome(
    command: SdCommand,
    kind: DiagKind,
    targets: u8,
) -> Result<SdResult, SessionOutcome> {
    match send_sd_and_wait(command, kind, targets).await {
        SdWaitOutcome::Result(result) => Ok(result),
        SdWaitOutcome::Timeout => Err(SessionOutcome::Failed(CODE_SD_TIMEOUT)),
        SdWaitOutcome::Interrupted(interrupt) => Err(session_outcome_from_interrupt(interrupt)),
    }
}
async fn send_sd_and_wait(command: SdCommand, kind: DiagKind, targets: u8) -> SdWaitOutcome {
    while SD_DIAG_RESULTS.try_receive().is_ok() {}

    let request_id = NEXT_SD_DIAG_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    SD_REQUESTS
        .send(SdRequest {
            id: request_id,
            command,
        })
        .await;

    let mut elapsed_ms = 0u64;
    while elapsed_ms < DIAG_SD_TIMEOUT_MS {
        if let Some(interrupt) = poll_session_interrupt(kind, targets) {
            return SdWaitOutcome::Interrupted(interrupt);
        }

        let wait_ms = (DIAG_SD_TIMEOUT_MS - elapsed_ms).min(DIAG_POLL_MS);
        match with_timeout(Duration::from_millis(wait_ms), SD_DIAG_RESULTS.receive()).await {
            Ok(result) if result.id == request_id => return SdWaitOutcome::Result(result),
            Ok(_) => {}
            Err(_) => {}
        }
        elapsed_ms = elapsed_ms.saturating_add(wait_ms);
    }
    SdWaitOutcome::Timeout
}
