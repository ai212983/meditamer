use core::{
    future::poll_fn,
    ptr::{addr_of, NonNull},
    task::Poll,
};

use embassy_time::{with_timeout, Duration};
use esp_phy::PhyInitGuard;
use esp_sync::RawMutex;
use portable_atomic::AtomicU32;
use portable_atomic::{AtomicBool, Ordering};

use super::{
    latch_transport_fault, record_rx_oversize, record_rx_queue_depth, record_rx_queue_overflow,
    record_tx_rejected, record_tx_timeout, reset_transport_state, transport_faulted,
    tx_cancellation::{TxCancellationGuard, TxCancellationLatch},
    Config, HciTransportError, ReceivedPacket, HCI_PACKET_CAPACITY,
};
use crate::{
    asynch::AtomicWaker,
    ble::{
        btdm::ble_os_adapter_chip_specific::{osi_funcs_s, G_OSI_FUNCS},
        HCI_OUT_COLLECTOR,
    },
    compat::common::str_from_c,
    hal::ram,
    sys::{c_types::*, include::*},
};

#[cfg_attr(esp32c3, path = "os_adapter_esp32c3_s3.rs")]
#[cfg_attr(esp32s3, path = "os_adapter_esp32c3_s3.rs")]
#[cfg_attr(esp32, path = "os_adapter_esp32.rs")]
pub(crate) mod ble_os_adapter_chip_specific;

static PACKET_SENT: AtomicBool = AtomicBool::new(true);
static HCI_TX_WAKER: AtomicWaker = AtomicWaker::new();
static HCI_CALLBACK_WAKER: AtomicWaker = AtomicWaker::new();
static CALLBACK_ADMISSION_OPEN: AtomicBool = AtomicBool::new(false);
static CALLBACK_ACCEPTED: AtomicU32 = AtomicU32::new(0);
static CALLBACK_REJECTED: AtomicU32 = AtomicU32::new(0);
static CALLBACK_IN_FLIGHT: AtomicU32 = AtomicU32::new(0);
static CALLBACK_HIGH_WATER: AtomicU32 = AtomicU32::new(0);
static CONTROLLER_CALLBACK_SOURCE_ACTIVE: AtomicBool = AtomicBool::new(false);
const HCI_TX_TIMEOUT: Duration = Duration::from_millis(100);

struct TransportCancellationLatch;

impl TxCancellationLatch for TransportCancellationLatch {
    fn latch() {
        // Dropping the send future can abandon either a complete packet
        // waiting for controller availability or a packet already in flight.
        // Latch before the collector mutex unlocks; only full controller
        // teardown/reinitialization may clear the fault.
        latch_transport_fault();
    }
}

struct HciCallbackGuard {
    admitted: bool,
}

impl HciCallbackGuard {
    fn enter() -> Self {
        let in_flight = CALLBACK_IN_FLIGHT.fetch_add(1, Ordering::AcqRel) + 1;
        let mut high_water = CALLBACK_HIGH_WATER.load(Ordering::Relaxed);
        while in_flight > high_water {
            match CALLBACK_HIGH_WATER.compare_exchange_weak(
                high_water,
                in_flight,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(observed) => high_water = observed,
            }
        }

        let admitted = CALLBACK_ADMISSION_OPEN.load(Ordering::Acquire);
        if admitted {
            CALLBACK_ACCEPTED.fetch_add(1, Ordering::Relaxed);
        } else {
            CALLBACK_REJECTED.fetch_add(1, Ordering::Relaxed);
        }
        Self { admitted }
    }
}

impl Drop for HciCallbackGuard {
    fn drop(&mut self) {
        if CALLBACK_IN_FLIGHT.fetch_sub(1, Ordering::AcqRel) == 1 {
            HCI_CALLBACK_WAKER.wake();
        }
    }
}

fn start_hci_callback_epoch() {
    assert_eq!(
        CALLBACK_IN_FLIGHT.load(Ordering::Acquire),
        0,
        "HCI callback still in flight during BLE initialization"
    );
    CALLBACK_ACCEPTED.store(0, Ordering::Relaxed);
    CALLBACK_REJECTED.store(0, Ordering::Relaxed);
    CALLBACK_HIGH_WATER.store(0, Ordering::Relaxed);
    CALLBACK_ADMISSION_OPEN.store(true, Ordering::Release);
}

/// Revoke callback admission and synchronously disable the controller callback source.
///
/// Callers must then cancel host futures and await [`wait_for_hci_callback_quiescence`]
/// before dropping storage reachable from an admitted callback.
#[instability::unstable]
pub fn begin_hci_callback_shutdown() {
    CALLBACK_ADMISSION_OPEN.store(false, Ordering::Release);
    if CONTROLLER_CALLBACK_SOURCE_ACTIVE.swap(false, Ordering::AcqRel) {
        unsafe extern "C" {
            fn btdm_controller_disable();
        }
        unsafe {
            btdm_controller_disable();
        }
    }
}

/// Wait until all controller callbacks, including rejected callback stubs, have returned.
#[instability::unstable]
pub async fn wait_for_hci_callback_quiescence(timeout: Duration) -> bool {
    let quiescent = poll_fn(|cx| {
        HCI_CALLBACK_WAKER.register(cx.waker());
        if CALLBACK_IN_FLIGHT.load(Ordering::Acquire) == 0 {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    });
    with_timeout(timeout, quiescent).await.is_ok()
}

/// Return callback admission and in-flight counters for the active controller epoch.
#[instability::unstable]
pub fn hci_callback_stats() -> super::HciCallbackStats {
    super::HciCallbackStats {
        admission_open: CALLBACK_ADMISSION_OPEN.load(Ordering::Acquire),
        accepted: CALLBACK_ACCEPTED.load(Ordering::Relaxed),
        rejected: CALLBACK_REJECTED.load(Ordering::Relaxed),
        in_flight: CALLBACK_IN_FLIGHT.load(Ordering::Acquire),
        high_water: CALLBACK_HIGH_WATER.load(Ordering::Relaxed),
    }
}

#[repr(C)]
struct VhciHostCallbacks {
    // callback used to notify that the host can
    // send packet to controller
    notify_host_send_available: extern "C" fn(),
    // callback used to notify that the
    // controller has a packet to send to
    // the host
    notify_host_recv: extern "C" fn(*mut u8, u16) -> i32,
}

unsafe extern "C" {
    fn btdm_osi_funcs_register(osi_funcs: *const osi_funcs_s) -> i32;
    fn btdm_controller_get_compile_version() -> *const c_char;

    #[cfg(any(esp32c3, esp32s3))]
    fn btdm_controller_init(config_opts: *const esp_bt_controller_config_t) -> i32;

    #[cfg(esp32)]
    fn btdm_controller_init(
        config_mask: u32,
        config_opts: *const esp_bt_controller_config_t,
    ) -> i32;

    fn btdm_controller_enable(mode: esp_bt_mode_t);

    fn API_vhci_host_check_send_available() -> bool;
    fn API_vhci_host_send_packet(data: *const u8, len: u16);
    fn API_vhci_host_register_callback(vhci_host_callbac: *const VhciHostCallbacks) -> i32;

    #[cfg(not(esp32))]
    fn coex_pti_v2();
}

static VHCI_HOST_CALLBACK: VhciHostCallbacks = VhciHostCallbacks {
    notify_host_send_available,
    notify_host_recv,
};

extern "C" fn notify_host_send_available() {
    trace!("notify_host_send_available");

    let callback = HciCallbackGuard::enter();
    if !callback.admitted {
        return;
    }

    PACKET_SENT.store(true, Ordering::Release);
    HCI_TX_WAKER.wake();
}

extern "C" fn notify_host_recv(data: *mut u8, len: u16) -> i32 {
    trace!("notify_host_recv {:?} {}", data, len);

    let callback = HciCallbackGuard::enter();
    if !callback.admitted {
        return 0;
    }

    let data = unsafe { core::slice::from_raw_parts(data, len as usize) };

    let mut packet_data = heapless::Vec::<u8, HCI_PACKET_CAPACITY>::new();
    if packet_data.extend_from_slice(data).is_err() {
        record_rx_oversize();
        return 0;
    }

    let (queued, depth) = super::BT_STATE.with(|state| {
        let queued = state
            .rx_queue
            .push_back(ReceivedPacket { data: packet_data })
            .is_ok();
        (queued, state.rx_queue.len())
    });
    if !queued {
        record_rx_queue_overflow();
        return 0;
    }
    record_rx_queue_depth(depth);

    super::dump_packet_info(data);

    crate::ble::controller::hci_read_data_available();

    0
}

// This is fine, we're only accessing it inside a critical section (protected by INTERRUPT_LOCK).
static mut G_INTER_FLAGS: heapless::Vec<esp_sync::RestoreState, 10> = heapless::Vec::new();

static INTERRUPT_LOCK: RawMutex = RawMutex::new();

#[ram]
unsafe extern "C" fn interrupt_enable() {
    #[allow(static_mut_refs)]
    unsafe {
        let flags = unwrap!(
            G_INTER_FLAGS.pop(),
            "interrupt_enable called without prior interrupt_disable"
        );
        trace!("interrupt_enable {:?}", flags);
        INTERRUPT_LOCK.release(flags);
    }
}

#[ram]
unsafe extern "C" fn interrupt_disable() {
    trace!("interrupt_disable");
    #[allow(static_mut_refs)]
    unsafe {
        let flags = INTERRUPT_LOCK.acquire();
        unwrap!(
            G_INTER_FLAGS.push(flags),
            "interrupt_disable was called too many times"
        );
        trace!("interrupt_disable {:?}", flags);
    }
}

#[ram]
unsafe extern "C" fn task_yield() {
    crate::preempt::yield_task();
}

unsafe extern "C" fn task_yield_from_isr() {
    // This is not called because we never set xHigherPriorityTaskWoken = true in the `_from_isr`
    // functions. This should be revisited if a scheduler needs it.
    crate::preempt::yield_task_from_isr();
}

unsafe extern "C" fn mutex_create() -> *const () {
    todo!();
}

unsafe extern "C" fn mutex_delete(_mutex: *const ()) {
    todo!();
}

unsafe extern "C" fn mutex_lock(_mutex: *const ()) -> i32 {
    todo!();
}

unsafe extern "C" fn mutex_unlock(_mutex: *const ()) -> i32 {
    todo!();
}

unsafe extern "C" fn task_create(
    func: *mut crate::sys::c_types::c_void,
    name_ptr: *const c_char,
    stack_depth: u32,
    param: *mut crate::sys::c_types::c_void,
    prio: u32,
    handle: *mut crate::sys::c_types::c_void,
    core_id: u32,
) -> i32 {
    let name = unsafe { str_from_c(name_ptr) };
    trace!(
        "task_create {:?} {:?} {} {} {:?} {} {:?} {}",
        func,
        name_ptr,
        name,
        stack_depth,
        param,
        prio,
        handle,
        core_id
    );

    unsafe {
        let task_func = core::mem::transmute::<
            *mut crate::sys::c_types::c_void,
            extern "C" fn(*mut crate::sys::c_types::c_void),
        >(func);

        let task = crate::preempt::task_create(
            name,
            task_func,
            param,
            prio,
            if core_id < 2 { Some(core_id) } else { None },
            stack_depth as usize,
        );
        *(handle as *mut usize) = task.as_ptr() as usize;
    }

    1
}

unsafe extern "C" fn task_delete(task: *mut ()) {
    trace!("task delete called for {:?}", task);

    unsafe {
        crate::preempt::schedule_task_deletion(NonNull::new(task));
    }
}

#[cfg(esp32)]
#[ram]
unsafe extern "C" fn cause_sw_intr_to_core(_core: i32, _intr_no: i32) -> i32 {
    trace!("cause_sw_intr_to_core {} {}", _core, _intr_no);
    unsafe { xtensa_lx_rt::xtensa_lx::interrupt::set(1 << _intr_no) };
    0
}

#[allow(unused)]
#[ram]
unsafe extern "C" fn srand(seed: u32) {
    debug!("!!!! unimplemented srand {}", seed);
}

#[allow(unused)]
#[ram]
unsafe extern "C" fn rand() -> i32 {
    trace!("rand");
    unsafe { crate::common_adapter::random() as i32 }
}

#[ram]
unsafe extern "C" fn btdm_lpcycles_2_hus(_cycles: u32, _error_corr: u32) -> u32 {
    todo!();
}

#[ram]
unsafe extern "C" fn btdm_hus_2_lpcycles(us: u32) -> u32 {
    const RTC_CLK_CAL_FRACT: u32 = 19;
    let g_btdm_lpcycle_us_frac = RTC_CLK_CAL_FRACT;
    let g_btdm_lpcycle_us = 2 << (g_btdm_lpcycle_us_frac);

    // Converts a duration in half us into a number of low power clock cycles.
    let cycles: u64 = ((us as u64) << g_btdm_lpcycle_us_frac) / (g_btdm_lpcycle_us as u64);
    trace!("btdm_hus_2_lpcycles {} {}", us, cycles);

    cycles as u32
}

unsafe extern "C" fn btdm_sleep_check_duration(_slot_cnt: i32) -> i32 {
    todo!();
}

unsafe extern "C" fn btdm_sleep_enter_phase1(_lpcycles: i32) {
    todo!();
}

unsafe extern "C" fn btdm_sleep_enter_phase2() {
    todo!();
}

unsafe extern "C" fn btdm_sleep_exit_phase1() {
    todo!();
}

unsafe extern "C" fn btdm_sleep_exit_phase2() {
    todo!();
}

unsafe extern "C" fn btdm_sleep_exit_phase3() {
    todo!();
}

unsafe extern "C" fn coex_schm_status_bit_set(_typ: i32, status: i32) {
    trace!("coex_schm_status_bit_set {} {}", _typ, status);
    #[cfg(feature = "coex")]
    unsafe {
        crate::sys::include::coex_schm_status_bit_set(_typ as u32, status as u32)
    };
}

unsafe extern "C" fn coex_schm_status_bit_clear(_typ: i32, status: i32) {
    trace!("coex_schm_status_bit_clear {} {}", _typ, status);
    #[cfg(feature = "coex")]
    unsafe {
        crate::sys::include::coex_schm_status_bit_clear(_typ as u32, status as u32)
    };
}

#[ram]
unsafe extern "C" fn read_efuse_mac(mac: *const ()) -> i32 {
    unsafe { crate::common_adapter::read_mac(mac as *mut _, 2) }
}

#[cfg(esp32)]
unsafe extern "C" fn set_isr13(n: i32, handler: unsafe extern "C" fn(), arg: *const ()) -> i32 {
    unsafe { ble_os_adapter_chip_specific::set_isr(n, handler, arg) }
}

#[cfg(esp32)]
unsafe extern "C" fn interrupt_l3_disable() {
    // info!("unimplemented interrupt_l3_disable");
}

#[cfg(esp32)]
unsafe extern "C" fn interrupt_l3_restore() {
    //  info!("unimplemented interrupt_l3_restore");
}

#[cfg(esp32)]
unsafe extern "C" fn custom_queue_create(
    _len: u32,
    _item_size: u32,
) -> *mut crate::sys::c_types::c_void {
    todo!();
}

pub(crate) fn ble_init(config: &Config) -> PhyInitGuard<'static> {
    let phy_init_guard;
    reset_transport_state();
    start_hci_callback_epoch();
    PACKET_SENT.store(true, Ordering::Relaxed);
    HCI_OUT_COLLECTOR
        .try_lock()
        .expect("HCI TX collector still owned during BLE initialization")
        .reset();
    unsafe {
        // turn on logging
        #[allow(static_mut_refs)]
        #[cfg(feature = "print-logs-from-driver")]
        {
            unsafe extern "C" {
                static mut g_bt_plf_log_level: u32;
            }

            debug!("g_bt_plf_log_level = {}", g_bt_plf_log_level);
            g_bt_plf_log_level = 10;
        }

        // esp32_bt_controller_init
        ble_os_adapter_chip_specific::btdm_controller_mem_init();

        let mut cfg = ble_os_adapter_chip_specific::create_ble_config(config);

        let res = btdm_osi_funcs_register(addr_of!(G_OSI_FUNCS));
        assert!(res == 0, "btdm_osi_funcs_register returned {}", res);

        #[cfg(feature = "coex")]
        {
            let res = crate::wifi::coex_init();
            assert!(res == 0, "coex_init failed");
        }

        let version = btdm_controller_get_compile_version();
        let version_str = str_from_c(version);
        debug!("BT controller compile version {}", version_str);

        ble_os_adapter_chip_specific::bt_periph_module_enable();

        ble_os_adapter_chip_specific::disable_sleep_mode();

        #[cfg(any(esp32c3, esp32s3))]
        let res = btdm_controller_init(&mut cfg as *mut esp_bt_controller_config_t);

        #[cfg(esp32)]
        let res = btdm_controller_init(
            (1 << 3) | (1 << 4),
            &mut cfg as *mut esp_bt_controller_config_t,
        ); // see btdm_config_mask_load for mask

        assert!(res == 0, "btdm_controller_init returned {}", res);

        debug!("The btdm_controller_init was initialized");

        #[cfg(feature = "coex")]
        crate::sys::include::coex_enable();

        phy_init_guard = esp_phy::enable_phy();

        cfg_if::cfg_if! {
            if #[cfg(esp32)] {
                unsafe extern "C" {
                    fn btdm_rf_bb_init_phase2();
                }

                btdm_rf_bb_init_phase2();
                coex_bt_high_prio();
            } else {
                coex_pti_v2();
            }
        }

        #[cfg(feature = "coex")]
        coex_enable();

        btdm_controller_enable(esp_bt_mode_t_ESP_BT_MODE_BLE);

        API_vhci_host_register_callback(&VHCI_HOST_CALLBACK);
        CONTROLLER_CALLBACK_SOURCE_ACTIVE.store(true, Ordering::Release);
    }

    // At some point the "High-speed ADC" entropy source became available.
    unsafe { esp_hal::rng::TrngSource::increase_entropy_source_counter() };
    phy_init_guard
}

pub(crate) fn ble_deinit() {
    begin_hci_callback_shutdown();
    esp_hal::rng::TrngSource::decrease_entropy_source_counter(unsafe {
        esp_hal::Internal::conjure()
    });

    unsafe extern "C" {
        fn btdm_controller_deinit();
    }

    unsafe {
        btdm_controller_deinit();
    }
    CONTROLLER_CALLBACK_SOURCE_ACTIVE.store(false, Ordering::Release);
    HCI_OUT_COLLECTOR
        .try_lock()
        .expect("HCI TX collector still owned during BLE teardown")
        .reset();
    reset_transport_state();
    PACKET_SENT.store(true, Ordering::Relaxed);
    // Disabling the PHY happens automatically, when the BLEController gets dropped.
}
async fn wait_for_tx_signal(mut ready: impl FnMut() -> bool) -> Result<(), HciTransportError> {
    let signaled = poll_fn(|cx| {
        // Register before checking the condition so a callback racing this poll
        // either observes the waker or leaves the condition ready for recheck.
        HCI_TX_WAKER.register(cx.waker());
        if ready() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    });

    if with_timeout(HCI_TX_TIMEOUT, signaled).await.is_err() {
        record_tx_timeout();
        Err(HciTransportError::TxTimeout)
    } else {
        Ok(())
    }
}

/// Sends HCI data to the BLE controller without blocking the async executor.
#[instability::unstable]
pub async fn send_hci(data: &[u8]) -> Result<(), HciTransportError> {
    if transport_faulted() {
        return Err(HciTransportError::Faulted);
    }
    let mut hci_out = match with_timeout(HCI_TX_TIMEOUT, HCI_OUT_COLLECTOR.lock()).await {
        Ok(hci_out) => hci_out,
        Err(_) => {
            record_tx_timeout();
            return Err(HciTransportError::TxTimeout);
        }
    };
    // A queued sender may have passed the first fault check before the prior
    // owner was cancelled. Recheck after acquiring the collector and reject
    // the stale partial/in-flight packet rather than appending or resending it.
    if transport_faulted() {
        hci_out.reset();
        return Err(HciTransportError::Faulted);
    }
    if let Err(error) = hci_out.push(data) {
        hci_out.reset();
        record_tx_rejected();
        return Err(error);
    }

    if hci_out.is_ready() {
        let mut cancellation_guard = TxCancellationGuard::<TransportCancellationLatch>::armed();
        let packet = hci_out.packet();

        if wait_for_tx_signal(|| unsafe { API_vhci_host_check_send_available() })
            .await
            .is_err()
        {
            hci_out.reset();
            cancellation_guard.disarm();
            return Err(HciTransportError::TxTimeout);
        }

        PACKET_SENT.store(false, Ordering::Release);

        #[cfg(all(esp32, feature = "coex"))]
        ble_os_adapter_chip_specific::async_wakeup_request(
            ble_os_adapter_chip_specific::BTDM_ASYNC_WAKEUP_REQ_HCI,
        );

        unsafe {
            API_vhci_host_send_packet(packet.as_ptr(), packet.len() as u16);
        }

        #[cfg(all(esp32, feature = "coex"))]
        ble_os_adapter_chip_specific::async_wakeup_request_end(
            ble_os_adapter_chip_specific::BTDM_ASYNC_WAKEUP_REQ_HCI,
        );

        trace!("sent vhci host packet");
        super::dump_packet_info(packet);

        // Keep the fixed collector intact until the controller acknowledges
        // the packet. The callback wakes this future; Embassy time supplies
        // the independent finite deadline if no callback arrives.
        if wait_for_tx_signal(|| PACKET_SENT.load(Ordering::Acquire))
            .await
            .is_err()
        {
            hci_out.reset();
            cancellation_guard.disarm();
            return Err(HciTransportError::TxTimeout);
        }

        hci_out.reset();
        cancellation_guard.disarm();
    }

    Ok(())
}
