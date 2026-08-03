use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    mutex::{Mutex, MutexGuard},
};

use super::super::types::SD_UPLOAD_CHUNK_MAX;
use super::super::{psram, psram::BufferAllocError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransferBufferError {
    Unavailable,
}

fn map_alloc_error(_error: BufferAllocError) -> TransferBufferError {
    TransferBufferError::Unavailable
}

pub(crate) struct UploadChunkBuffer {
    data: Option<psram::LargeByteBuffer>,
}

impl UploadChunkBuffer {
    const fn new() -> Self {
        Self { data: None }
    }

    fn ensure_ready(&mut self) -> Result<(), TransferBufferError> {
        if self.data.is_none() {
            self.data =
                Some(psram::alloc_large_byte_buffer(SD_UPLOAD_CHUNK_MAX).map_err(map_alloc_error)?);
            psram::log_allocator_high_water("upload_chunk_buffer_alloc");
        }
        Ok(())
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [u8] {
        self.data
            .as_mut()
            .expect("upload chunk buffer must be initialized")
            .as_mut_slice()
    }

    fn release(&mut self) {
        self.data = None;
    }
}

static UPLOAD_CHUNK_BUFFER: Mutex<CriticalSectionRawMutex, UploadChunkBuffer> =
    Mutex::new(UploadChunkBuffer::new());

pub(crate) async fn lock_upload_chunk_buffer(
) -> Result<MutexGuard<'static, CriticalSectionRawMutex, UploadChunkBuffer>, TransferBufferError> {
    let mut guard = UPLOAD_CHUNK_BUFFER.lock().await;
    guard.ensure_ready()?;
    Ok(guard)
}

pub(crate) async fn release_upload_chunk_buffer() {
    let mut guard = UPLOAD_CHUNK_BUFFER.lock().await;
    guard.release();
}
