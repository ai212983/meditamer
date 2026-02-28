use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex,
    mutex::{Mutex, MutexGuard},
};

use super::super::types::{SD_ASSET_READ_MAX, SD_UPLOAD_CHUNK_MAX};
#[cfg(feature = "psram-alloc")]
use super::super::{psram, psram::BufferAllocError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(feature = "psram-alloc"), allow(dead_code))]
pub(crate) enum TransferBufferError {
    Unavailable,
}

#[cfg(feature = "psram-alloc")]
fn map_alloc_error(_error: BufferAllocError) -> TransferBufferError {
    TransferBufferError::Unavailable
}

pub(crate) struct UploadChunkBuffer {
    #[cfg(feature = "psram-alloc")]
    data: Option<psram::LargeByteBuffer>,
    #[cfg(not(feature = "psram-alloc"))]
    data: [u8; SD_UPLOAD_CHUNK_MAX],
}

impl UploadChunkBuffer {
    const fn new() -> Self {
        Self {
            #[cfg(feature = "psram-alloc")]
            data: None,
            #[cfg(not(feature = "psram-alloc"))]
            data: [0; SD_UPLOAD_CHUNK_MAX],
        }
    }

    #[cfg(feature = "psram-alloc")]
    fn ensure_ready(&mut self) -> Result<(), TransferBufferError> {
        if self.data.is_none() {
            self.data =
                Some(psram::alloc_large_byte_buffer(SD_UPLOAD_CHUNK_MAX).map_err(map_alloc_error)?);
            psram::log_allocator_high_water("upload_chunk_buffer_alloc");
        }
        Ok(())
    }

    #[cfg(not(feature = "psram-alloc"))]
    fn ensure_ready(&mut self) -> Result<(), TransferBufferError> {
        Ok(())
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [u8] {
        #[cfg(feature = "psram-alloc")]
        {
            self.data
                .as_mut()
                .expect("upload chunk buffer must be initialized")
                .as_mut_slice()
        }
        #[cfg(not(feature = "psram-alloc"))]
        {
            &mut self.data
        }
    }

    fn release(&mut self) {
        #[cfg(feature = "psram-alloc")]
        {
            self.data = None;
        }
    }
}

pub(crate) struct AssetReadBuffer {
    #[cfg(feature = "psram-alloc")]
    data: Option<psram::LargeByteBuffer>,
    #[cfg(not(feature = "psram-alloc"))]
    data: [u8; SD_ASSET_READ_MAX],
}

impl AssetReadBuffer {
    const fn new() -> Self {
        Self {
            #[cfg(feature = "psram-alloc")]
            data: None,
            #[cfg(not(feature = "psram-alloc"))]
            data: [0; SD_ASSET_READ_MAX],
        }
    }

    #[cfg(feature = "psram-alloc")]
    fn ensure_ready(&mut self) -> Result<(), TransferBufferError> {
        if self.data.is_none() {
            self.data =
                Some(psram::alloc_large_byte_buffer(SD_ASSET_READ_MAX).map_err(map_alloc_error)?);
            psram::log_allocator_high_water("asset_read_buffer_alloc");
        }
        Ok(())
    }

    #[cfg(not(feature = "psram-alloc"))]
    fn ensure_ready(&mut self) -> Result<(), TransferBufferError> {
        Ok(())
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [u8] {
        #[cfg(feature = "psram-alloc")]
        {
            self.data
                .as_mut()
                .expect("asset read buffer must be initialized")
                .as_mut_slice()
        }
        #[cfg(not(feature = "psram-alloc"))]
        {
            &mut self.data
        }
    }

    fn release(&mut self) {
        #[cfg(feature = "psram-alloc")]
        {
            self.data = None;
        }
    }
}

static UPLOAD_CHUNK_BUFFER: Mutex<CriticalSectionRawMutex, UploadChunkBuffer> =
    Mutex::new(UploadChunkBuffer::new());
static ASSET_READ_BUFFER: Mutex<CriticalSectionRawMutex, AssetReadBuffer> =
    Mutex::new(AssetReadBuffer::new());

pub(crate) async fn lock_upload_chunk_buffer(
) -> Result<MutexGuard<'static, CriticalSectionRawMutex, UploadChunkBuffer>, TransferBufferError> {
    let mut guard = UPLOAD_CHUNK_BUFFER.lock().await;
    guard.ensure_ready()?;
    Ok(guard)
}

pub(crate) async fn lock_asset_read_buffer(
) -> Result<MutexGuard<'static, CriticalSectionRawMutex, AssetReadBuffer>, TransferBufferError> {
    let mut guard = ASSET_READ_BUFFER.lock().await;
    guard.ensure_ready()?;
    Ok(guard)
}

pub(crate) async fn release_upload_chunk_buffer() {
    let mut guard = UPLOAD_CHUNK_BUFFER.lock().await;
    guard.release();
}

pub(crate) async fn release_asset_read_buffer() {
    let mut guard = ASSET_READ_BUFFER.lock().await;
    guard.release();
}
