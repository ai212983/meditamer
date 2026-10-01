use embassy_time::{Duration, Instant};
use sdcard::fat::FatEngine;
use sdcard::runtime as sd_ops;

use super::{
    failure_backoff_ms, process_request, publish_result, SdCommand, SdProbeDriver, SdRequest,
};

pub(super) async fn initialize(
    sd_probe: &mut SdProbeDriver,
    powered: &mut bool,
    no_power: &mut impl FnMut(sd_ops::SdPowerAction) -> Result<(), ()>,
    fat_engine: &mut FatEngine,
) -> (u8, Option<Instant>) {
    let boot_req = SdRequest {
        id: 0,
        command: SdCommand::Probe,
    };
    let boot_result = process_request(boot_req, sd_probe, powered, no_power, fat_engine).await;
    publish_result(boot_result);

    if boot_result.ok {
        (0, None)
    } else {
        (
            1,
            Some(Instant::now() + Duration::from_millis(failure_backoff_ms(1))),
        )
    }
}
