//! Stackless FAT32 operation engine.
//!
//! This module deliberately contains no async code. It advances one CPU stage
//! at a time and asks its owner to execute at most one block-device action.

mod mount;
mod mutate;
mod read;
mod scan;
mod types;

pub use types::{
    FatBufferId, FatEngineError, FatIoAction, FatIoCompletion, FatPayloadId, FatRequest, FatResult,
    FatStageLabel, FatStep, SdWorkspace, FAT_ENGINE_LIST_CAPACITY,
};

use super::*;
use mount::MountStage;
use mutate::{
    AllocationState, DataWriteState, FatWriteState, FreeState, MutationState, ZeroWriteState,
};
use read::{ListState, ReadState, StreamState};
use scan::{FatReadState, ResolveState, ScanState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScanReturn {
    Resolve,
    Target,
    Mutation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FatReadReturn {
    Scan,
    List,
    Read,
    Stream,
    DataWrite,
    UploadCursor,
    AppendTraverse,
    TruncateTraverse,
    TruncateFreeStart,
    ZeroWrite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FatWriteReturn {
    AllocateBatchLink,
    DirectoryLink,
    AppendLink,
    TruncateLink,
    TruncateCut,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommandStage {
    Idle,
    Mount,
    Resolve,
    FindTarget,
    ReadFat,
    WriteFat,
    Allocate,
    Free,
    DataWrite,
    ZeroWrite,
    Mutate,
    List,
    Read,
    Stream,
}

#[derive(Clone, Copy)]
struct UploadSessionState {
    valid: bool,
    volume: Fat32Volume,
    location: DirLocation,
    parent_cluster: u32,
    record: DirRecord,
    allocated_clusters: u32,
    tail_cluster: u32,
    write_cluster: u32,
    contiguous: bool,
}

impl UploadSessionState {
    const fn new() -> Self {
        Self {
            valid: false,
            volume: Fat32Volume {
                fat_start_lba: 0,
                fat_size_sectors: 0,
                fats: 0,
                data_start_lba: 0,
                sectors_per_cluster: 0,
                root_cluster: 0,
                total_clusters: 0,
            },
            location: DirLocation::ZERO,
            parent_cluster: 0,
            record: DirRecord {
                short_name: [0; 11],
                display_name: [0; FAT_NAME_MAX],
                display_name_len: 0,
                attr: 0,
                first_cluster: 0,
                size: 0,
            },
            allocated_clusters: 0,
            tail_cluster: 0,
            write_cluster: 0,
            contiguous: false,
        }
    }

    fn clear(&mut self) {
        self.valid = false;
        self.parent_cluster = 0;
        self.allocated_clusters = 0;
        self.tail_cluster = 0;
        self.write_cluster = 0;
        self.contiguous = false;
        self.record.first_cluster = 0;
        self.record.size = 0;
    }
}

/// Fixed-state FAT32 interpreter owned by the SD task.
pub struct FatEngine {
    workspace: SdWorkspace,
    request: Option<FatRequest>,
    stage: CommandStage,
    mount: MountStage,
    resolve: ResolveState,
    scan: ScanState,
    fat_read: FatReadState,
    list: ListState,
    read: ReadState,
    stream: StreamState,
    mutation: MutationState,
    allocation: AllocationState,
    free: FreeState,
    data_write: DataWriteState,
    zero_write: ZeroWriteState,
    fat_write: FatWriteState,
    volume: Option<Fat32Volume>,
    target: Option<DirFound>,
    scan_return: ScanReturn,
    fat_read_return: FatReadReturn,
    fat_write_return: FatWriteReturn,
    pending: bool,
    upload: UploadSessionState,
}

impl FatEngine {
    pub fn new() -> Self {
        Self {
            workspace: SdWorkspace::new(),
            request: None,
            stage: CommandStage::Idle,
            mount: MountStage::new(),
            resolve: ResolveState::new(),
            scan: ScanState::new(),
            fat_read: FatReadState::new(),
            list: ListState::new(),
            read: ReadState::new(),
            stream: StreamState::new(),
            mutation: MutationState::new(),
            allocation: AllocationState::new(),
            free: FreeState::new(),
            data_write: DataWriteState::new(),
            zero_write: ZeroWriteState::new(),
            fat_write: FatWriteState::new(),
            volume: None,
            target: None,
            scan_return: ScanReturn::Resolve,
            fat_read_return: FatReadReturn::Scan,
            fat_write_return: FatWriteReturn::DirectoryLink,
            pending: false,
            upload: UploadSessionState::new(),
        }
    }

    pub fn workspace(&self) -> &SdWorkspace {
        &self.workspace
    }

    pub fn workspace_mut(&mut self) -> &mut SdWorkspace {
        &mut self.workspace
    }

    pub fn is_busy(&self) -> bool {
        self.stage != CommandStage::Idle
    }

    pub fn has_outstanding_io(&self) -> bool {
        self.pending
    }

    pub fn list_output_sequence(&self) -> usize {
        self.list.count
    }

    /// Total payload bytes delivered by the current [`FatRequest::Stream`]
    /// operation so far. Monotonically increasing; the driver loop polls it
    /// the same way it polls [`FatEngine::list_output_sequence`] to notice a
    /// freshly delivered chunk.
    pub fn stream_bytes_delivered(&self) -> u32 {
        self.stream.written
    }

    /// Length of the most recently delivered chunk in
    /// `workspace().sector`. Valid only immediately after
    /// [`FatEngine::stream_bytes_delivered`] has just increased.
    pub fn stream_chunk_len(&self) -> u16 {
        self.stream.chunk_len
    }

    pub fn stage_label(&self) -> FatStageLabel {
        match self.stage {
            CommandStage::Idle => FatStageLabel::Idle,
            CommandStage::Mount => self.mount.label(),
            CommandStage::Resolve => self.resolve.label(),
            CommandStage::FindTarget => self.scan.label(),
            CommandStage::ReadFat => FatStageLabel::ReadFat,
            CommandStage::WriteFat => FatStageLabel::UpdateDirectory,
            CommandStage::Allocate => FatStageLabel::Allocate,
            CommandStage::Free => FatStageLabel::FreeChain,
            CommandStage::DataWrite => FatStageLabel::WriteFile,
            CommandStage::ZeroWrite => FatStageLabel::WriteFile,
            CommandStage::Mutate => self.mutation.label(),
            CommandStage::List => self.list.label(),
            CommandStage::Read => self.read.label(),
            CommandStage::Stream => self.stream.label(),
        }
    }

    pub fn start(&mut self, request: FatRequest) -> Result<(), FatEngineError> {
        if self.is_busy() {
            return Err(FatEngineError::Busy);
        }
        self.workspace.reset_operation();
        self.request = Some(request);
        self.mount.reset();
        self.resolve.reset();
        self.scan.reset();
        self.fat_read.reset();
        self.list.reset();
        self.read.reset();
        self.stream.reset();
        self.mutation.reset();
        self.allocation.reset();
        self.free.reset();
        self.data_write.reset();
        self.zero_write.reset();
        self.fat_write.reset();
        self.volume = None;
        self.target = None;
        self.pending = false;
        match request {
            FatRequest::UploadChunk { .. }
            | FatRequest::UploadCommit { .. }
            | FatRequest::UploadFlush => {
                if self.upload.valid {
                    self.volume = Some(self.upload.volume);
                    self.target = Some(DirFound {
                        short_location: self.upload.location,
                        lfn_locations: [DirLocation::ZERO; MAX_LFN_SLOTS],
                        lfn_count: 0,
                        record: self.upload.record,
                    });
                    self.resolve.start(self.upload.volume.root_cluster, 0);
                }
                self.stage = CommandStage::Mutate;
            }
            FatRequest::UploadClear => {
                self.upload.clear();
                self.stage = CommandStage::Mutate;
            }
            _ => self.stage = CommandStage::Mount,
        }
        Ok(())
    }

    pub fn invalidate(&mut self) {
        self.request = None;
        self.volume = None;
        self.target = None;
        self.pending = false;
        self.stage = CommandStage::Idle;
        self.workspace.reset_operation();
        self.upload.clear();
    }

    pub fn advance(&mut self, completion: FatIoCompletion) -> FatStep {
        if self.stage == CommandStage::Idle {
            return FatStep::Complete(FatResult::Error(FatEngineError::NotStarted));
        }
        if self.pending {
            match completion {
                FatIoCompletion::Pending => {
                    return FatStep::Complete(FatResult::Error(
                        FatEngineError::MissingIoCompletion,
                    ));
                }
                FatIoCompletion::Done => self.pending = false,
                FatIoCompletion::Failed(err) => {
                    return self.finish(FatResult::Error(FatEngineError::Io(err)));
                }
                FatIoCompletion::TimedOut => {
                    return self.finish(FatResult::Error(FatEngineError::TimedOut));
                }
                FatIoCompletion::InvalidState => {
                    return self.finish(FatResult::Error(FatEngineError::InvalidState));
                }
            }
        } else if !matches!(completion, FatIoCompletion::Pending) {
            return self.finish(FatResult::Error(FatEngineError::UnexpectedIoCompletion));
        }

        match self.advance_cpu() {
            Ok(step) => step,
            Err(err) => self.finish(FatResult::Error(FatEngineError::Fat(err))),
        }
    }

    fn advance_cpu(&mut self) -> Result<FatStep, SdFatError> {
        match self.stage {
            CommandStage::Idle => Ok(FatStep::Complete(FatResult::Error(
                FatEngineError::NotStarted,
            ))),
            CommandStage::Mount => self.advance_mount(),
            CommandStage::Resolve => self.advance_resolve(),
            CommandStage::FindTarget => self.advance_find_target(),
            CommandStage::ReadFat => self.advance_fat_read(),
            CommandStage::WriteFat => self.advance_fat_write(),
            CommandStage::Allocate => self.advance_allocate(),
            CommandStage::Free => self.advance_free(),
            CommandStage::DataWrite => self.advance_data_write(),
            CommandStage::ZeroWrite => self.advance_zero_write(),
            CommandStage::Mutate => self.advance_mutation(),
            CommandStage::List => self.advance_list(),
            CommandStage::Read => self.advance_read(),
            CommandStage::Stream => self.advance_stream(),
        }
    }

    fn issue(&mut self, action: FatIoAction) -> FatStep {
        debug_assert!(!self.pending);
        self.pending = true;
        FatStep::Io(action)
    }

    fn finish(&mut self, result: FatResult) -> FatStep {
        self.request = None;
        self.target = None;
        self.pending = false;
        self.stage = CommandStage::Idle;
        FatStep::Complete(result)
    }
}

impl Default for FatEngine {
    fn default() -> Self {
        Self::new()
    }
}
