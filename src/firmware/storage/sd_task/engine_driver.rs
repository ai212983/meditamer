use core::fmt::Write;

use embassy_futures::yield_now;
use embassy_time::Instant;
use sdcard::fat::{
    FatEngine, FatEngineError, FatIoCompletion, FatPayloadId, FatRequest, FatResult, FatStep,
};

use super::super::super::{
    telemetry,
    types::{SdCommand, SdProbeDriver, SdResultCode},
};
use super::serial_log::{self, SdSerialLine};
mod io;
mod reporting;

use io::{execute_action, stage_after_tag, stage_before_tag};
use reporting::{publish_list_entry, publish_result};

macro_rules! queue_sd_line {
    ($($arg:tt)*) => {{
        let mut line = SdSerialLine::new();
        let _ = write!(&mut line, $($arg)*);
        let _ = line.push_str("\r\n");
        let _ = serial_log::send(line);
    }};
}

pub(super) async fn run_fat_engine_command(
    command: SdCommand,
    probe: &mut SdProbeDriver,
    engine: &mut FatEngine,
) -> (SdResultCode, bool) {
    let mut input = [0u8; sdcard::SD_WRITE_MAX];
    let mut output = [0u8; 96];
    let request = match command {
        SdCommand::FatList { path, path_len } => FatRequest::List { path, path_len },
        SdCommand::FatRead { path, path_len } => FatRequest::Read {
            path,
            path_len,
            output: FatPayloadId::Primary,
            output_capacity: output.len() as u32,
        },
        SdCommand::FatWrite {
            path,
            path_len,
            data,
            data_len,
        } => {
            let len = usize::from(data_len).min(data.len());
            input[..len].copy_from_slice(&data[..len]);
            FatRequest::Write {
                path,
                path_len,
                input: FatPayloadId::Primary,
                input_len: len as u32,
            }
        }
        SdCommand::FatStat { path, path_len } => FatRequest::Stat { path, path_len },
        SdCommand::FatMkdir { path, path_len } => FatRequest::Mkdir { path, path_len },
        SdCommand::FatRemove { path, path_len } => FatRequest::Remove { path, path_len },
        SdCommand::FatRename {
            src_path,
            src_path_len,
            dst_path,
            dst_path_len,
        } => FatRequest::Rename {
            src_path,
            src_path_len,
            dst_path,
            dst_path_len,
            replace: false,
        },
        SdCommand::FatAppend {
            path,
            path_len,
            data,
            data_len,
        } => {
            let len = usize::from(data_len).min(data.len());
            input[..len].copy_from_slice(&data[..len]);
            FatRequest::Append {
                path,
                path_len,
                input: FatPayloadId::Primary,
                input_len: len as u32,
            }
        }
        SdCommand::FatTruncate {
            path,
            path_len,
            size,
        } => FatRequest::Truncate {
            path,
            path_len,
            size,
        },
        SdCommand::Probe | SdCommand::RwVerify { .. } => {
            return (SdResultCode::OperationFailed, false);
        }
    };

    let result = run_fat_request(request, probe, engine, &input, &mut output).await;
    publish_result(command, result, &output)
}

pub(super) async fn run_fat_request(
    request: FatRequest,
    probe: &mut SdProbeDriver,
    engine: &mut FatEngine,
    input: &[u8],
    output: &mut [u8],
) -> FatResult {
    if let Err(err) = engine.start(request) {
        return FatResult::Error(err);
    }

    let mut completion = FatIoCompletion::Pending;
    let mut listed = 0usize;
    let mut cpu_transitions = 0u8;
    let mut cpu_slice_started = Instant::now();
    loop {
        let step = engine.advance(completion);
        if engine.list_output_sequence() > listed {
            publish_list_entry(&engine.workspace().entry);
            listed = engine.list_output_sequence();
        }
        match step {
            FatStep::Io(action) => {
                cpu_transitions = 0;
                telemetry::log_stack_headroom(stage_before_tag(engine.stage_label()));
                completion = execute_action(action, probe, engine, input, output).await;
                if matches!(completion, FatIoCompletion::TimedOut) {
                    queue_sd_line!(
                        "sdfat[request]: io_timeout stage={:?} action={:?}",
                        engine.stage_label(),
                        action
                    );
                    probe.recover_after_timeout();
                } else if matches!(
                    &completion,
                    FatIoCompletion::Failed(err) if err.requires_bus_recovery()
                ) {
                    probe.recover_after_timeout();
                }
                telemetry::log_stack_headroom(stage_after_tag(engine.stage_label()));
                // A completed DMA future resumes inside the current executor poll.
                // Yield before advancing the engine or arming another transfer so
                // higher-priority input work can run at every I/O boundary.
                yield_now().await;
                cpu_slice_started = Instant::now();
            }
            FatStep::Continue => {
                completion = FatIoCompletion::Pending;
                cpu_transitions = cpu_transitions.saturating_add(1);
                if cpu_transitions >= 8
                    || Instant::now()
                        .saturating_duration_since(cpu_slice_started)
                        .as_micros()
                        >= 1_000
                {
                    cpu_transitions = 0;
                    yield_now().await;
                    cpu_slice_started = Instant::now();
                }
            }
            FatStep::Yield => {
                completion = FatIoCompletion::Pending;
                cpu_transitions = 0;
                yield_now().await;
                cpu_slice_started = Instant::now();
            }
            FatStep::Complete(result) => {
                telemetry::log_stack_headroom("sd_fat_complete");
                if matches!(result, FatResult::Error(FatEngineError::TimedOut)) {
                    engine.invalidate();
                }
                return result;
            }
        }
    }
}
