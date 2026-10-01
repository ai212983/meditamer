use core::fmt::Write;

use sdcard::fat::{FatEngine, FatRequest, FatResult};
use sdcard::service::{FatBuffers, FatObserver};

use super::super::super::{
    observability,
    types::{SdCommand, SdProbeDriver, SdResultCode},
};
use super::serial_log::{self, SdSerialLine};
mod io;
mod reporting;

use io::{stage_after_tag, stage_before_tag};
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
    let mut output = [0u8; 96];
    let Some(request) = command.fat_request(output.len() as u32) else {
        return (SdResultCode::OperationFailed, false);
    };
    let result =
        run_fat_request(request, probe, engine, command.input_payload(), &mut output).await;
    publish_result(command, result, &output)
}

pub(super) async fn run_fat_request(
    request: FatRequest,
    probe: &mut SdProbeDriver,
    engine: &mut FatEngine,
    input: &[u8],
    output: &mut [u8],
) -> FatResult {
    sdcard::service::run_fat_request(
        request,
        probe,
        engine,
        &mut FatBuffers { input, output },
        &mut ProductObserver,
    )
    .await
}

pub(super) struct ProductObserver;
impl FatObserver for ProductObserver {
    fn on_list_entry(&mut self, entry: &sdcard::fat::FatDirEntry) {
        publish_list_entry(entry);
    }
    fn on_stage(&mut self, stage: sdcard::fat::FatStageLabel, before: bool) {
        observability::log_stack_headroom(if before {
            stage_before_tag(stage)
        } else {
            stage_after_tag(stage)
        });
    }
    fn on_transport_error(
        &mut self,
        stage: sdcard::fat::FatStageLabel,
        action: sdcard::fat::FatIoAction,
        error: sdcard::transport::TransportError,
    ) {
        queue_sd_line!(
            "sdfat[request]: transport_error stage={:?} action={:?} err={:?}",
            stage,
            action,
            error
        );
    }
    fn on_complete(&mut self, _: &FatResult) {
        observability::log_stack_headroom("sd_fat_complete");
    }
}
