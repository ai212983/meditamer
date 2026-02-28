use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{with_timeout, Duration, Timer};

use crate::firmware::{
    app_state::{AppStateDiagControl, DiagKind},
    config::{DIAG_CONTROL_EVENTS, SD_DIAG_RESULTS, SD_REQUESTS},
    telemetry,
    types::{SdCommand, SdRequest, SdResult},
};

const DIAG_POLL_MS: u64 = 300;
const DIAG_SD_TIMEOUT_MS: u64 = 8_000;
const DIAG_WIFI_TIMEOUT_MS: u64 = 15_000;
const SD_DIAG_RWVERIFY_LBA: u32 = 2_048;

const STATE_IDLE: u8 = 0;
const STATE_RUNNING: u8 = 1;
const STATE_DONE: u8 = 2;
const STATE_FAILED: u8 = 3;
const STATE_CANCELED: u8 = 4;

const STEP_IDLE: u8 = 0;
const STEP_START: u8 = 1;
const STEP_SD_PROBE: u8 = 2;
const STEP_SD_RWVERIFY: u8 = 3;
const STEP_WIFI_READY: u8 = 4;
const STEP_COMPLETE: u8 = 5;
const STEP_CANCELED: u8 = 6;

const CODE_OK: u8 = 0;
const CODE_INVALID_TARGETS: u8 = 1;
const CODE_UNSUPPORTED_TARGETS: u8 = 2;
const CODE_SD_TIMEOUT: u8 = 10;
const CODE_SD_PROBE_FAILED: u8 = 11;
const CODE_SD_RWVERIFY_FAILED: u8 = 12;
const CODE_WIFI_DISABLED: u8 = 20;
const CODE_WIFI_NOT_READY: u8 = 21;
const CODE_CANCELED: u8 = 30;

const TARGET_SD: u8 = 1 << 0;
const TARGET_WIFI: u8 = 1 << 1;
const TARGET_DISPLAY: u8 = 1 << 2;
const TARGET_TOUCH: u8 = 1 << 3;
const TARGET_IMU: u8 = 1 << 4;

static DIAG_STATUS: AtomicU32 = AtomicU32::new(0);
static NEXT_SD_DIAG_REQUEST_ID: AtomicU32 = AtomicU32::new(0xD100_0000);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiagRunState {
    Idle,
    Running,
    Done,
    Failed,
    Canceled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiagRunStep {
    Idle,
    Start,
    SdProbe,
    SdRwVerify,
    WifiReady,
    Complete,
    Canceled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DiagRuntimeStatus {
    pub(crate) state: DiagRunState,
    pub(crate) step: DiagRunStep,
    pub(crate) code: u8,
    pub(crate) targets: u8,
}

impl DiagRuntimeStatus {
    pub(crate) const fn state_label(self) -> &'static str {
        match self.state {
            DiagRunState::Idle => "idle",
            DiagRunState::Running => "running",
            DiagRunState::Done => "done",
            DiagRunState::Failed => "failed",
            DiagRunState::Canceled => "canceled",
        }
    }

    pub(crate) const fn step_label(self) -> &'static str {
        match self.step {
            DiagRunStep::Idle => "idle",
            DiagRunStep::Start => "start",
            DiagRunStep::SdProbe => "sd_probe",
            DiagRunStep::SdRwVerify => "sd_rwverify",
            DiagRunStep::WifiReady => "wifi_ready",
            DiagRunStep::Complete => "complete",
            DiagRunStep::Canceled => "canceled",
        }
    }
}

enum SessionOutcome {
    Done(u8),
    Failed(u8),
    Stopped,
    Restart { kind: DiagKind, targets: u8 },
}

enum SessionInterrupt {
    Stopped,
    Restart { kind: DiagKind, targets: u8 },
}

enum SdWaitOutcome {
    Result(SdResult),
    Timeout,
    Interrupted(SessionInterrupt),
}

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

pub(crate) fn read_diag_runtime_status() -> DiagRuntimeStatus {
    let raw = DIAG_STATUS.load(Ordering::Relaxed);
    let state = match (raw & 0xFF) as u8 {
        STATE_RUNNING => DiagRunState::Running,
        STATE_DONE => DiagRunState::Done,
        STATE_FAILED => DiagRunState::Failed,
        STATE_CANCELED => DiagRunState::Canceled,
        _ => DiagRunState::Idle,
    };
    let step = match ((raw >> 8) & 0xFF) as u8 {
        STEP_START => DiagRunStep::Start,
        STEP_SD_PROBE => DiagRunStep::SdProbe,
        STEP_SD_RWVERIFY => DiagRunStep::SdRwVerify,
        STEP_WIFI_READY => DiagRunStep::WifiReady,
        STEP_COMPLETE => DiagRunStep::Complete,
        STEP_CANCELED => DiagRunStep::Canceled,
        _ => DiagRunStep::Idle,
    };
    let code = ((raw >> 16) & 0xFF) as u8;
    let targets = ((raw >> 24) & 0xFF) as u8;
    DiagRuntimeStatus {
        state,
        step,
        code,
        targets,
    }
}

fn set_status(state: u8, step: u8, code: u8, targets: u8) {
    let packed =
        (state as u32) | ((step as u32) << 8) | ((code as u32) << 16) | ((targets as u32) << 24);
    DIAG_STATUS.store(packed, Ordering::Relaxed);
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
    if let Some(interrupt) = poll_session_interrupt(kind, targets) {
        return session_outcome_from_interrupt(interrupt);
    }
    if targets == 0 {
        return SessionOutcome::Failed(CODE_INVALID_TARGETS);
    }
    if (targets & (TARGET_DISPLAY | TARGET_TOUCH | TARGET_IMU)) != 0 {
        return SessionOutcome::Failed(CODE_UNSUPPORTED_TARGETS);
    }

    set_status(STATE_RUNNING, STEP_START, CODE_OK, targets);

    if (targets & TARGET_SD) != 0 {
        if let Some(interrupt) = poll_session_interrupt(kind, targets) {
            return session_outcome_from_interrupt(interrupt);
        }
        set_status(STATE_RUNNING, STEP_SD_PROBE, CODE_OK, targets);
        let probe = match send_sd_and_wait(SdCommand::Probe, kind, targets).await {
            SdWaitOutcome::Result(result) => result,
            SdWaitOutcome::Timeout => return SessionOutcome::Failed(CODE_SD_TIMEOUT),
            SdWaitOutcome::Interrupted(interrupt) => {
                return session_outcome_from_interrupt(interrupt);
            }
        };
        if !probe.ok {
            return SessionOutcome::Failed(CODE_SD_PROBE_FAILED);
        }

        if let Some(interrupt) = poll_session_interrupt(kind, targets) {
            return session_outcome_from_interrupt(interrupt);
        }
        set_status(STATE_RUNNING, STEP_SD_RWVERIFY, CODE_OK, targets);
        let verify = match send_sd_and_wait(
            SdCommand::RwVerify {
                lba: SD_DIAG_RWVERIFY_LBA,
            },
            kind,
            targets,
        )
        .await
        {
            SdWaitOutcome::Result(result) => result,
            SdWaitOutcome::Timeout => return SessionOutcome::Failed(CODE_SD_TIMEOUT),
            SdWaitOutcome::Interrupted(interrupt) => {
                return session_outcome_from_interrupt(interrupt);
            }
        };
        if !verify.ok {
            return SessionOutcome::Failed(CODE_SD_RWVERIFY_FAILED);
        }
    }

    if (targets & TARGET_WIFI) != 0 {
        if let Some(interrupt) = poll_session_interrupt(kind, targets) {
            return session_outcome_from_interrupt(interrupt);
        }
        set_status(STATE_RUNNING, STEP_WIFI_READY, CODE_OK, targets);
        let snapshot = crate::firmware::app_state::read_app_state_snapshot();
        if !snapshot.services.upload_enabled {
            return SessionOutcome::Failed(CODE_WIFI_DISABLED);
        }

        let mut elapsed_ms = 0u64;
        while elapsed_ms < DIAG_WIFI_TIMEOUT_MS {
            if let Some(interrupt) = poll_session_interrupt(kind, targets) {
                return session_outcome_from_interrupt(interrupt);
            }
            if telemetry::snapshot().wifi_link_connected {
                break;
            }
            let wait_ms = (DIAG_WIFI_TIMEOUT_MS - elapsed_ms).min(DIAG_POLL_MS);
            Timer::after(Duration::from_millis(wait_ms)).await;
            elapsed_ms = elapsed_ms.saturating_add(wait_ms);
        }
        if !telemetry::snapshot().wifi_link_connected {
            return SessionOutcome::Failed(CODE_WIFI_NOT_READY);
        }
    }

    SessionOutcome::Done(CODE_OK)
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

fn poll_session_interrupt(active_kind: DiagKind, active_targets: u8) -> Option<SessionInterrupt> {
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

fn session_outcome_from_interrupt(interrupt: SessionInterrupt) -> SessionOutcome {
    match interrupt {
        SessionInterrupt::Stopped => SessionOutcome::Stopped,
        SessionInterrupt::Restart { kind, targets } => SessionOutcome::Restart { kind, targets },
    }
}
