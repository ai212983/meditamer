//! Exclusive target radio policy: acknowledged BLE close before Wi-Fi starts,
//! acknowledged network teardown before BLE resumes or the target sleeps.
use arbitration::handoff::{
    NetworkOwnerAck, NetworkOwnerAckKind, NetworkOwnerCommand, NetworkOwnerState,
};
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex, signal::Signal};
use embassy_time::{with_timeout, Duration};
use netstack::host::{NetHost, ProductState};

static ENABLED: AtomicBool = AtomicBool::new(false);
static RESTORE_AFTER_SLEEP: AtomicBool = AtomicBool::new(false);
static RESUME: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static START: Signal<CriticalSectionRawMutex, ()> = Signal::new();
#[cfg(feature = "cheertok-controls")]
static START_REQUESTED: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<CriticalSectionRawMutex, State> = Mutex::new(State {
    started: false,
    off: true,
    boot: 0,
    epoch: 0,
    restore_after_sleep: false,
    uncertain: false,
});
struct State {
    started: bool,
    off: bool,
    boot: u32,
    epoch: u32,
    restore_after_sleep: bool,
    uncertain: bool,
}
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}
pub fn desired_for_sleep() -> bool {
    RESTORE_AFTER_SLEEP.load(Ordering::Acquire)
}

pub struct Host;
impl NetHost for Host {
    async fn serve(&self, stack: embassy_net::Stack<'_>) {
        crate::net_http::serve(stack).await
    }
    async fn abort_service_work(&self) -> bool {
        crate::storage::abort_upload_barrier().await
    }
}
static PRODUCT_STATE: ProductState = ProductState {
    active_http_connections: crate::net_http::active_connections,
    active_sd_roundtrips: crate::storage::active_roundtrips,
    upload_session_active: crate::storage::upload_session_active,
    transport_quiet: || false,
    upload_enabled: is_enabled,
    allocator_memory_snapshot: crate::network_memory::snapshot,
    probe_internal_block_above_reserve: crate::network_memory::probe,
};

/// Wi-Fi-only composition. The combined Wi-Fi + BLE image runs
/// [`radio_supervisor_task`] instead; compiling this task macro there as well
/// would keep its static task pool in the ELF beside the supervisor's.
#[cfg(not(feature = "cheertok-controls"))]
#[embassy_executor::task]
pub async fn run(wifi: esp_hal::peripherals::WIFI<'static>) {
    // BLE owns the initial product mode. Merely spawning this task must not
    // initialize a second radio stack beside it.
    START.wait().await;
    netstack::host::install(&PRODUCT_STATE);
    if !crate::net_http::token_configured() {
        netstack::host::set_listener_enabled(false);
    }
    netstack::run_network_owner(&Host, wifi, netstack::stack_resources()).await;
}

async fn handoff(command: NetworkOwnerCommand) -> Option<NetworkOwnerAck> {
    let ticket = netstack::request_handoff(command).ok()?;
    let seconds = match command {
        NetworkOwnerCommand::Status => 3,
        NetworkOwnerCommand::AcquireExclusive { .. } => 30,
        NetworkOwnerCommand::ReleaseExclusive { .. } => 100,
    };
    match with_timeout(
        Duration::from_secs(seconds),
        netstack::receive_handoff_ack(ticket),
    )
    .await
    {
        Ok(ack) => Some(ack),
        Err(_) => {
            netstack::cancel_handoff_request(ticket);
            None
        }
    }
}

async fn close_ble() -> bool {
    #[cfg(feature = "cheertok-controls")]
    {
        crate::cheertok::suspend().await
    }
    #[cfg(not(feature = "cheertok-controls"))]
    {
        arbitration::claim::set_ble_ownership(arbitration::claim::Ownership::KnownClosed);
        true
    }
}
fn resume_ble() {
    #[cfg(feature = "cheertok-controls")]
    crate::cheertok::resume();
}

async fn transition(state: &mut State, enabled: bool) -> bool {
    if state.uncertain {
        // Cancelling our reply wait does not cancel the owner's operation.
        // Reconcile a terminal owner state before trusting local fast paths.
        let Some(status) = handoff(NetworkOwnerCommand::Status).await else {
            return false;
        };
        match status.state {
            NetworkOwnerState::OffConfirmed => {
                state.off = true;
                state.boot = status.boot_generation;
                state.epoch = state.epoch.max(status.epoch);
                ENABLED.store(false, Ordering::Release);
            }
            NetworkOwnerState::Serving => state.off = false,
            _ => return false,
        }
        state.uncertain = false;
    }
    if enabled {
        if !state.off {
            ENABLED.store(true, Ordering::Release);
            return true;
        }
        if !close_ble().await {
            return false;
        }
        ENABLED.store(true, Ordering::Release);
        if !state.started {
            state.started = true;
            state.off = false;
            #[cfg(feature = "cheertok-controls")]
            START_REQUESTED.store(true, Ordering::Release);
            START.signal(());
            // A started owner retains radio ownership even if association
            // fails. A later STOP must still perform the teardown handshake.
            return true;
        }
        state.off = false; // A timed-out restore may already have reinitialized Wi-Fi.
        let result = handoff(NetworkOwnerCommand::ReleaseExclusive {
            boot_generation: state.boot,
            epoch: state.epoch,
        })
        .await;
        state.uncertain = result.is_none();
        match result {
            Some(ack) if ack.kind == NetworkOwnerAckKind::Restored => {
                state.off = false;
                true
            }
            _ => {
                state.uncertain = true;
                ENABLED.store(false, Ordering::Release);
                false
            }
        }
    } else {
        if state.off {
            ENABLED.store(false, Ordering::Release);
            return true;
        }
        let Some(status) = handoff(NetworkOwnerCommand::Status).await else {
            return false;
        };
        // Serving status clears its active epoch after restoration. Retain our
        // last attempted epoch so every acquire remains newer, including after
        // a rejected attempt or a completed BLE/Wi-Fi cycle.
        let Some(epoch) = state.epoch.max(status.epoch).checked_add(1) else {
            return false;
        };
        state.epoch = epoch;
        let result = handoff(NetworkOwnerCommand::AcquireExclusive {
            boot_generation: status.boot_generation,
            epoch,
        })
        .await;
        state.uncertain = result.is_none();
        match result {
            Some(ack) if ack.kind == NetworkOwnerAckKind::Quiesced => {
                state.boot = ack.boot_generation;
                state.epoch = ack.epoch;
                state.off = true;
                ENABLED.store(false, Ordering::Release);
                true
            }
            _ => {
                state.uncertain = true;
                false
            }
        }
    }
}

pub async fn set_enabled(enabled: bool) -> bool {
    if enabled
        && netstack::wifi::current_runtime_config()
            .credentials
            .is_none()
    {
        console::println!("NET_POLICY enabled=true accepted=false reason=unprovisioned");
        return false;
    }
    let mut state = STATE.lock().await;
    let ok = transition(&mut state, enabled).await;
    if ok && !enabled {
        resume_ble();
    }
    console::println!(
        "NET_POLICY enabled={} accepted={} radio_off={}",
        enabled,
        ok,
        state.off
    );
    ok
}

pub async fn suspend() -> bool {
    let mut state = STATE.lock().await;
    state.restore_after_sleep = is_enabled();
    RESTORE_AFTER_SLEEP.store(state.restore_after_sleep, Ordering::Release);
    if !transition(&mut state, false).await {
        return false;
    }
    close_ble().await
}

pub fn resume() {
    RESUME.signal(());
}

/// Wait for the sleep-resume request. Serviced by the network command
/// worker, which keeps its console-command duty in the same task.
pub(crate) async fn wait_for_sleep_resume() {
    RESUME.wait().await;
}

/// Handle one sleep-resume request: restore Wi-Fi when sleep retained it,
/// otherwise resume the BLE session. Formerly the dedicated `resume_task`;
/// folding it into the command worker removes a task pool without dropping
/// console commands.
pub(crate) async fn handle_sleep_resume() {
    let mut state = STATE.lock().await;
    if state.restore_after_sleep {
        if !transition(&mut state, true).await {
            console::println!("NET_POLICY error=resume_failed");
        }
    } else {
        resume_ble();
    }
}

/// Whether the one-shot Wi-Fi start was requested (first `NET START` or the
/// retained sleep-restore). Read by the BLE-first supervisor loop.
#[cfg(feature = "cheertok-controls")]
pub(crate) fn start_requested() -> bool {
    START_REQUESTED.load(Ordering::Acquire)
}

/// Wait for the one-shot Wi-Fi start. Raced against the BLE resume signal
/// between sessions so the supervisor notices `NET START` while parked.
#[cfg(feature = "cheertok-controls")]
pub(crate) async fn wait_for_start() {
    START.wait().await;
}

/// The product side of the radio supervisor's off state: wait for the resume
/// permit the stop path published, then run one CheerTok session on the
/// retained token. A retained permit makes the wait return immediately when
/// the resume already happened before the supervisor entered OffConfirmed.
#[cfg(feature = "cheertok-controls")]
struct CheerTokOffService {
    cheer: &'static mut crate::cheertok::CheerTokService,
    initial: bool,
}

#[cfg(feature = "cheertok-controls")]
impl netstack::RadioOffService for CheerTokOffService {
    async fn run(&mut self, _boot_generation: u32, _epoch: u32) {
        let initial = self.initial;
        self.initial = false;
        if !initial {
            self.cheer.wait_for_resume().await;
        }
        loop {
            self.cheer.run_one_session().await;
            if !initial || start_requested() {
                return;
            }
            embassy_futures::select::select(self.cheer.wait_for_resume(), wait_for_start()).await;
            if start_requested() {
                return;
            }
        }
    }
}

/// The one permanent radio task for the combined Wi-Fi + BLE image.
///
/// BLE owns the initial product mode: sessions run first on the retained
/// token, and this same task starts `netstack::run_radio_supervisor` only
/// after a start request safely stopped them. During Wi-Fi OffConfirmed the
/// supervisor drives the retained CheerTok session through the off service.
/// Wi-Fi and BLE futures are awaited in different states, so Embassy reuses
/// this one task pool for both epochs rather than reserving two.
#[cfg(feature = "cheertok-controls")]
#[embassy_executor::task]
pub async fn radio_supervisor_task(
    wifi_peripheral: esp_hal::peripherals::WIFI<'static>,
    bluetooth: esp_hal::peripherals::BT<'static>,
    resources: &'static mut embassy_net::StackResources<{ netstack::NET_STACK_SOCKETS }>,
) {
    netstack::host::install(&PRODUCT_STATE);
    if !crate::net_http::token_configured() {
        netstack::host::set_listener_enabled(false);
    }
    let off = CheerTokOffService {
        cheer: crate::cheertok::CheerTokService::initialize(bluetooth),
        initial: true,
    };
    netstack::run_radio_supervisor_initially_off(&Host, wifi_peripheral, resources, off).await;
}
