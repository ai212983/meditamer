//! Product-neutral upload session state and path policy.
//!
//! The FAT engine owns the on-card transaction (`UploadBegin`, chunks, commit,
//! and clear).  This module owns the small amount of state around that
//! transaction so a network or console front end can use the same atomic
//! temporary-file publication contract.

/// HTTP path operations retain the existing idempotent mkdir/remove policy.
#[derive(Clone, Copy)]
pub enum PathOperation {
    Mkdir,
    Remove,
    Stat,
}

pub fn normalize_path_result(operation: PathOperation, result: FatResult) -> FatResult {
    match (&operation, &result) {
        (
            PathOperation::Mkdir,
            FatResult::Error(FatEngineError::Fat(crate::fat::SdFatError::AlreadyExists)),
        )
        | (
            PathOperation::Remove,
            FatResult::Error(FatEngineError::Fat(crate::fat::SdFatError::NotFound)),
        ) => FatResult::Done,
        _ => result,
    }
}

use crate::fat::{FatEngine, FatEngineError, FatResult};
use crate::fat::{FatPayloadId, FatRequest};
use crate::service::{self, FatBuffers, FatObserver};
use crate::transport::SectorTransport;
use crate::SD_PATH_MAX;
use embassy_time::Instant;

pub const TEMP_BASENAME: &[u8] = b"HCTLUPLD.TMP";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathError {
    Empty,
    TooLong,
    InvalidUtf8,
    MustBeAbsolute,
    OutsideRoot,
    InvalidSegment,
    RootNotFile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionError {
    EmptyPath,
    PathTooLong,
    ExceedsExpectedSize,
    SizeOverflow,
    FatPathTooLong,
}

pub struct AbortRequests {
    pub remove: FatRequest,
    pub clear: FatRequest,
}

/// Reusable upload request driver. Products retain ownership of power,
/// transport construction, buffers, and observation; this adapter only
/// sequences the shared FAT transaction.
pub struct Executor<'a, T: SectorTransport, O: FatObserver> {
    transport: &'a mut T,
    engine: &'a mut FatEngine,
    observer: &'a mut O,
    deadline: Option<Instant>,
}

impl<'a, T: SectorTransport, O: FatObserver> Executor<'a, T, O> {
    pub fn new(transport: &'a mut T, engine: &'a mut FatEngine, observer: &'a mut O) -> Self {
        Self {
            transport,
            engine,
            observer,
            deadline: None,
        }
    }

    pub fn with_deadline(mut self, deadline: Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }

    async fn run(
        &mut self,
        request: Result<FatRequest, SessionError>,
        buffers: &mut FatBuffers<'_>,
    ) -> FatResult {
        match request {
            Ok(request) => match self.deadline {
                Some(deadline) => {
                    service::run_fat_request_until(
                        request,
                        self.transport,
                        self.engine,
                        buffers,
                        self.observer,
                        deadline,
                    )
                    .await
                }
                None => {
                    service::run_fat_request(
                        request,
                        self.transport,
                        self.engine,
                        buffers,
                        self.observer,
                    )
                    .await
                }
            },
            Err(_) => FatResult::Error(FatEngineError::InvalidState),
        }
    }

    pub async fn begin<const CAP: usize>(
        &mut self,
        session: &Session<CAP>,
        buffers: &mut FatBuffers<'_>,
    ) -> FatResult {
        self.run(session.begin_request(), buffers).await
    }

    pub async fn chunk(&mut self, length: u32, buffers: &mut FatBuffers<'_>) -> FatResult {
        self.run(Ok(Session::<1>::chunk_request(length)), buffers)
            .await
    }

    pub async fn commit<const CAP: usize>(
        &mut self,
        session: &Session<CAP>,
        buffers: &mut FatBuffers<'_>,
    ) -> FatResult {
        self.run(session.commit_request(), buffers).await
    }

    pub async fn abort<const CAP: usize>(
        &mut self,
        session: &Session<CAP>,
        buffers: &mut FatBuffers<'_>,
    ) -> (FatResult, FatResult) {
        let requests = match session.abort_requests() {
            Ok(requests) => requests,
            Err(_) => {
                return (
                    FatResult::Error(FatEngineError::InvalidState),
                    FatResult::Error(FatEngineError::InvalidState),
                )
            }
        };
        let remove = self.run(Ok(requests.remove), buffers).await;
        let clear = self.run(Ok(requests.clear), buffers).await;
        (remove, clear)
    }
}

/// Builds a temporary sibling path. The final name is never exposed until
/// the FAT engine has completed its commit/rename operation.
pub fn temporary_path<const CAP: usize>(
    final_path: &[u8],
    root: &str,
    temp_basename: &[u8],
) -> Result<([u8; CAP], usize), PathError> {
    if final_path == root.as_bytes() || final_path.ends_with(b"/") {
        return Err(PathError::RootNotFile);
    }
    let Some(separator) = final_path.iter().rposition(|byte| *byte == b'/') else {
        return Err(PathError::MustBeAbsolute);
    };
    if separator + 1 >= final_path.len() {
        return Err(PathError::RootNotFile);
    }
    let parent_len = if separator == 0 { 1 } else { separator };
    let temp_len = parent_len
        .checked_add(1)
        .and_then(|value| value.checked_add(temp_basename.len()))
        .ok_or(PathError::TooLong)?;
    if temp_len > CAP {
        return Err(PathError::TooLong);
    }
    let mut output = [0; CAP];
    output[..parent_len].copy_from_slice(&final_path[..parent_len]);
    output[parent_len] = b'/';
    output[parent_len + 1..temp_len].copy_from_slice(temp_basename);
    Ok((output, temp_len))
}

/// Validates an upload path and applies the product's permitted root.
pub fn validate_path<'a>(
    path: &'a [u8],
    path_len: usize,
    root: &str,
) -> Result<&'a str, PathError> {
    if path_len == 0 {
        return Err(PathError::Empty);
    }
    if path_len > path.len() {
        return Err(PathError::TooLong);
    }
    let value = core::str::from_utf8(&path[..path_len]).map_err(|_| PathError::InvalidUtf8)?;
    if !value.starts_with('/') {
        return Err(PathError::MustBeAbsolute);
    }
    if value != root
        && (!value.starts_with(root) || value.as_bytes().get(root.len()) != Some(&b'/'))
    {
        return Err(PathError::OutsideRoot);
    }
    for segment in value.split('/').skip(1) {
        if segment == "." || segment == ".." || segment.chars().any(|ch| ch.is_control()) {
            return Err(PathError::InvalidSegment);
        }
    }
    Ok(value)
}

/// State for one bounded, sequential upload. The caller supplies the path
/// capacity at compile time, keeping this usable in no-alloc firmware.
pub struct Session<const PATH_CAP: usize> {
    final_path: [u8; PATH_CAP],
    final_path_len: u8,
    temp_path: [u8; PATH_CAP],
    temp_path_len: u8,
    expected_size: u32,
    bytes_written: u32,
    last_activity_at: Instant,
}

impl<const PATH_CAP: usize> Session<PATH_CAP> {
    pub fn begin(
        final_path: &[u8],
        temp_path: &[u8],
        expected_size: u32,
    ) -> Result<Self, SessionError> {
        if final_path.is_empty() || temp_path.is_empty() {
            return Err(SessionError::EmptyPath);
        }
        if final_path.len() > PATH_CAP || temp_path.len() > PATH_CAP {
            return Err(SessionError::PathTooLong);
        }
        let mut final_buf = [0; PATH_CAP];
        let mut temp_buf = [0; PATH_CAP];
        final_buf[..final_path.len()].copy_from_slice(final_path);
        temp_buf[..temp_path.len()].copy_from_slice(temp_path);
        Ok(Self {
            final_path: final_buf,
            final_path_len: final_path.len() as u8,
            temp_path: temp_buf,
            temp_path_len: temp_path.len() as u8,
            expected_size,
            bytes_written: 0,
            last_activity_at: Instant::now(),
        })
    }

    pub const fn expected_size(&self) -> u32 {
        self.expected_size
    }
    pub const fn bytes_written(&self) -> u32 {
        self.bytes_written
    }
    pub const fn remaining(&self) -> u32 {
        self.expected_size.saturating_sub(self.bytes_written)
    }
    pub const fn final_path(&self) -> (&[u8; PATH_CAP], u8) {
        (&self.final_path, self.final_path_len)
    }
    pub const fn temp_path(&self) -> (&[u8; PATH_CAP], u8) {
        (&self.temp_path, self.temp_path_len)
    }
    pub const fn last_activity_at(&self) -> Instant {
        self.last_activity_at
    }

    pub fn accept_chunk(&mut self, length: u32) -> Result<u32, SessionError> {
        let next = self
            .bytes_written
            .checked_add(length)
            .ok_or(SessionError::SizeOverflow)?;
        if next > self.expected_size {
            return Err(SessionError::ExceedsExpectedSize);
        }
        self.bytes_written = next;
        self.last_activity_at = Instant::now();
        Ok(next)
    }

    pub const fn is_complete(&self) -> bool {
        self.bytes_written == self.expected_size
    }

    fn fat_path(
        &self,
        path: &[u8; PATH_CAP],
        len: u8,
    ) -> Result<([u8; SD_PATH_MAX], u8), SessionError> {
        let len = len as usize;
        if len > SD_PATH_MAX {
            return Err(SessionError::FatPathTooLong);
        }
        let mut output = [0; SD_PATH_MAX];
        output[..len].copy_from_slice(&path[..len]);
        Ok((output, len as u8))
    }

    pub fn begin_request(&self) -> Result<FatRequest, SessionError> {
        let (path, path_len) = self.fat_path(&self.temp_path, self.temp_path_len)?;
        Ok(FatRequest::UploadBegin {
            path,
            path_len,
            expected_size: self.expected_size,
        })
    }

    pub const fn chunk_request(input_len: u32) -> FatRequest {
        FatRequest::UploadChunk {
            input: FatPayloadId::Primary,
            input_len,
        }
    }

    pub fn commit_request(&self) -> Result<FatRequest, SessionError> {
        let (path, path_len) = self.fat_path(&self.final_path, self.final_path_len)?;
        Ok(FatRequest::UploadCommit { path, path_len })
    }

    pub fn abort_requests(&self) -> Result<AbortRequests, SessionError> {
        let (path, path_len) = self.fat_path(&self.temp_path, self.temp_path_len)?;
        Ok(AbortRequests {
            remove: FatRequest::Remove { path, path_len },
            clear: FatRequest::UploadClear,
        })
    }
}

#[cfg(all(test, feature = "host-tests"))]
mod tests {
    use super::*;

    #[test]
    fn validates_root_and_rejects_traversal() {
        assert!(validate_path(b"/assets/a.bin", 13, "/assets").is_ok());
        assert_eq!(
            validate_path(b"/assets/../x", 11, "/assets"),
            Err(PathError::InvalidSegment)
        );
        assert_eq!(
            validate_path(b"/other/a", 8, "/assets"),
            Err(PathError::OutsideRoot)
        );
    }

    #[test]
    fn temp_path_is_sibling() {
        let (path, len) = temporary_path::<32>(b"/assets/a.bin", "/assets", TEMP_BASENAME).unwrap();
        assert_eq!(&path[..len], b"/assets/HCTLUPLD.TMP");
    }

    #[test]
    fn session_enforces_expected_size() {
        let mut session = Session::<32>::begin(b"/assets/a", b"/assets/HCTLUPLD.TMP", 3).unwrap();
        assert_eq!(session.accept_chunk(2), Ok(2));
        assert!(!session.is_complete());
        assert_eq!(
            session.accept_chunk(2),
            Err(SessionError::ExceedsExpectedSize)
        );
        assert_eq!(session.accept_chunk(1), Ok(3));
        assert!(session.is_complete());
    }

    #[test]
    fn normalizes_idempotent_mkdir_and_remove_errors() {
        let mkdir = FatResult::Error(FatEngineError::Fat(crate::fat::SdFatError::AlreadyExists));
        let remove = FatResult::Error(FatEngineError::Fat(crate::fat::SdFatError::NotFound));

        assert!(matches!(
            normalize_path_result(PathOperation::Mkdir, mkdir),
            FatResult::Done
        ));
        assert!(matches!(
            normalize_path_result(PathOperation::Remove, remove),
            FatResult::Done
        ));
    }

    #[test]
    fn preserves_non_idempotent_path_errors() {
        let mkdir = FatResult::Error(FatEngineError::Fat(crate::fat::SdFatError::NotFound));
        let remove = FatResult::Error(FatEngineError::Fat(crate::fat::SdFatError::AlreadyExists));

        assert!(matches!(
            normalize_path_result(PathOperation::Mkdir, mkdir),
            FatResult::Error(FatEngineError::Fat(crate::fat::SdFatError::NotFound))
        ));
        assert!(matches!(
            normalize_path_result(PathOperation::Remove, remove),
            FatResult::Error(FatEngineError::Fat(crate::fat::SdFatError::AlreadyExists))
        ));
    }
}
