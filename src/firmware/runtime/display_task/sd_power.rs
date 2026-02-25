use crate::firmware::{
    config::{SD_POWER_REQUESTS, SD_POWER_RESPONSES},
    types::{DisplayContext, SdPowerRequest},
};

pub(super) async fn process_sd_power_requests(context: &mut DisplayContext) {
    while let Ok(request) = SD_POWER_REQUESTS.try_receive() {
        let ok = match request {
            SdPowerRequest::On => context.inkplate.sd_card_power_on().is_ok(),
            SdPowerRequest::Off => context.inkplate.sd_card_power_off().is_ok(),
        };
        SD_POWER_RESPONSES.send(ok).await;
    }
}
