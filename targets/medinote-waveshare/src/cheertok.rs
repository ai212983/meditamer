//! CheerTok BLE input composition for Medinote on ESP32-S3.
//!
//! `platform/connectivity/ble` owns the radio/GATT/HID session. This target owns the BT
//! peripheral and Embassy task. Medinote owns the Page Up/Page Down product
//! mapping; the shared [`InputPublisher`] remains the single bounded edge
//! queue between the BLE callback and the UI owner.

use core::cell::RefCell;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::signal::Signal;
#[cfg(feature = "wifi-storage")]
use embassy_time::{with_timeout, Duration};
use heapless::Vec;

use ble::hid::DecodedReport;
use ble::input::InputPublisher;
use medinote::controls::{self, RotationInput};
use medinote::ui::overlay::ble_status::BleStatus;

const TARGET_DEVICE_NAME: &str = "CheerTok";
const SCAN_TIMEOUT_SECONDS: u64 = 15;
const CONNECT_TIMEOUT_SECONDS: u64 = 15;

struct RuntimeState {
    publisher: InputPublisher,
    ui: BleStatus,
    edge_queue_high_water: usize,
    edge_overflow_total: u32,
}

impl RuntimeState {
    const fn new() -> Self {
        Self {
            publisher: InputPublisher::new(),
            ui: BleStatus::Active,
            edge_queue_high_water: 0,
            edge_overflow_total: 0,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct InputQueueMetrics {
    pub(crate) pending: usize,
    pub(crate) high_water: usize,
    pub(crate) overflow_total: u32,
}

static RUNTIME: Mutex<CriticalSectionRawMutex, RefCell<RuntimeState>> =
    Mutex::new(RefCell::new(RuntimeState::new()));
static SINK: CheerTokSink = CheerTokSink;

static STARTUP_COMPLETE: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static STOP_REQUESTED: AtomicBool = AtomicBool::new(false);
static SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);
static SESSION_CLOSED: AtomicBool = AtomicBool::new(true);
static RESUME: Signal<CriticalSectionRawMutex, ()> = Signal::new();

pub(crate) async fn wait_for_startup() {
    STARTUP_COMPLETE.wait().await;
}

fn startup_complete() {
    console::println!("CHEERTOK_INPUT state=scan_ready at_us={}", now_micros());
    STARTUP_COMPLETE.signal(());
}

/// Ask the current BLE session to stop at a safe async boundary. The caller
/// must wait for the acknowledgement before handing the radio to Wi-Fi.
#[cfg(feature = "wifi-storage")]
pub(crate) async fn suspend() -> bool {
    STOP_REQUESTED.store(true, Ordering::Release);
    with_timeout(Duration::from_secs(3), async {
        while SESSION_ACTIVE.load(Ordering::Acquire) {
            embassy_time::Timer::after_millis(20).await;
        }
        SESSION_CLOSED.load(Ordering::Acquire)
    })
    .await
    .unwrap_or(false)
}

/// Clear a previous stop request. A subsequent session start is owned by the
/// target task; this function does not steal or recreate the BT peripheral.
#[cfg(feature = "wifi-storage")]
pub(crate) fn resume() {
    STOP_REQUESTED.store(false, Ordering::Release);
    RESUME.signal(());
}

fn stop_requested() -> bool {
    STOP_REQUESTED.load(Ordering::Acquire)
}

fn now_micros() -> u64 {
    esp_hal::time::Instant::now()
        .duration_since_epoch()
        .as_micros()
}

struct CheerTokSink;

impl ble::HidInputSink for CheerTokSink {
    fn input_ready(&self, now_us: u64) {
        RUNTIME.lock(|state| {
            let mut state = state.borrow_mut();
            state.publisher.clear_and_reconcile(now_us);
            state.ui = BleStatus::Ready;
        });
        let heap = esp_alloc::HEAP.stats();
        console::println!(
            "CHEERTOK_INPUT state=ready at_us={} heap_current={} heap_max={} heap_free={}",
            now_us,
            heap.current_usage,
            heap.max_usage,
            esp_alloc::HEAP.free(),
        );
    }

    fn report(&self, report_id: u8, now_us: u64, bytes: &[u8]) {
        // Physical capture identified Report ID 1 as the keyboard report.
        // Mouse/touch/pen reports remain subscribed for device correctness
        // but do not map to Medinote controls.
        if report_id != 1 {
            return;
        }
        let Ok(buttons) = controls::cheertok_keyboard_buttons(bytes) else {
            RUNTIME.lock(|state| state.borrow_mut().publisher.clear_and_reconcile(now_us));
            console::println!(
                "CHEERTOK_INPUT state=decode_failure report_id={} len={}",
                report_id,
                bytes.len()
            );
            return;
        };
        console::println!(
            "CHEERTOK_INPUT state=keyboard_report at_us={} buttons={:#06b}",
            now_us,
            buttons,
        );
        let decoded = DecodedReport {
            buttons,
            hat: None,
            axes: [None; ble::capacity::GAMEPAD_AXES_MAX],
        };
        let overflowed = RUNTIME.lock(|state| {
            let mut state = state.borrow_mut();
            let generation = state.publisher.generation();
            let _ = state.publisher.publish(generation, now_us, &decoded);
            let pending = state.publisher.pending_edges().count();
            state.edge_queue_high_water = state.edge_queue_high_water.max(pending);
            let overflow_count = state.publisher.overflow_count();
            if overflow_count == 0 {
                false
            } else {
                state.edge_overflow_total =
                    state.edge_overflow_total.saturating_add(overflow_count);
                state.publisher.clear_and_reconcile(now_us);
                true
            }
        });
        if overflowed {
            console::println!(
                "CHEERTOK_INPUT state=edge_overflow_reconciled at_us={}",
                now_us
            );
        }
    }

    fn input_closed(&self, now_us: u64) {
        RUNTIME.lock(|state| {
            let mut state = state.borrow_mut();
            state.publisher.clear_and_reconcile(now_us);
            state.ui = BleStatus::Stopped;
        });
        console::println!("CHEERTOK_INPUT state=closed at_us={}", now_us);
    }
}

pub(crate) fn ui_state() -> BleStatus {
    RUNTIME.lock(|state| state.borrow().ui)
}

pub(crate) fn input_queue_metrics() -> InputQueueMetrics {
    RUNTIME.lock(|state| {
        let state = state.borrow();
        InputQueueMetrics {
            pending: state.publisher.pending_edges().count(),
            high_water: state.edge_queue_high_water,
            overflow_total: state.edge_overflow_total,
        }
    })
}

/// Drain the shared publisher's one ordered edge queue into product inputs.
/// Unknown buttons and all releases are still drained; product policy decides
/// which of them become step commands.
pub fn drain_rotation_inputs() -> Vec<RotationInput, { ble::capacity::EDGE_QUEUE_MAX }> {
    let edges = RUNTIME.lock(|state| state.borrow_mut().publisher.drain_edges());
    let mut inputs = Vec::new();
    for edge in edges {
        if let Some(input) =
            controls::rotation_input_from_button(edge.button, edge.pressed, edge.ticks)
        {
            let _ = inputs.push(input);
        }
    }
    inputs
}

#[inline(never)]
fn report_storage() -> &'static mut ble::LiveConnectReport {
    // Outlining keeps the large initializer out of this task's async frame.
    static REPORT: static_cell::StaticCell<ble::LiveConnectReport> = static_cell::StaticCell::new();
    REPORT.init_with(ble::LiveConnectReport::new)
}

#[inline(never)]
fn resources_storage() -> &'static mut ble::LiveConnectResources {
    // Retire this initializer before polling the session's deep BLE frames.
    // The task owns this single borrow across all sequential controller epochs.
    static RESOURCES: static_cell::StaticCell<ble::LiveConnectResources> =
        static_cell::StaticCell::new();
    RESOURCES.init_with(ble::LiveConnectResources::new)
}

#[inline(never)]
fn poll_live_session<F: core::future::Future>(
    session: core::pin::Pin<&mut F>,
    context: &mut core::task::Context<'_>,
) -> core::task::Poll<F::Output> {
    session.poll(context)
}

/// Reusable CheerTok BLE session owner for the target's single radio task.
///
/// Retains the original BT token across sequential controller epochs; every
/// session reborrows it instead of reacquiring the peripheral. The bond
/// table, static report/resources, startup signal, stop-aware closure, claim
/// publication, UI state, and terminal pending on ambiguous closure all keep
/// the original `input_task` semantics; only the trailing resume wait moved
/// to the caller so the radio supervisor can race it against Wi-Fi start.
pub(crate) struct CheerTokService {
    bluetooth: esp_hal::peripherals::BT<'static>,
    bond_table: ble::bond::BondTable,
    report: &'static mut ble::LiveConnectReport,
    resources: &'static mut ble::LiveConnectResources,
}

impl CheerTokService {
    pub(crate) fn initialize(bluetooth: esp_hal::peripherals::BT<'static>) -> &'static mut Self {
        static SERVICE: static_cell::StaticCell<CheerTokService> = static_cell::StaticCell::new();
        SERVICE.init(Self {
            bluetooth,
            bond_table: ble::bond::BondTable::new(),
            report: report_storage(),
            resources: resources_storage(),
        })
    }

    /// Run one sequential BLE session on the retained token.
    ///
    /// A cooperatively stopped session reports a safe close; an ambiguous
    /// closure still pends terminally so no other radio owner takes over.
    pub(crate) async fn run_one_session(&mut self) {
        // The one-time initializers run only when the service is constructed.
        // Each epoch reborrows that retained storage after confirmed teardown.
        let report = &mut *self.report;
        let resources = &mut *self.resources;
        if !STOP_REQUESTED.load(Ordering::Acquire) {
            SESSION_ACTIVE.store(true, Ordering::Release);
            SESSION_CLOSED.store(false, Ordering::Release);
            arbitration::claim::set_ble_ownership(arbitration::claim::Ownership::Active);
            RUNTIME.lock(|state| state.borrow_mut().ui = BleStatus::Active);
            console::println!(
                "CHEERTOK_INPUT state=starting target={}",
                TARGET_DEVICE_NAME
            );
            let closed = {
                let mut session = core::pin::pin!(ble::run_live_connect_with_resources(
                    self.bluetooth.reborrow(),
                    ble::LiveConnectOptions {
                        target_name: TARGET_DEVICE_NAME,
                        scan_timeout: embassy_time::Duration::from_secs(SCAN_TIMEOUT_SECONDS),
                        connect_timeout: embassy_time::Duration::from_secs(CONNECT_TIMEOUT_SECONDS),
                        now_ticks: now_micros,
                        startup_complete: Some(startup_complete),
                        request_pairing: true,
                        // This legacy-pairing peer requires encryption before the first
                        // GATT client operation (ADR-0016).
                        pair_before_discovery: true,
                        hid_capture_duration: None,
                        hid_input_sink: Some(&SINK),
                        stop_requested: Some(stop_requested),
                    },
                    &mut self.bond_table,
                    report,
                    resources,
                ));
                core::future::poll_fn(|cx| poll_live_session(session.as_mut(), cx)).await
            };
            SESSION_CLOSED.store(closed, Ordering::Release);
            arbitration::claim::set_ble_ownership(if closed {
                arbitration::claim::Ownership::KnownClosed
            } else {
                arbitration::claim::Ownership::Unknown
            });
            SESSION_ACTIVE.store(false, Ordering::Release);
            console::println!(
                "CHEERTOK_INPUT state=stopped outcome={:?} closed={} services={} reports={} subscriptions={}",
                report.outcome, closed, report.services.len(), report.hid_reports.len(), report.hid_subscriptions,
            );
            RUNTIME.lock(|state| state.borrow_mut().ui = BleStatus::Stopped);
            // Failed initialization/scan must release the UI's sole startup wait.
            STARTUP_COMPLETE.signal(());
            if !closed {
                // Ambiguous closure cannot be repaired by starting a new epoch.
                core::future::pending::<()>().await;
            }
        } else {
            arbitration::claim::set_ble_ownership(arbitration::claim::Ownership::KnownClosed);
            STARTUP_COMPLETE.signal(());
        }
    }

    /// Wait until the owner asks for the next session.
    pub(crate) async fn wait_for_resume(&self) {
        RESUME.wait().await;
    }
}

/// BLE-only composition: sequential sessions with the original
/// park-between-epochs behavior.
///
/// This task macro must stay out of the combined Wi-Fi + BLE image, where the
/// single radio supervisor owns the BT token instead; otherwise its static
/// task pool remains in the ELF.
#[cfg(not(feature = "wifi-storage"))]
#[embassy_executor::task]
pub async fn input_task(device: esp_hal::peripherals::BT<'static>) {
    let service = CheerTokService::initialize(device);
    loop {
        service.run_one_session().await;
        service.wait_for_resume().await;
    }
}
