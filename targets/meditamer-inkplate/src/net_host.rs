//! Meditamer's implementation of the network owner's product surface.
//!
//! ADR-0015 Tier 2: the product half of the inversion described in
//! `netstack::host`. This is the only place tying the network supervisor to
//! Meditamer's upload and firmware-update subsystems; the supervisor itself
//! sees nothing but the `NetHost` bound and the installed `ProductState`
//! accessors.
//!
//! Moved here from `products/meditamer` in the product and target axis
//! completion plan's Phase 3: its only consumer was `system.rs`, which
//! moved the same way, and `#[embassy_executor::task]` cannot be generic --
//! this is the concrete task-spawn binding point, squarely target-owned
//! chip/task wiring even though everything it calls into stays product-side.
//! `NetHost`/`ProductState` themselves moved out to `platform/connectivity/netstack` in
//! Phase 4; this file now implements ports on a platform trait/struct rather
//! than a product one, and converts psram's allocator-specific probe type to
//! netstack's plain-data one, so netstack itself depends on no product
//! allocator.

use embassy_net::Stack;
use meditamer_product::firmware::psram;
use meditamer_product::firmware::{service_mode, storage, update};
use netstack::host::{
    AllocatorMemorySnapshot, AllocatorState, InternalBlockProbe, NetHost, ProductState,
};

#[derive(Clone, Copy, Default)]
pub struct MeditamerNetHost;

impl NetHost for MeditamerNetHost {
    fn serve(&self, stack: Stack<'_>) -> impl core::future::Future<Output = ()> {
        storage::upload::run_http_server(stack)
    }

    fn abort_service_work(&self) -> impl core::future::Future<Output = bool> {
        storage::upload::abort_sd_upload()
    }
}

fn allocator_state(state: psram::AllocatorState) -> AllocatorState {
    match state {
        psram::AllocatorState::Disabled => AllocatorState::Disabled,
        psram::AllocatorState::NotInitialized => AllocatorState::NotInitialized,
        psram::AllocatorState::Initialized => AllocatorState::Initialized,
        psram::AllocatorState::InitFailed => AllocatorState::InitFailed,
    }
}

fn allocator_memory_snapshot() -> AllocatorMemorySnapshot {
    let snapshot = psram::allocator_memory_snapshot();
    AllocatorMemorySnapshot {
        feature_enabled: snapshot.feature_enabled,
        state: allocator_state(snapshot.state),
        total_bytes: snapshot.total_bytes,
        used_bytes: snapshot.used_bytes,
        free_bytes: snapshot.free_bytes,
        peak_used_bytes: snapshot.peak_used_bytes,
        free_internal_bytes: snapshot.free_internal_bytes,
        free_external_bytes: snapshot.free_external_bytes,
        min_free_bytes: snapshot.min_free_bytes,
        min_free_internal_bytes: snapshot.min_free_internal_bytes,
        min_free_external_bytes: snapshot.min_free_external_bytes,
        large_alloc_external_ok: snapshot.large_alloc_external_ok,
        large_alloc_internal_ok: snapshot.large_alloc_internal_ok,
        large_alloc_fail: snapshot.large_alloc_fail,
    }
}

fn probe_internal_block_above_reserve(reserve_bytes: usize) -> InternalBlockProbe {
    let probe = psram::probe_internal_block_above_reserve(reserve_bytes);
    InternalBlockProbe {
        free_before_bytes: probe.free_before_bytes,
        block_bytes: probe.block_bytes,
        reserve_bytes: probe.reserve_bytes,
        free_after_bytes: probe.free_after_bytes,
        stable: probe.stable,
    }
}

static PRODUCT_STATE: ProductState = ProductState {
    active_http_connections: storage::upload::active_http_connections,
    active_sd_roundtrips: storage::upload::active_sd_roundtrips,
    upload_session_active: storage::upload::sd_upload_session_active,
    transport_quiet: update::transport_quiet,
    upload_enabled: service_mode::upload_enabled,
    allocator_memory_snapshot,
    probe_internal_block_above_reserve,
};

/// Publish Meditamer's state accessors to the network supervisor. Must run
/// before [`network_owner_task`] is spawned.
pub fn install() {
    netstack::host::install(&PRODUCT_STATE);
}

/// The embassy task shell for the network supervisor.
///
/// It lives target-side because `run_network_owner` is generic over
/// [`NetHost`] and an `#[embassy_executor::task]` cannot be generic. This is
/// the seam: everything below it is product-neutral.
#[cfg(not(feature = "ble-foundation"))]
#[embassy_executor::task]
pub async fn network_owner_task(
    wifi_peripheral: esp_hal::peripherals::WIFI<'static>,
    resources: &'static mut embassy_net::StackResources<{ netstack::NET_STACK_SOCKETS }>,
) {
    install();
    netstack::run_network_owner(&MeditamerNetHost, wifi_peripheral, resources).await;
}

#[cfg(feature = "ble-foundation")]
struct MeditamerRadioOffService(meditamer_product::firmware::ble::Phase1Service);

#[cfg(feature = "ble-foundation")]
impl netstack::RadioOffService for MeditamerRadioOffService {
    async fn run(&mut self, boot_generation: u32, epoch: u32) {
        self.0.run_next(boot_generation, epoch).await;
    }
}

/// The one permanent radio task for the combined Wi-Fi/BLE image.
///
/// Each Wi-Fi epoch is fully destroyed before the BLE future is created; BLE
/// then returns and drops before Wi-Fi restoration begins. This makes the task
/// pool hold the larger epoch future rather than the sum of two task pools.
#[cfg(feature = "ble-foundation")]
#[embassy_executor::task]
pub async fn radio_supervisor_task(
    wifi_peripheral: esp_hal::peripherals::WIFI<'static>,
    bluetooth: esp_hal::peripherals::BT<'static>,
    resources: &'static mut embassy_net::StackResources<{ netstack::NET_STACK_SOCKETS }>,
) {
    install();
    let ble = MeditamerRadioOffService(meditamer_product::firmware::ble::Phase1Service::new(
        bluetooth,
    ));
    netstack::run_radio_supervisor(&MeditamerNetHost, wifi_peripheral, resources, ble).await;
}
