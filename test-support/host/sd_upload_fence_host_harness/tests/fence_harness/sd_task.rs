//! SD task mirror: shared constants, power-rail stub, FAT engine stub.

use super::super::types::SdPowerRequest;
use std::sync::{Mutex, MutexGuard};

// Mirrors `SD_UPLOAD_PATH_BUF_MAX`/`SD_UPLOAD_ROOT` in the product `sd_task`
// module (buffer sizing only; abort behavior under test does not depend on
// the values).
pub(crate) const SD_UPLOAD_PATH_BUF_MAX: usize = 72;
pub(crate) const SD_UPLOAD_ROOT: &str = "/assets";

static SERIAL: Mutex<()> = Mutex::new(());
static POWER_ON_OK: Mutex<bool> = Mutex::new(true);
static POWER_LOG: Mutex<Vec<SdPowerRequest>> = Mutex::new(Vec::new());

pub(crate) fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|err| err.into_inner())
}

/// Power-rail stand-in: records every request so tests can prove the card
/// was (or was not) touched.
pub(crate) async fn request_sd_power(action: SdPowerRequest) -> bool {
    POWER_LOG
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .push(action);
    match action {
        SdPowerRequest::On => *POWER_ON_OK.lock().unwrap_or_else(|err| err.into_inner()),
        SdPowerRequest::Off => true,
    }
}

pub(crate) fn reset_power_stub() {
    *POWER_ON_OK.lock().unwrap_or_else(|err| err.into_inner()) = true;
    POWER_LOG
        .lock()
        .unwrap_or_else(|err| err.into_inner())
        .clear();
}

pub(crate) fn fail_power_on() {
    *POWER_ON_OK.lock().unwrap_or_else(|err| err.into_inner()) = false;
}

pub(crate) fn take_power_requests() -> Vec<SdPowerRequest> {
    core::mem::take(&mut *POWER_LOG.lock().unwrap_or_else(|err| err.into_inner()))
}

/// FAT transport stand-in: records cleanup traffic and replays a scripted
/// outcome, standing in for the SD card.
pub mod engine_driver {
    use super::super::super::types::SdProbeDriver;
    use sdcard::fat::{FatEngine, FatEngineError, FatRequest, FatResult, SdFatError};
    use std::sync::Mutex;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(crate) enum EngineScript {
        Done,
        NotFound,
        Failed,
    }

    #[derive(Debug, PartialEq, Eq)]
    pub(crate) enum EngineCallKind {
        Remove,
        Clear,
    }

    #[derive(Debug)]
    pub(crate) struct EngineCall {
        pub kind: EngineCallKind,
        pub path: Vec<u8>,
    }

    static SCRIPT: Mutex<EngineScript> = Mutex::new(EngineScript::Done);
    static CALLS: Mutex<Vec<EngineCall>> = Mutex::new(Vec::new());

    pub(crate) async fn run_fat_request(
        request: FatRequest,
        _probe: &mut SdProbeDriver,
        _engine: &mut FatEngine,
        _input: &[u8],
        _output: &mut [u8],
    ) -> FatResult {
        match request {
            FatRequest::Remove { path, path_len } => {
                CALLS
                    .lock()
                    .unwrap_or_else(|err| err.into_inner())
                    .push(EngineCall {
                        kind: EngineCallKind::Remove,
                        path: path[..path_len as usize].to_vec(),
                    });
                match *SCRIPT.lock().unwrap_or_else(|err| err.into_inner()) {
                    EngineScript::Done => FatResult::Done,
                    EngineScript::NotFound => {
                        FatResult::Error(FatEngineError::Fat(SdFatError::NotFound))
                    }
                    EngineScript::Failed => {
                        FatResult::Error(FatEngineError::Fat(SdFatError::NoFatPartition))
                    }
                }
            }
            FatRequest::UploadClear => {
                CALLS
                    .lock()
                    .unwrap_or_else(|err| err.into_inner())
                    .push(EngineCall {
                        kind: EngineCallKind::Clear,
                        path: Vec::new(),
                    });
                FatResult::Done
            }
            unexpected => {
                panic!("abort fence test issued unexpected FAT request: {unexpected:?}")
            }
        }
    }

    pub(crate) fn reset_engine_stub() {
        *SCRIPT.lock().unwrap_or_else(|err| err.into_inner()) = EngineScript::Done;
        CALLS.lock().unwrap_or_else(|err| err.into_inner()).clear();
    }

    pub(crate) fn set_engine_script(script: EngineScript) {
        *SCRIPT.lock().unwrap_or_else(|err| err.into_inner()) = script;
    }

    pub(crate) fn take_engine_calls() -> Vec<EngineCall> {
        core::mem::take(&mut *CALLS.lock().unwrap_or_else(|err| err.into_inner()))
    }
}

#[path = "upload.rs"]
pub mod upload;
