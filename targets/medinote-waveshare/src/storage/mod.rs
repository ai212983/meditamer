//! Sole owner of the native card and shared FAT engine.
mod bus;
#[cfg(feature = "wifi-storage")]
pub mod http;
#[cfg(feature = "wifi-storage")]
pub use http::{
    abort_upload_barrier, active_roundtrips, init_upload_buffer, upload_session_active,
};
mod transport;

use aligned::{Aligned, A4};
use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use embassy_futures::select::{select, Either};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, signal::Signal,
};
use embassy_time::{with_timeout, Duration, Instant};
use esp_hal::{sdmmc::Slot, Async};
use sdcard::fat::{FatEngine, FatResult};
use sdcard::request::StorageCommand;
use sdcard::service::{FatBuffers, FatObserver};
#[cfg(feature = "wifi-storage")]
use sdcard::upload::{Executor, Session, TEMP_BASENAME};
use static_cell::StaticCell;
use transport::NativeTransport;

struct Request {
    id: u32,
    command: RequestCommand,
    deadline: Instant,
}
#[derive(Clone, Copy)]
enum RequestCommand {
    Fat(StorageCommand),
    #[cfg(feature = "wifi-storage")]
    Upload(UploadCommand),
    #[cfg(feature = "wifi-storage")]
    HttpFat(StorageCommand),
}

#[derive(Clone, Copy)]
#[cfg(feature = "wifi-storage")]
pub enum UploadCommand {
    Begin {
        path: [u8; sdcard::SD_PATH_MAX],
        path_len: u8,
        expected_size: u32,
    },
    Chunk {
        data_len: u16,
    },
    Commit,
    Abort,
}
static REQUESTS: Channel<CriticalSectionRawMutex, Request, 2> = Channel::new();
#[cfg(feature = "wifi-storage")]
static UPLOAD_RESULTS: Channel<CriticalSectionRawMutex, UploadResponse, 1> = Channel::new();
static WAKE: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static SUSPENDED: Signal<CriticalSectionRawMutex, u32> = Signal::new();
static SD_SLOT: StaticCell<RefCell<Slot<'static, 1, Async>>> = StaticCell::new();
static ADMISSION: AtomicBool = AtomicBool::new(true);
static EPOCH: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "wifi-storage")]
static SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);
static NEXT_ID: AtomicU32 = AtomicU32::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UploadStatus {
    Done,
    Busy,
    InvalidPath,
    NotFound,
    NotEmpty,
    DirectoryFull,
    #[cfg(feature = "wifi-storage")]
    SessionNotActive,
    SizeMismatch,
    Cancelled,
    TimedOut,
    Unavailable,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(feature = "wifi-storage")]
pub struct UploadResponse {
    pub id: u32,
    pub status: UploadStatus,
    pub bytes_written: u32,
}

pub fn try_submit(command: StorageCommand) -> Result<(), ()> {
    // Admission and enqueue run synchronously on the same executor as sleep.
    if !ADMISSION.load(Ordering::Acquire) {
        return Err(());
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    REQUESTS
        .try_send(Request {
            id,
            command: RequestCommand::Fat(command),
            deadline: Instant::now() + Duration::from_secs(10),
        })
        .map_err(|_| ())?;
    console::println!("SDREQ id={} op={:?}", id, command.kind());
    Ok(())
}

#[cfg(feature = "wifi-storage")]
fn reply_upload(id: u32, status: UploadStatus, bytes_written: u32) {
    let _ = UPLOAD_RESULTS.try_send(UploadResponse {
        id,
        status,
        bytes_written,
    });
}

/// Wait for an acknowledgement of this specific pause, never a stale one.
pub async fn suspend() -> bool {
    ADMISSION.store(false, Ordering::Release);
    let epoch = EPOCH.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
    WAKE.signal(());
    let wait = async {
        loop {
            if SUSPENDED.wait().await == epoch {
                break;
            }
        }
    };
    with_timeout(Duration::from_secs(8), wait).await.is_ok()
}

pub fn resume() {
    ADMISSION.store(true, Ordering::Release);
    WAKE.signal(());
}

struct Observer {
    id: u32,
}
impl FatObserver for Observer {
    fn should_cancel(&self) -> bool {
        !ADMISSION.load(Ordering::Acquire)
    }
    fn on_stream_chunk(&mut self, offset: u32, bytes: &[u8]) {
        use core::fmt::Write;
        // Bound each serial record independently of the FAT sector size. The
        // offset permits exact reconstruction and detects missing records.
        for (index, chunk) in bytes.chunks(128).enumerate() {
            let mut hex = heapless::String::<256>::new();
            for byte in chunk {
                let _ = write!(hex, "{:02x}", byte);
            }
            console::println!(
                "SDFAT_DATA id={} offset={} hex={}",
                self.id,
                offset + (index * 128) as u32,
                hex
            );
        }
    }
    fn on_list_entry(&mut self, entry: &sdcard::fat::FatDirEntry) {
        let name = core::str::from_utf8(&entry.name[..usize::from(entry.name_len)]).unwrap_or("?");
        console::println!(
            "SDFAT_ENTRY id={} name={} size={} dir={}",
            self.id,
            name,
            entry.size,
            entry.is_dir
        );
    }
}

#[embassy_executor::task]
pub async fn run(
    slot: Slot<'static, 1, Async>,
    engine: &'static mut FatEngine,
    workspace: &'static mut Aligned<A4, [u8; 512]>,
) {
    let slot = SD_SLOT.init(RefCell::new(slot));
    let mut transport = NativeTransport::new(slot, workspace);
    let mut initialized = false;
    #[cfg(feature = "wifi-storage")]
    let mut upload_session: Option<Session<72>> = None;
    let mut output = [0u8; 256];
    console::println!(
        "STORAGE_READY transport=sdmmc width=1 clock_hz=20000000 lazy=true engine_bytes={}",
        core::mem::size_of::<FatEngine>()
    );
    loop {
        if !ADMISSION.load(Ordering::Acquire) {
            #[cfg(feature = "wifi-storage")]
            {
                upload_session = None;
            }
            #[cfg(feature = "wifi-storage")]
            SESSION_ACTIVE.store(false, Ordering::Release);
            engine.invalidate();
            transport.quiesce_for_suspend();
            initialized = false;
            while let Ok(request) = REQUESTS.try_receive() {
                reply_request(request.id, request.command, UploadStatus::Cancelled, 0);
                console::println!("SDRESULT id={} status=cancelled", request.id);
            }
            let epoch = EPOCH.load(Ordering::Acquire);
            SUSPENDED.signal(epoch);
            console::println!("STORAGE_SUSPENDED epoch={}", epoch);
            // Inspect desired state again after every wake; an expired pause
            // followed by resume cannot strand the owner on an old signal.
            while !ADMISSION.load(Ordering::Acquire) {
                WAKE.wait().await;
            }
            continue;
        }
        #[cfg(feature = "wifi-storage")]
        if upload_session
            .as_ref()
            .is_some_and(|session| session.last_activity_at().elapsed() >= Duration::from_secs(60))
        {
            if let Some(expired) = upload_session.take() {
                let mut observer = Observer { id: 0 };
                let (remove, clear) = Executor::new(&mut transport, engine, &mut observer)
                    .with_deadline(Instant::now() + Duration::from_secs(10))
                    .abort(
                        &expired,
                        &mut FatBuffers {
                            input: &[],
                            output: &mut output,
                        },
                    )
                    .await;
                if matches!(remove, FatResult::Error(ref e) if e.is_transport_failure())
                    || matches!(clear, FatResult::Error(ref e) if e.is_transport_failure())
                {
                    initialized = false;
                    engine.invalidate();
                }
                SESSION_ACTIVE.store(false, Ordering::Release);
            }
        }
        let request = match select(
            with_timeout(Duration::from_secs(1), WAKE.wait()),
            REQUESTS.receive(),
        )
        .await
        {
            Either::First(_) => continue,
            Either::Second(request) => request,
        };
        if !ADMISSION.load(Ordering::Acquire) {
            reply_request(request.id, request.command, UploadStatus::Cancelled, 0);
            console::println!("SDRESULT id={} status=cancelled", request.id);
            continue;
        }
        if Instant::now() >= request.deadline {
            reply_request(request.id, request.command, UploadStatus::TimedOut, 0);
            console::println!("SDRESULT id={} status=timeout", request.id);
            continue;
        }
        if !initialized {
            if let Err(error) = transport.initialize().await {
                engine.invalidate();
                console::println!(
                    "SDRESULT id={} status=unavailable error={:?}",
                    request.id,
                    error
                );
                reply_request(request.id, request.command, UploadStatus::Unavailable, 0);
                continue;
            }
            initialized = true;
        }
        let command = match request.command {
            RequestCommand::Fat(command) => command,
            #[cfg(feature = "wifi-storage")]
            RequestCommand::HttpFat(command) => command,
            #[cfg(feature = "wifi-storage")]
            RequestCommand::Upload(command) => {
                let engine_invalidated = process_upload(
                    request.id,
                    command,
                    request.deadline,
                    &mut upload_session,
                    &mut transport,
                    engine,
                    &mut output,
                )
                .await;
                initialized &= !engine_invalidated;
                SESSION_ACTIVE.store(upload_session.is_some(), Ordering::Release);
                continue;
            }
        };
        // Console reads stream the entire file; the shared command's bounded
        // read operation remains unchanged for other products and consumers.
        let operation = match command {
            StorageCommand::FatRead { path, path_len } => {
                Some(sdcard::fat::FatRequest::Stream { path, path_len })
            }
            _ => command.fat_request(output.len() as u32),
        };
        let Some(operation) = operation else {
            console::println!("SDRESULT id={} status=unsupported", request.id);
            continue;
        };
        #[cfg(feature = "wifi-storage")]
        if upload_session.is_some() {
            reply_request(request.id, request.command, UploadStatus::Busy, 0);
            console::println!("SDRESULT id={} status=busy", request.id);
            continue;
        }
        let started = Instant::now();
        let result = sdcard::service::run_fat_request_until(
            operation,
            &mut transport,
            engine,
            &mut FatBuffers {
                input: command.input_payload(),
                output: &mut output,
            },
            &mut Observer { id: request.id },
            request.deadline,
        )
        .await;
        #[cfg(feature = "wifi-storage")]
        let result = if matches!(request.command, RequestCommand::HttpFat(_)) {
            let policy = match command {
                StorageCommand::FatMkdir { .. } => sdcard::upload::PathOperation::Mkdir,
                StorageCommand::FatRemove { .. } => sdcard::upload::PathOperation::Remove,
                _ => sdcard::upload::PathOperation::Stat,
            };
            sdcard::upload::normalize_path_result(policy, result)
        } else {
            result
        };
        #[cfg(not(feature = "wifi-storage"))]
        let result = result;
        reply_request(request.id, request.command, status_for(&result), 0);
        if matches!(result, FatResult::Error(ref error) if error.is_transport_failure()) {
            initialized = false;
        }
        if let FatResult::Stat(entry) = &result {
            console::println!(
                "SDRESULT id={} result=Stat size={} dir={} elapsed_us={}",
                request.id,
                entry.size,
                entry.is_dir,
                started.elapsed().as_micros()
            );
        } else {
            console::println!(
                "SDRESULT id={} result={:?} elapsed_us={}",
                request.id,
                result,
                started.elapsed().as_micros()
            );
        }
    }
}

fn reply_request(id: u32, command: RequestCommand, status: UploadStatus, bytes_written: u32) {
    #[cfg(feature = "wifi-storage")]
    if matches!(
        command,
        RequestCommand::Upload(_) | RequestCommand::HttpFat(_)
    ) {
        reply_upload(id, status, bytes_written);
    }
    #[cfg(not(feature = "wifi-storage"))]
    let _ = (id, command, status, bytes_written);
}

#[cfg(feature = "wifi-storage")]
enum UploadOutcome {
    Report(FatResult),
    Replied(bool),
}

#[cfg(feature = "wifi-storage")]
async fn begin_upload(
    id: u32,
    path: [u8; sdcard::SD_PATH_MAX],
    path_len: u8,
    expected_size: u32,
    session: &mut Option<Session<72>>,
    executor: &mut Executor<'_, NativeTransport, Observer>,
    output: &mut [u8],
) -> UploadOutcome {
    if session.is_some() {
        return UploadOutcome::Report(FatResult::Error(sdcard::fat::FatEngineError::Busy));
    }
    let final_path = match sdcard::upload::validate_path(&path, path_len as usize, "/assets") {
        Ok(path) => path.as_bytes(),
        Err(_) => {
            reply_upload(id, UploadStatus::InvalidPath, 0);
            return UploadOutcome::Replied(false);
        }
    };
    let (temp, temp_len) =
        match sdcard::upload::temporary_path::<72>(final_path, "/assets", TEMP_BASENAME) {
            Ok(value) => value,
            Err(_) => {
                reply_upload(id, UploadStatus::InvalidPath, 0);
                return UploadOutcome::Replied(false);
            }
        };
    let core = match Session::begin(final_path, &temp[..temp_len], expected_size) {
        Ok(core) => core,
        Err(_) => {
            reply_upload(id, UploadStatus::InvalidPath, 0);
            return UploadOutcome::Replied(false);
        }
    };
    let result = executor
        .begin(&core, &mut FatBuffers { input: &[], output })
        .await;
    if matches!(result, FatResult::Done) {
        *session = Some(core);
    }
    UploadOutcome::Report(result)
}

#[cfg(feature = "wifi-storage")]
async fn chunk_upload(
    id: u32,
    data_len: u16,
    session: &mut Option<Session<72>>,
    executor: &mut Executor<'_, NativeTransport, Observer>,
    output: &mut [u8],
) -> UploadOutcome {
    if session.is_none() {
        reply_upload(id, UploadStatus::SessionNotActive, 0);
        return UploadOutcome::Replied(false);
    }
    let data = http::take_buffer();
    let Some(data) = data else {
        reply_upload(id, UploadStatus::Failed, 0);
        return UploadOutcome::Replied(false);
    };
    let result = if let Some(active) = session.as_mut() {
        if usize::from(data_len) > data.len() || u32::from(data_len) > active.remaining() {
            FatResult::Error(sdcard::fat::FatEngineError::InvalidState)
        } else {
            let result = executor
                .chunk(
                    u32::from(data_len),
                    &mut FatBuffers {
                        input: &data[..usize::from(data_len)],
                        output,
                    },
                )
                .await;
            if matches!(result, FatResult::Done) {
                let _ = active.accept_chunk(u32::from(data_len));
            }
            result
        }
    } else {
        FatResult::Error(sdcard::fat::FatEngineError::InvalidState)
    };
    http::return_buffer(data);
    UploadOutcome::Report(result)
}

#[cfg(feature = "wifi-storage")]
async fn commit_upload(
    id: u32,
    session: &mut Option<Session<72>>,
    executor: &mut Executor<'_, NativeTransport, Observer>,
    output: &mut [u8],
) -> UploadOutcome {
    let Some(active) = session.as_ref() else {
        reply_upload(id, UploadStatus::SessionNotActive, 0);
        return UploadOutcome::Replied(false);
    };
    if !active.is_complete() {
        UploadOutcome::Report(FatResult::Error(sdcard::fat::FatEngineError::InvalidState))
    } else {
        UploadOutcome::Report(
            executor
                .commit(active, &mut FatBuffers { input: &[], output })
                .await,
        )
    }
}

#[cfg(feature = "wifi-storage")]
async fn abort_upload(
    id: u32,
    session: &mut Option<Session<72>>,
    executor: &mut Executor<'_, NativeTransport, Observer>,
    output: &mut [u8],
) -> UploadOutcome {
    let Some(active) = session.take() else {
        reply_upload(id, UploadStatus::Done, 0);
        return UploadOutcome::Replied(false);
    };
    let (remove, clear) = executor
        .abort(&active, &mut FatBuffers { input: &[], output })
        .await;
    if matches!(clear, FatResult::Done) {
        UploadOutcome::Report(remove)
    } else {
        UploadOutcome::Report(clear)
    }
}

#[cfg(feature = "wifi-storage")]
async fn process_upload(
    id: u32,
    command: UploadCommand,
    deadline: Instant,
    session: &mut Option<Session<72>>,
    transport: &mut NativeTransport,
    engine: &mut FatEngine,
    output: &mut [u8],
) -> bool {
    let mut observer = Observer { id };
    let mut executor = Executor::new(transport, engine, &mut observer).with_deadline(deadline);
    let result = match command {
        UploadCommand::Begin {
            path,
            path_len,
            expected_size,
        } => {
            match begin_upload(
                id,
                path,
                path_len,
                expected_size,
                session,
                &mut executor,
                output,
            )
            .await
            {
                UploadOutcome::Report(result) => result,
                UploadOutcome::Replied(failed) => return failed,
            }
        }
        UploadCommand::Chunk { data_len } => {
            match chunk_upload(id, data_len, session, &mut executor, output).await {
                UploadOutcome::Report(result) => result,
                UploadOutcome::Replied(failed) => return failed,
            }
        }
        UploadCommand::Commit => match commit_upload(id, session, &mut executor, output).await {
            UploadOutcome::Report(result) => result,
            UploadOutcome::Replied(failed) => return failed,
        },
        UploadCommand::Abort => match abort_upload(id, session, &mut executor, output).await {
            UploadOutcome::Report(result) => result,
            UploadOutcome::Replied(failed) => return failed,
        },
    };
    console::println!("SDRESULT id={} upload_result={:?}", id, result);
    let status = status_for(&result);
    let bytes_written = session.as_ref().map_or(0, Session::bytes_written);
    let failed = matches!(result, FatResult::Error(ref error) if error.is_transport_failure());
    if failed || (matches!(command, UploadCommand::Commit) && status == UploadStatus::Done) {
        *session = None;
    }
    reply_upload(id, status, bytes_written);
    failed
}

fn status_for(result: &FatResult) -> UploadStatus {
    match result {
        FatResult::Done | FatResult::Stat(_) => UploadStatus::Done,
        FatResult::Error(sdcard::fat::FatEngineError::Fat(sdcard::fat::SdFatError::NotFound)) => {
            UploadStatus::NotFound
        }
        FatResult::Error(sdcard::fat::FatEngineError::Fat(sdcard::fat::SdFatError::NotEmpty)) => {
            UploadStatus::NotEmpty
        }
        FatResult::Error(sdcard::fat::FatEngineError::Fat(sdcard::fat::SdFatError::DirFull)) => {
            UploadStatus::DirectoryFull
        }
        FatResult::Error(sdcard::fat::FatEngineError::Fat(
            sdcard::fat::SdFatError::InvalidPath,
        )) => UploadStatus::InvalidPath,
        FatResult::Error(sdcard::fat::FatEngineError::InvalidState) => UploadStatus::SizeMismatch,
        FatResult::Error(sdcard::fat::FatEngineError::Busy) => UploadStatus::Busy,
        FatResult::Error(sdcard::fat::FatEngineError::TimedOut) => UploadStatus::TimedOut,
        FatResult::Error(sdcard::fat::FatEngineError::Cancelled) => UploadStatus::Cancelled,
        _ => UploadStatus::Failed,
    }
}
