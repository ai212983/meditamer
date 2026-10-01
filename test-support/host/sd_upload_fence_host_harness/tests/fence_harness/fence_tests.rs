//! Regression tests: route decision plus real fence behavior.
//!
//! Every test drives production code (the fence router, the command
//! splitter, the Abort handler) with real product types; the fakes observe
//! the hardware boundary only.

use super::super::super::super::types::{
    FakeProbe, SdPowerRequest, SdUploadCommand, SdUploadResult, SdUploadResultCode, SD_PATH_MAX,
};
use super::super::engine_driver::{
    reset_engine_stub, set_engine_script, take_engine_calls, EngineCallKind, EngineScript,
};
use super::super::{fail_power_on, reset_power_stub, serial, take_power_requests};
use super::stream::{drive_abort, open_upload_session, session_accept_chunk};
use super::{
    no_session_abort_fence_result, route_upload_request, split_upload_command, SdUploadSession,
    UploadCommandGroup, UploadRequestRoute, UploadStreamCommand,
};
use sdcard::fat::FatEngine;

fn block_on<F: core::future::Future>(future: F) -> F::Output {
    use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    fn noop(_: *const ()) {}
    fn clone_waker(_: *const ()) -> RawWaker {
        RawWaker::new(core::ptr::null(), &VTABLE)
    }
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone_waker, noop, noop, noop);
    let waker = unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VTABLE)) };
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => core::hint::spin_loop(),
        }
    }
}

fn reset_all() {
    reset_power_stub();
    reset_engine_stub();
}

const FINAL_PATH: &[u8] = b"/assets/clock.bin";
const TEMP_PATH: &[u8] = b"/assets/HCTLUPLD.TMP";

fn abort_drive(
    session: &mut Option<SdUploadSession>,
    probe: &mut FakeProbe,
    powered: &mut bool,
    mounted: &mut bool,
    engine: &mut FatEngine,
) -> SdUploadResult {
    block_on(drive_abort(session, probe, powered, mounted, engine))
}

#[test]
fn abort_without_session_succeeds_without_storage_contact() {
    let _serial = serial();
    reset_all();
    // Uninitialized probe: any init attempt would be observable, but none
    // may happen.
    let mut probe = FakeProbe::new(false);
    let mut powered = false;
    let mut mounted = false;
    let mut engine = FatEngine::new();
    let mut session = None;
    let result = abort_drive(
        &mut session,
        &mut probe,
        &mut powered,
        &mut mounted,
        &mut engine,
    );
    assert!(result.ok, "no-session fence must succeed");
    assert_eq!(result.code, SdUploadResultCode::Ok);
    assert!(session.is_none());
    assert!(
        !powered && !mounted,
        "card must stay unpowered and unmounted"
    );
    assert!(take_power_requests().is_empty(), "no power rail contact");
    assert!(take_engine_calls().is_empty(), "no FAT traffic");
}

#[test]
fn abort_routes_to_fence_while_ordinary_commands_stay_gated() {
    let _serial = serial();
    assert!(matches!(
        route_upload_request(&SdUploadCommand::Abort, false),
        UploadRequestRoute::AbortFence {
            session_active: false
        }
    ));
    assert!(matches!(
        route_upload_request(&SdUploadCommand::Abort, true),
        UploadRequestRoute::AbortFence {
            session_active: true
        }
    ));
    for session_active in [false, true] {
        assert!(matches!(
            route_upload_request(
                &SdUploadCommand::Begin {
                    path: [0u8; SD_PATH_MAX],
                    path_len: 0,
                    expected_size: 0,
                },
                session_active
            ),
            UploadRequestRoute::GatedTransfer
        ));
        assert!(matches!(
            route_upload_request(&SdUploadCommand::Chunk { data_len: 0 }, session_active),
            UploadRequestRoute::GatedTransfer
        ));
        assert!(matches!(
            route_upload_request(&SdUploadCommand::Commit, session_active),
            UploadRequestRoute::GatedTransfer
        ));
        assert!(matches!(
            route_upload_request(
                &SdUploadCommand::Mkdir {
                    path: [0u8; SD_PATH_MAX],
                    path_len: 0,
                },
                session_active
            ),
            UploadRequestRoute::GatedTransfer
        ));
        assert!(matches!(
            route_upload_request(
                &SdUploadCommand::Remove {
                    path: [0u8; SD_PATH_MAX],
                    path_len: 0,
                },
                session_active
            ),
            UploadRequestRoute::GatedTransfer
        ));
        assert!(matches!(
            route_upload_request(
                &SdUploadCommand::Stat {
                    path: [0u8; SD_PATH_MAX],
                    path_len: 0,
                },
                session_active
            ),
            UploadRequestRoute::GatedTransfer
        ));
    }
}

#[test]
fn no_session_fence_result_echoes_correlation_id() {
    let _serial = serial();
    // Includes the boot id from the correlated supervisor fence that
    // motivated this fix.
    for id in [1, 7, 3_115_839_248, u32::MAX] {
        let result = no_session_abort_fence_result(id);
        assert!(result.ok, "fence must be accepted");
        assert_eq!(result.code, SdUploadResultCode::Ok);
        assert_eq!(result.request_id, id);
        assert_eq!(result.bytes_written, 0);
    }
}

#[test]
fn active_session_abort_performs_real_cleanup() {
    let _serial = serial();
    reset_all();
    let mut session = open_upload_session(FINAL_PATH, TEMP_PATH, 1024);
    session_accept_chunk(&mut session, 256);
    let mut probe = FakeProbe::new(true);
    let mut powered = false;
    let mut mounted = false;
    let mut engine = FatEngine::new();
    let result = abort_drive(
        &mut session,
        &mut probe,
        &mut powered,
        &mut mounted,
        &mut engine,
    );
    assert!(result.ok, "cleanup must succeed");
    assert_eq!(result.code, SdUploadResultCode::Ok);
    assert_eq!(
        result.bytes_written, 256,
        "result reports the real session progress"
    );
    assert!(session.is_none(), "session is consumed by cleanup");
    assert!(powered && mounted, "cleanup powers and mounts storage");
    let power = take_power_requests();
    assert_eq!(power.len(), 1);
    assert!(matches!(power[0], SdPowerRequest::On));
    let calls = take_engine_calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].kind, EngineCallKind::Remove);
    assert_eq!(calls[0].path, TEMP_PATH);
    assert_eq!(calls[1].kind, EngineCallKind::Clear);
}

#[test]
fn active_session_abort_tolerates_already_removed_temp_file() {
    let _serial = serial();
    reset_all();
    set_engine_script(EngineScript::NotFound);
    let mut session = open_upload_session(FINAL_PATH, TEMP_PATH, 1024);
    let mut probe = FakeProbe::new(true);
    let mut powered = false;
    let mut mounted = false;
    let mut engine = FatEngine::new();
    let result = abort_drive(
        &mut session,
        &mut probe,
        &mut powered,
        &mut mounted,
        &mut engine,
    );
    assert!(result.ok, "missing temp file still acks");
    assert_eq!(result.code, SdUploadResultCode::Ok);
    assert!(session.is_none());
    assert_eq!(take_engine_calls().len(), 2);
}

#[test]
fn active_session_abort_exposes_remove_failure() {
    let _serial = serial();
    reset_all();
    set_engine_script(EngineScript::Failed);
    let mut session = open_upload_session(FINAL_PATH, TEMP_PATH, 1024);
    let mut probe = FakeProbe::new(true);
    let mut powered = false;
    let mut mounted = false;
    let mut engine = FatEngine::new();
    let result = abort_drive(
        &mut session,
        &mut probe,
        &mut powered,
        &mut mounted,
        &mut engine,
    );
    assert!(!result.ok, "cleanup failure must fail the fence");
    assert_eq!(result.code, SdUploadResultCode::OperationFailed);
    assert!(
        result.code != SdUploadResultCode::Busy && result.code != SdUploadResultCode::Ok,
        "failure is exposed, never masked as Busy or Ok"
    );
    assert!(session.is_none());
    assert_eq!(take_engine_calls().len(), 2);
}

#[test]
fn active_session_abort_exposes_power_failure() {
    let _serial = serial();
    reset_all();
    fail_power_on();
    let mut session = open_upload_session(FINAL_PATH, TEMP_PATH, 1024);
    let mut probe = FakeProbe::new(true);
    let mut powered = false;
    let mut mounted = false;
    let mut engine = FatEngine::new();
    let result = abort_drive(
        &mut session,
        &mut probe,
        &mut powered,
        &mut mounted,
        &mut engine,
    );
    assert!(!result.ok);
    assert_eq!(result.code, SdUploadResultCode::PowerOnFailed);
    assert!(
        take_engine_calls().is_empty(),
        "failed power never reaches storage"
    );
}

#[test]
fn active_session_abort_exposes_init_failure() {
    let _serial = serial();
    reset_all();
    let mut session = open_upload_session(FINAL_PATH, TEMP_PATH, 1024);
    let mut probe = FakeProbe::new(false);
    let mut powered = false;
    let mut mounted = false;
    let mut engine = FatEngine::new();
    let result = abort_drive(
        &mut session,
        &mut probe,
        &mut powered,
        &mut mounted,
        &mut engine,
    );
    assert!(!result.ok);
    assert_eq!(result.code, SdUploadResultCode::InitFailed);
    assert!(
        take_engine_calls().is_empty(),
        "failed init never reaches storage"
    );
}

#[test]
fn abort_splits_to_the_abort_handler() {
    let _serial = serial();
    assert!(matches!(
        split_upload_command(SdUploadCommand::Abort),
        UploadCommandGroup::Stream(UploadStreamCommand::Abort)
    ));
    assert!(matches!(
        split_upload_command(SdUploadCommand::Commit),
        UploadCommandGroup::Stream(UploadStreamCommand::Commit)
    ));
    assert!(matches!(
        split_upload_command(SdUploadCommand::Stat {
            path: [0u8; SD_PATH_MAX],
            path_len: 0,
        }),
        UploadCommandGroup::Path(_)
    ));
}
