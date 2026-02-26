#[cfg(feature = "graphics")]
use crate::firmware::assets::runtime::clear_runtime_asset_caches;
use crate::firmware::{
    runtime::service_mode,
    storage::transfer_buffers,
    types::{DisplayContext, RuntimeServices, RuntimeServicesUpdate},
};

pub(super) async fn apply_runtime_services_update(
    context: &mut DisplayContext,
    update: RuntimeServicesUpdate,
) -> RuntimeServices {
    let previous = service_mode::runtime_services();
    let next = update.apply(previous);
    if next == previous {
        return next;
    }

    service_mode::set_runtime_services(next);
    context.mode_store.save_runtime_services(next);

    if previous.upload_enabled_flag() && !next.upload_enabled_flag() {
        transfer_buffers::release_upload_chunk_buffer().await;
    }
    if previous.asset_reads_enabled_flag() && !next.asset_reads_enabled_flag() {
        transfer_buffers::release_asset_read_buffer().await;
        #[cfg(feature = "graphics")]
        clear_runtime_asset_caches().await;
    }
    if !previous.upload_enabled_flag() && next.upload_enabled_flag() {
        let _ = context.inkplate.frontlight_off();
    }

    esp_println::println!(
        "runtime_mode: upload={} assets={}",
        if next.upload_enabled_flag() {
            "on"
        } else {
            "off"
        },
        if next.asset_reads_enabled_flag() {
            "on"
        } else {
            "off"
        }
    );

    next
}
