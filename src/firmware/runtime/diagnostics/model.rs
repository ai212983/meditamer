use core::sync::atomic::{AtomicU32, Ordering};

use crate::firmware::{app_state::DiagKind, types::SdResult};

pub(super) const DIAG_POLL_MS: u64 = 300;
pub(super) const DIAG_SD_TIMEOUT_MS: u64 = 8_000;
pub(super) const DIAG_WIFI_TIMEOUT_MS: u64 = 15_000;
pub(super) const SD_DIAG_RWVERIFY_LBA: u32 = 2_048;

pub(super) const STATE_IDLE: u8 = 0;
pub(super) const STATE_RUNNING: u8 = 1;
pub(super) const STATE_DONE: u8 = 2;
pub(super) const STATE_FAILED: u8 = 3;
pub(super) const STATE_CANCELED: u8 = 4;

pub(super) const STEP_IDLE: u8 = 0;
pub(super) const STEP_START: u8 = 1;
pub(super) const STEP_SD_PROBE: u8 = 2;
pub(super) const STEP_SD_RWVERIFY: u8 = 3;
pub(super) const STEP_WIFI_READY: u8 = 4;
pub(super) const STEP_COMPLETE: u8 = 5;
pub(super) const STEP_CANCELED: u8 = 6;

pub(super) const CODE_OK: u8 = 0;
pub(super) const CODE_INVALID_TARGETS: u8 = 1;
pub(super) const CODE_UNSUPPORTED_TARGETS: u8 = 2;
pub(super) const CODE_SD_TIMEOUT: u8 = 10;
pub(super) const CODE_SD_PROBE_FAILED: u8 = 11;
pub(super) const CODE_SD_RWVERIFY_FAILED: u8 = 12;
pub(super) const CODE_WIFI_DISABLED: u8 = 20;
pub(super) const CODE_WIFI_NOT_READY: u8 = 21;
pub(super) const CODE_CANCELED: u8 = 30;

pub(super) const TARGET_SD: u8 = 1 << 0;
pub(super) const TARGET_WIFI: u8 = 1 << 1;
pub(super) const TARGET_DISPLAY: u8 = 1 << 2;
pub(super) const TARGET_TOUCH: u8 = 1 << 3;
pub(super) const TARGET_IMU: u8 = 1 << 4;

static DIAG_STATUS: AtomicU32 = AtomicU32::new(0);
pub(super) static NEXT_SD_DIAG_REQUEST_ID: AtomicU32 = AtomicU32::new(0xD100_0000);

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

pub(super) enum SessionOutcome {
    Done(u8),
    Failed(u8),
    Stopped,
    Restart { kind: DiagKind, targets: u8 },
}

pub(super) enum SessionInterrupt {
    Stopped,
    Restart { kind: DiagKind, targets: u8 },
}

pub(super) enum SdWaitOutcome {
    Result(SdResult),
    Timeout,
    Interrupted(SessionInterrupt),
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

pub(super) fn set_status(state: u8, step: u8, code: u8, targets: u8) {
    let packed =
        (state as u32) | ((step as u32) << 8) | ((code as u32) << 16) | ((targets as u32) << 24);
    DIAG_STATUS.store(packed, Ordering::Relaxed);
}
