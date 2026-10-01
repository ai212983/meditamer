//! HTTP tokens serialize one outstanding operation. Cancelling the HTTP future
//! leaves its token here until the owner has completed all FAT and DMA access.
use super::*;
use embassy_sync::blocking_mutex::Mutex;
use http_upload::{ChunkFinish, ChunkStarted, ChunkTryFinish, Command, Host};

static BUFFER: Mutex<CriticalSectionRawMutex, RefCell<Option<&'static mut [u8]>>> =
    Mutex::new(RefCell::new(None));
static PENDING: AtomicU32 = AtomicU32::new(0);

pub fn init_upload_buffer(buffer: &'static mut [u8]) {
    assert!(buffer.len() >= 8192);
    BUFFER.lock(|slot| {
        let mut slot = slot.borrow_mut();
        assert!(slot.is_none());
        *slot = Some(buffer);
    });
}
pub(super) fn take_buffer() -> Option<&'static mut [u8]> {
    BUFFER.lock(|slot| slot.borrow_mut().take())
}
pub(super) fn return_buffer(buffer: &'static mut [u8]) {
    BUFFER.lock(|slot| {
        let previous = slot.borrow_mut().replace(buffer);
        assert!(previous.is_none());
    });
}
pub fn active_roundtrips() -> u16 {
    u16::from(PENDING.load(Ordering::Acquire) != 0)
}
pub fn upload_session_active() -> bool {
    SESSION_ACTIVE.load(Ordering::Acquire)
}

pub struct HttpHost;
pub struct Inflight {
    id: u32,
    started: Instant,
}
fn submit(command: RequestCommand) -> Result<Inflight, UploadStatus> {
    if !ADMISSION.load(Ordering::Acquire) {
        return Err(UploadStatus::Cancelled);
    }
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed).max(1);
    PENDING
        .compare_exchange(0, id, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| UploadStatus::Busy)?;
    let started = Instant::now();
    if REQUESTS
        .try_send(Request {
            id,
            command,
            deadline: started + Duration::from_secs(10),
        })
        .is_err()
    {
        PENDING.store(0, Ordering::Release);
        return Err(UploadStatus::Busy);
    }
    Ok(Inflight { id, started })
}
fn finish(token: Inflight, response: UploadResponse) -> Result<ChunkFinish, UploadStatus> {
    if response.id != token.id {
        return Err(UploadStatus::Failed);
    }
    PENDING.store(0, Ordering::Release);
    if response.status != UploadStatus::Done {
        return Err(response.status);
    }
    Ok(ChunkFinish {
        roundtrip_ms: token.started.elapsed().as_millis() as u32,
        queue_wait_ms: 0,
        handler_ms: 0,
        post_handler_ms: 0,
        publish_to_receive_ms: 0,
    })
}
async fn wait(token: Inflight) -> Result<ChunkFinish, UploadStatus> {
    // A timeout does not free the token or the staging buffer; the cancellation
    // barrier can subsequently consume the owner's terminal reply.
    let response = with_timeout(Duration::from_secs(12), UPLOAD_RESULTS.receive())
        .await
        .map_err(|_| UploadStatus::TimedOut)?;
    finish(token, response)
}
pub async fn abort_upload_barrier() -> bool {
    let id = PENDING.load(Ordering::Acquire);
    if id != 0
        && with_timeout(Duration::from_secs(12), async {
            loop {
                let response = UPLOAD_RESULTS.receive().await;
                if response.id == id {
                    PENDING.store(0, Ordering::Release);
                    break;
                }
            }
        })
        .await
        .is_err()
    {
        return false;
    }
    if !upload_session_active() {
        return true;
    }
    match submit(RequestCommand::Upload(UploadCommand::Abort)) {
        Ok(token) => wait(token).await.is_ok(),
        Err(_) => false,
    }
}
impl Host for HttpHost {
    type Error = UploadStatus;
    type Inflight = Inflight;
    fn admission_open() -> bool {
        ADMISSION.load(Ordering::Acquire)
            && crate::net_host::is_enabled()
            && netstack::host::radio_handoff_admission_open()
    }
    fn upload_token() -> Option<&'static [u8]> {
        option_env!("MEDITAMER_UPLOAD_HTTP_TOKEN")
            .or(option_env!("UPLOAD_HTTP_TOKEN"))
            .filter(|value| !value.is_empty())
            .map(str::as_bytes)
    }
    async fn roundtrip(command: Command) -> Result<(), Self::Error> {
        let command = match command {
            Command::Begin {
                path,
                path_len,
                expected_size,
            } => RequestCommand::Upload(UploadCommand::Begin {
                path,
                path_len,
                expected_size,
            }),
            Command::Commit => RequestCommand::Upload(UploadCommand::Commit),
            Command::Abort => RequestCommand::Upload(UploadCommand::Abort),
            Command::Mkdir { path, path_len } => {
                RequestCommand::HttpFat(StorageCommand::FatMkdir { path, path_len })
            }
            Command::Remove { path, path_len } => {
                RequestCommand::HttpFat(StorageCommand::FatRemove { path, path_len })
            }
            Command::Stat { path, path_len } => {
                RequestCommand::HttpFat(StorageCommand::FatStat { path, path_len })
            }
        };
        wait(submit(command)?).await.map(|_| ())
    }
    async fn chunk_start(data: &[u8]) -> Result<ChunkStarted<Inflight>, Self::Error> {
        if PENDING.load(Ordering::Acquire) != 0 {
            return Err(UploadStatus::Busy);
        }
        let buffer = take_buffer().ok_or(UploadStatus::Busy)?;
        if data.len() > buffer.len() || data.len() > u16::MAX as usize {
            return_buffer(buffer);
            return Err(UploadStatus::SizeMismatch);
        }
        buffer[..data.len()].copy_from_slice(data);
        return_buffer(buffer);
        let transfer = submit(RequestCommand::Upload(UploadCommand::Chunk {
            data_len: data.len() as u16,
        }))?;
        Ok(ChunkStarted {
            transfer,
            copy_ms: 0,
        })
    }
    async fn chunk_finish(transfer: Inflight) -> Result<ChunkFinish, Self::Error> {
        wait(transfer).await
    }
    fn chunk_try_finish(transfer: Inflight) -> Result<ChunkTryFinish<Inflight>, Self::Error> {
        match UPLOAD_RESULTS.try_receive() {
            Ok(response) => finish(transfer, response).map(ChunkTryFinish::Finished),
            Err(_) => Ok(ChunkTryFinish::Pending(transfer)),
        }
    }
    fn error_log(error: Self::Error) -> &'static str {
        match error {
            UploadStatus::NotFound => "not found",
            UploadStatus::Busy => "sd busy",
            UploadStatus::SessionNotActive => "upload session not active",
            UploadStatus::NotEmpty => "directory not empty",
            UploadStatus::DirectoryFull => "sd directory entries full",
            UploadStatus::InvalidPath => "invalid path",
            UploadStatus::SizeMismatch => "size mismatch",
            UploadStatus::Unavailable => "sd init failed",
            UploadStatus::TimedOut => "sd upload timeout",
            UploadStatus::Cancelled => "cancelled",
            _ => "sd operation failed",
        }
    }
    fn error_status(error: Self::Error) -> &'static [u8] {
        match error {
            UploadStatus::NotFound => b"404 Not Found",
            UploadStatus::Busy | UploadStatus::SessionNotActive | UploadStatus::NotEmpty => {
                b"409 Conflict"
            }
            UploadStatus::TimedOut => b"504 Gateway Timeout",
            UploadStatus::DirectoryFull => b"507 Insufficient Storage",
            UploadStatus::Cancelled | UploadStatus::Unavailable => b"503 Service Unavailable",
            UploadStatus::InvalidPath | UploadStatus::SizeMismatch => b"400 Bad Request",
            _ => b"500 Internal Server Error",
        }
    }
    fn error_body(error: Self::Error) -> &'static [u8] {
        Self::error_log(error).as_bytes()
    }
}
