//! Meditamer's implementation of the network owner's product surface.
//!
//! ADR-0015 Tier 2: the product half of the inversion described in `net::host`.
//! This is the only place tying the network supervisor to Meditamer's upload and
//! firmware-update subsystems; the supervisor itself sees nothing but the
//! `NetHost` bound and the installed `ProductState` accessors.

use embassy_net::Stack;

use super::net::host::{NetHost, ProductState};
use super::{storage, update};

#[derive(Clone, Copy, Default)]
pub(crate) struct MeditamerNetHost;

impl NetHost for MeditamerNetHost {
    fn serve(&self, stack: Stack<'_>) -> impl core::future::Future<Output = ()> {
        storage::upload::run_http_server(stack)
    }

    fn abort_upload(&self) -> impl core::future::Future<Output = bool> {
        storage::upload::abort_sd_upload()
    }
}

static PRODUCT_STATE: ProductState = ProductState {
    active_http_connections: storage::upload::active_http_connections,
    active_sd_roundtrips: storage::upload::active_sd_roundtrips,
    upload_session_active: storage::upload::sd_upload_session_active,
    transport_quiet: update::transport_quiet,
};

/// Publish Meditamer's state accessors to the network supervisor. Must run
/// before [`network_owner_task`] is spawned.
pub(crate) fn install() {
    super::net::host::install(&PRODUCT_STATE);
}

/// The embassy task shell for the network supervisor.
///
/// It lives product-side because `run_network_owner` is generic over
/// [`NetHost`] and an `#[embassy_executor::task]` cannot be generic. This is
/// the seam: everything below it is product-neutral.
#[embassy_executor::task]
pub(crate) async fn network_owner_task(
    wifi_peripheral: esp_hal::peripherals::WIFI<'static>,
    resources: &'static mut embassy_net::StackResources<{ super::net::NET_STACK_SOCKETS }>,
) {
    install();
    super::net::run_network_owner(&MeditamerNetHost, wifi_peripheral, resources).await;
}
