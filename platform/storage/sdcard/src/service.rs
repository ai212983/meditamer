//! Shared asynchronous FAT execution boundary.

use embassy_time::Instant;
#[cfg(target_os = "none")]
use embassy_time::Timer;

use crate::fat::{
    FatBufferId, FatDirEntry, FatEngine, FatIoAction, FatIoCompletion, FatPayloadId, FatRequest,
    FatResult, FatStageLabel, FatStep,
};
use crate::transport::{SectorTransport, TransportError};

/// Caller-owned payload buffers. No storage is allocated by the service.
pub struct FatBuffers<'a> {
    pub input: &'a [u8],
    pub output: &'a mut [u8],
}

pub trait FatObserver {
    fn on_list_entry(&mut self, _entry: &FatDirEntry) {}
    fn on_stage(&mut self, _stage: FatStageLabel, _before_io: bool) {}
    fn on_transport_error(
        &mut self,
        _stage: FatStageLabel,
        _action: FatIoAction,
        _error: TransportError,
    ) {
    }
    fn on_complete(&mut self, _result: &FatResult) {}
    fn on_stream_chunk(&mut self, _offset: u32, _bytes: &[u8]) {}
    fn should_cancel(&self) -> bool {
        false
    }
}

impl FatObserver for () {}

fn complete(result: Result<(), TransportError>) -> FatIoCompletion {
    match result {
        Ok(()) => FatIoCompletion::Done,
        Err(error) if error.is_timeout() => FatIoCompletion::TimedOut(error),
        Err(error) => FatIoCompletion::Failed(error),
    }
}

async fn do_read_sector<T: SectorTransport>(
    transport: &mut T,
    engine: &mut FatEngine,
    lba: u32,
    buffer: FatBufferId,
) -> FatIoCompletion {
    if buffer != FatBufferId::Sector {
        return FatIoCompletion::InvalidState;
    }
    complete(
        transport
            .read_sector(lba, &mut engine.workspace_mut().sector)
            .await
            .map_err(Into::into),
    )
}

async fn do_write_sector<T: SectorTransport>(
    transport: &mut T,
    engine: &mut FatEngine,
    lba: u32,
    buffer: FatBufferId,
) -> FatIoCompletion {
    if buffer != FatBufferId::Sector {
        return FatIoCompletion::InvalidState;
    }
    complete(
        transport
            .write_sector(lba, &engine.workspace().sector)
            .await
            .map_err(Into::into),
    )
}

async fn do_read_sector_to_payload<T: SectorTransport>(
    transport: &mut T,
    engine: &mut FatEngine,
    buffers: &mut FatBuffers<'_>,
    action: FatIoAction,
) -> FatIoCompletion {
    let FatIoAction::ReadSectorToPayload {
        lba,
        buffer,
        payload,
        payload_offset,
        sector_offset,
        len,
    } = action
    else {
        return FatIoCompletion::InvalidState;
    };
    if buffer != FatBufferId::Sector || payload != FatPayloadId::Primary {
        return FatIoCompletion::InvalidState;
    }
    let start = match usize::try_from(payload_offset) {
        Ok(v) => v,
        Err(_) => return FatIoCompletion::InvalidState,
    };
    let source = usize::from(sector_offset);
    let len = usize::from(len);
    let end = match start.checked_add(len) {
        Some(v) => v,
        None => return FatIoCompletion::InvalidState,
    };
    let source_end = match source.checked_add(len) {
        Some(v) => v,
        None => return FatIoCompletion::InvalidState,
    };
    if end > buffers.output.len() {
        return FatIoCompletion::InvalidState;
    }
    if source_end > engine.workspace().sector.len() {
        return FatIoCompletion::InvalidState;
    }
    let result = transport
        .read_sector(lba, &mut engine.workspace_mut().sector)
        .await
        .map_err(Into::into);
    if result.is_ok() {
        buffers.output[start..end].copy_from_slice(&engine.workspace().sector[source..source_end]);
    }
    complete(result)
}

async fn do_write_sector_from_payload<T: SectorTransport>(
    transport: &mut T,
    engine: &mut FatEngine,
    buffers: &mut FatBuffers<'_>,
    action: FatIoAction,
) -> FatIoCompletion {
    let FatIoAction::WriteSectorFromPayload {
        lba,
        buffer,
        payload,
        payload_offset,
        sector_offset,
        len,
        preserve_existing,
    } = action
    else {
        return FatIoCompletion::InvalidState;
    };
    if buffer != FatBufferId::Sector || payload != FatPayloadId::Primary {
        return FatIoCompletion::InvalidState;
    }
    let src = match usize::try_from(payload_offset) {
        Ok(v) => v,
        Err(_) => return FatIoCompletion::InvalidState,
    };
    let dst = usize::from(sector_offset);
    let len = usize::from(len);
    let src_end = match src.checked_add(len) {
        Some(v) => v,
        None => return FatIoCompletion::InvalidState,
    };
    let dst_end = match dst.checked_add(len) {
        Some(v) => v,
        None => return FatIoCompletion::InvalidState,
    };
    if src_end > buffers.input.len() || dst_end > engine.workspace().sector.len() {
        return FatIoCompletion::InvalidState;
    }
    if !preserve_existing {
        engine.workspace_mut().sector.fill(0);
    }
    engine.workspace_mut().sector[dst..dst_end].copy_from_slice(&buffers.input[src..src_end]);
    complete(
        transport
            .write_sector(lba, &engine.workspace().sector)
            .await
            .map_err(Into::into),
    )
}

async fn do_write_payload_sectors<T: SectorTransport>(
    transport: &mut T,
    buffers: &mut FatBuffers<'_>,
    start_lba: u32,
    payload: FatPayloadId,
    payload_offset: u32,
    sectors: u16,
) -> FatIoCompletion {
    if payload != FatPayloadId::Primary {
        return FatIoCompletion::InvalidState;
    }
    let start = match usize::try_from(payload_offset) {
        Ok(v) => v,
        Err(_) => return FatIoCompletion::InvalidState,
    };
    let bytes = match usize::from(sectors).checked_mul(512) {
        Some(v) => v,
        None => return FatIoCompletion::InvalidState,
    };
    let end = match start.checked_add(bytes) {
        Some(v) => v,
        None => return FatIoCompletion::InvalidState,
    };
    if end > buffers.input.len() {
        return FatIoCompletion::InvalidState;
    }
    complete(
        transport
            .write_sectors_contiguous(start_lba, &buffers.input[start..end])
            .await
            .map_err(Into::into),
    )
}

pub async fn execute_action<T: SectorTransport>(
    action: FatIoAction,
    transport: &mut T,
    engine: &mut FatEngine,
    buffers: &mut FatBuffers<'_>,
) -> FatIoCompletion {
    match action {
        FatIoAction::ReadSector { lba, buffer } => {
            do_read_sector(transport, engine, lba, buffer).await
        }
        FatIoAction::WriteSector { lba, buffer } => {
            do_write_sector(transport, engine, lba, buffer).await
        }
        action @ FatIoAction::ReadSectorToPayload { .. } => {
            do_read_sector_to_payload(transport, engine, buffers, action).await
        }
        action @ FatIoAction::WriteSectorFromPayload { .. } => {
            do_write_sector_from_payload(transport, engine, buffers, action).await
        }
        FatIoAction::WritePayloadSectors {
            start_lba,
            payload,
            payload_offset,
            sectors,
        } => {
            do_write_payload_sectors(
                transport,
                buffers,
                start_lba,
                payload,
                payload_offset,
                sectors,
            )
            .await
        }
    }
}

struct YieldOnce(bool);
impl core::future::Future for YieldOnce {
    type Output = ();
    fn poll(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<()> {
        if self.0 {
            core::task::Poll::Ready(())
        } else {
            self.0 = true;
            cx.waker().wake_by_ref();
            core::task::Poll::Pending
        }
    }
}

async fn yield_once() {
    YieldOnce(false).await
}

async fn yield_after_io() {
    #[cfg(target_os = "none")]
    Timer::after_micros(50).await;
    #[cfg(not(target_os = "none"))]
    yield_once().await;
}

pub async fn run_fat_request<T: SectorTransport, O: FatObserver>(
    request: FatRequest,
    transport: &mut T,
    engine: &mut FatEngine,
    buffers: &mut FatBuffers<'_>,
    observer: &mut O,
) -> FatResult {
    run_fat_request_inner(request, transport, engine, buffers, observer, None).await
}

pub async fn run_fat_request_until<T: SectorTransport, O: FatObserver>(
    request: FatRequest,
    transport: &mut T,
    engine: &mut FatEngine,
    buffers: &mut FatBuffers<'_>,
    observer: &mut O,
    deadline: Instant,
) -> FatResult {
    run_fat_request_inner(
        request,
        transport,
        engine,
        buffers,
        observer,
        Some(deadline),
    )
    .await
}

async fn run_fat_request_inner<T: SectorTransport, O: FatObserver>(
    request: FatRequest,
    transport: &mut T,
    engine: &mut FatEngine,
    buffers: &mut FatBuffers<'_>,
    observer: &mut O,
    deadline: Option<Instant>,
) -> FatResult {
    if deadline.is_some_and(|limit| Instant::now() >= limit) || observer.should_cancel() {
        if deadline.is_some_and(|limit| Instant::now() >= limit) {
            engine.invalidate();
        }
        let error = if observer.should_cancel() {
            crate::fat::FatEngineError::Cancelled
        } else {
            crate::fat::FatEngineError::TimedOut
        };
        let result = FatResult::Error(error);
        observer.on_complete(&result);
        return result;
    }
    if let Err(error) = engine.start(request) {
        let result = FatResult::Error(error);
        observer.on_complete(&result);
        return result;
    }
    let mut completion = FatIoCompletion::Pending;
    let mut listed = 0;
    let mut streamed = 0;
    let mut transitions = 0u8;
    let mut slice_started = Instant::now();
    loop {
        if deadline.is_some_and(|limit| Instant::now() >= limit) || observer.should_cancel() {
            engine.invalidate();
            let error = if observer.should_cancel() {
                crate::fat::FatEngineError::Cancelled
            } else {
                crate::fat::FatEngineError::TimedOut
            };
            let result = FatResult::Error(error);
            observer.on_complete(&result);
            return result;
        }
        let step = engine.advance(completion);
        if engine.list_output_sequence() > listed {
            observer.on_list_entry(&engine.workspace().entry);
            listed = engine.list_output_sequence();
        }
        let delivered = engine.stream_bytes_delivered();
        if delivered > streamed {
            let chunk_len = usize::from(engine.stream_chunk_len());
            observer.on_stream_chunk(streamed, &engine.workspace().sector[..chunk_len]);
            streamed = delivered;
        }
        match step {
            FatStep::Io(action) => {
                transitions = 0;
                observer.on_stage(engine.stage_label(), true);
                completion = execute_action(action, transport, engine, buffers).await;
                if let FatIoCompletion::Failed(error) = completion {
                    observer.on_transport_error(engine.stage_label(), action, error);
                    if error.requires_bus_recovery() {
                        transport.recover_after_timeout().await;
                    }
                } else if let FatIoCompletion::TimedOut(error) = completion {
                    observer.on_transport_error(engine.stage_label(), action, error);
                    transport.recover_after_timeout().await;
                }
                observer.on_stage(engine.stage_label(), false);
                // SD's bounded CPU/FIFO frames complete synchronously inside
                // the transport future. A short timer handoff removes this
                // FAT task from the ready queue so serial/control work cannot
                // be starved during long streams or range batches.
                yield_after_io().await;
                slice_started = Instant::now();
            }
            FatStep::Continue => {
                completion = FatIoCompletion::Pending;
                transitions = transitions.saturating_add(1);
                if transitions >= 8
                    || Instant::now()
                        .saturating_duration_since(slice_started)
                        .as_micros()
                        >= 1_000
                {
                    transitions = 0;
                    yield_once().await;
                    slice_started = Instant::now();
                }
            }
            FatStep::Yield => {
                completion = FatIoCompletion::Pending;
                transitions = 0;
                yield_once().await;
                slice_started = Instant::now();
            }
            FatStep::Complete(result) => {
                if matches!(
                    result,
                    FatResult::Error(ref error) if error.is_transport_failure()
                ) {
                    engine.invalidate();
                }
                observer.on_complete(&result);
                return result;
            }
        }
    }
}
