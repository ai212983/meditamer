#![allow(dead_code)]

use esp_wifi_sys::{
    c_types::c_char,
    include::{esp_phy_calibration_data_t, timeval},
};
use portable_atomic::{AtomicU32, AtomicUsize, Ordering};

use crate::{
    binary::{
        c_types::{c_int, c_ulong, c_void},
        include::esp_event_base_t,
    },
    compat::{common::*, semaphore::*},
    hal::{self, clock::ModemClockController, ram},
    time::blob_ticks_to_micros,
};

fn wifi_use_legacy_esp_timer_get_time_diag_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_ESP_TIMER_GET_TIME_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("ESP_RADIO_USE_LEGACY_ESP_TIMER_GET_TIME_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("MEDITAMER_WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn wifi_use_legacy_queue_send_from_isr_diag_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_QUEUE_SEND_FROM_ISR_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn wifi_use_legacy_semaphore_from_isr_diag_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_SEMAPHORE_FROM_ISR_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("ESP_RADIO_USE_LEGACY_SEMAPHORE_FROM_ISR_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

#[cfg(feature = "wifi")]
const WIFI_OS_QUEUE_SAMPLE_CAP: usize = 6;

#[cfg(feature = "wifi")]
#[derive(Clone, Copy)]
pub struct PhyCommonClockDiag {
    pub enable_calls: u32,
    pub disable_calls: u32,
    pub ref_count: u32,
    pub real_enable: bool,
}

#[cfg(feature = "wifi")]
unsafe extern "C" {
    fn esp_rtos_queue_item_size(queue: *mut c_void) -> usize;
    fn __esp_rtos_diag_timer_callback_current_ptr() -> usize;
    fn __esp_rtos_diag_timer_callback_current_arg_ptr() -> usize;
    fn __esp_rtos_diag_current_task_ptr_or_zero() -> usize;
}

#[cfg(feature = "wifi")]
#[derive(Clone, Copy)]
pub struct WifiOsDiagSnapshot {
    pub sem_take: u32,
    pub sem_take_isr: u32,
    pub sem_give: u32,
    pub sem_give_isr: u32,
    pub queue_send: u32,
    pub queue_send_first_task_ptr: usize,
    pub queue_send_last_task_ptr: usize,
    pub queue_send_task_changes: u32,
    pub queue_send_isr: u32,
    pub queue_send_isr_legacy_branch: u32,
    pub queue_send_sample_queues: [usize; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_send_sample_tasks: [usize; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_send_sample_item_word0: [u32; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_send_sample_item_pointee_word0: [u32; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_send_sample_item_pointee_word1: [u32; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_send_sample_timer_callback_ptr: [usize; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_send_sample_timer_arg_ptr: [usize; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_send_recent_ordinals: [u32; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_send_recent_queues: [usize; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_send_recent_tasks: [usize; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_send_recent_item_word0: [u32; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_send_recent_item_pointee_word0: [u32; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_send_recent_item_pointee_word1: [u32; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_send_recent_timer_callback_ptr: [usize; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_send_recent_timer_arg_ptr: [usize; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_send_last_item_size: usize,
    pub queue_send_last_item_word0: u32,
    pub queue_send_last_item_word1: u32,
    pub queue_send_last_item_pointee_word0: u32,
    pub queue_send_last_item_pointee_word1: u32,
    pub queue_send_last_timer_callback_ptr: usize,
    pub queue_send_last_timer_arg_ptr: usize,
    pub queue_recv: u32,
    pub queue_recv_first_task_ptr: usize,
    pub queue_recv_last_task_ptr: usize,
    pub queue_recv_task_changes: u32,
    pub queue_recv_isr: u32,
    pub queue_recv_sample_queues: [usize; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_recv_sample_tasks: [usize; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_recv_sample_item_word0: [u32; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_recv_sample_item_pointee_word0: [u32; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_recv_sample_item_pointee_word1: [u32; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_recv_recent_ordinals: [u32; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_recv_recent_queues: [usize; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_recv_recent_tasks: [usize; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_recv_recent_item_word0: [u32; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_recv_recent_item_pointee_word0: [u32; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_recv_recent_item_pointee_word1: [u32; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_recv_recent_caller_ptr: [usize; WIFI_OS_QUEUE_SAMPLE_CAP],
    pub queue_recv_last_item_size: usize,
    pub queue_recv_last_item_word0: u32,
    pub queue_recv_last_item_word1: u32,
    pub queue_recv_last_item_pointee_word0: u32,
    pub queue_recv_last_item_pointee_word1: u32,
    pub queue_recv_last_caller_ptr: usize,
    pub event_post: u32,
}

#[cfg(feature = "wifi")]
static WIFI_OS_SEM_TAKE_COUNT: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_SEM_TAKE_ISR_COUNT: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_SEM_GIVE_COUNT: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_SEM_GIVE_ISR_COUNT: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_COUNT: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_FIRST_TASK_PTR: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_LAST_TASK_PTR: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_TASK_CHANGES: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_ISR_COUNT: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_ISR_LEGACY_BRANCH_COUNT: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_SAMPLE_QUEUES: [AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicUsize::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_SAMPLE_TASKS: [AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicUsize::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_SAMPLE_ITEM_WORD0: [AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicU32::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_SAMPLE_ITEM_POINTEE_WORD0: [AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicU32::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_SAMPLE_ITEM_POINTEE_WORD1: [AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicU32::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_SAMPLE_TIMER_CALLBACK_PTR: [AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicUsize::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_SAMPLE_TIMER_ARG_PTR: [AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicUsize::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_RECENT_ORDINALS: [AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicU32::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_RECENT_QUEUES: [AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicUsize::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_RECENT_TASKS: [AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicUsize::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_RECENT_ITEM_WORD0: [AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicU32::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_RECENT_ITEM_POINTEE_WORD0: [AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicU32::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_RECENT_ITEM_POINTEE_WORD1: [AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicU32::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_RECENT_TIMER_CALLBACK_PTR: [AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicUsize::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_RECENT_TIMER_ARG_PTR: [AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicUsize::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_LAST_ITEM_SIZE: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_LAST_ITEM_WORD0: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_LAST_ITEM_WORD1: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_LAST_ITEM_POINTEE_WORD0: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_LAST_ITEM_POINTEE_WORD1: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_LAST_TIMER_CALLBACK_PTR: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_SEND_LAST_TIMER_ARG_PTR: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_RECV_COUNT: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_RECV_FIRST_TASK_PTR: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_RECV_LAST_TASK_PTR: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_RECV_TASK_CHANGES: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_RECV_ISR_COUNT: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_RECV_SAMPLE_QUEUES: [AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicUsize::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_RECV_SAMPLE_TASKS: [AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicUsize::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_RECV_SAMPLE_ITEM_WORD0: [AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicU32::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_RECV_SAMPLE_ITEM_POINTEE_WORD0: [AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicU32::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_RECV_SAMPLE_ITEM_POINTEE_WORD1: [AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicU32::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_RECV_RECENT_ORDINALS: [AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicU32::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_RECV_RECENT_QUEUES: [AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicUsize::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_RECV_RECENT_TASKS: [AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicUsize::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_RECV_RECENT_ITEM_WORD0: [AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicU32::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_RECV_RECENT_ITEM_POINTEE_WORD0: [AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicU32::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_RECV_RECENT_ITEM_POINTEE_WORD1: [AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicU32::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_RECV_RECENT_CALLER_PTR: [AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP] =
    [const { AtomicUsize::new(0) }; WIFI_OS_QUEUE_SAMPLE_CAP];
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_RECV_LAST_ITEM_SIZE: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_RECV_LAST_ITEM_WORD0: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_RECV_LAST_ITEM_WORD1: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_RECV_LAST_ITEM_POINTEE_WORD0: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_RECV_LAST_ITEM_POINTEE_WORD1: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_QUEUE_RECV_LAST_CALLER_PTR: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "wifi")]
static WIFI_OS_EVENT_POST_COUNT: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "wifi")]
static PHY_COMMON_CLOCK_ENABLE_CALLS: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "wifi")]
static PHY_COMMON_CLOCK_DISABLE_CALLS: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "wifi")]
static PHY_COMMON_CLOCK_ENABLE_REF: AtomicU32 = AtomicU32::new(0);

#[cfg(feature = "wifi")]
fn phy_common_clock_real_enable() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_PHY_COMMON_CLOCK_ENABLE_REAL_DIAG"),
        Some("1")
    )
}

#[cfg(feature = "wifi")]
pub fn wifi_os_diag_reset() {
    WIFI_OS_SEM_TAKE_COUNT.store(0, Ordering::Relaxed);
    WIFI_OS_SEM_TAKE_ISR_COUNT.store(0, Ordering::Relaxed);
    WIFI_OS_SEM_GIVE_COUNT.store(0, Ordering::Relaxed);
    WIFI_OS_SEM_GIVE_ISR_COUNT.store(0, Ordering::Relaxed);
    WIFI_OS_QUEUE_SEND_COUNT.store(0, Ordering::Relaxed);
    WIFI_OS_QUEUE_SEND_FIRST_TASK_PTR.store(0, Ordering::Relaxed);
    WIFI_OS_QUEUE_SEND_LAST_TASK_PTR.store(0, Ordering::Relaxed);
    WIFI_OS_QUEUE_SEND_TASK_CHANGES.store(0, Ordering::Relaxed);
    WIFI_OS_QUEUE_SEND_ISR_COUNT.store(0, Ordering::Relaxed);
    WIFI_OS_QUEUE_SEND_ISR_LEGACY_BRANCH_COUNT.store(0, Ordering::Relaxed);
    for slot in 0..WIFI_OS_QUEUE_SAMPLE_CAP {
        WIFI_OS_QUEUE_SEND_SAMPLE_QUEUES[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_SEND_SAMPLE_TASKS[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_SEND_SAMPLE_ITEM_WORD0[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_SEND_SAMPLE_ITEM_POINTEE_WORD0[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_SEND_SAMPLE_ITEM_POINTEE_WORD1[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_SEND_SAMPLE_TIMER_CALLBACK_PTR[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_SEND_SAMPLE_TIMER_ARG_PTR[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_SEND_RECENT_ORDINALS[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_SEND_RECENT_QUEUES[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_SEND_RECENT_TASKS[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_SEND_RECENT_ITEM_WORD0[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_SEND_RECENT_ITEM_POINTEE_WORD0[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_SEND_RECENT_ITEM_POINTEE_WORD1[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_SEND_RECENT_TIMER_CALLBACK_PTR[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_SEND_RECENT_TIMER_ARG_PTR[slot].store(0, Ordering::Relaxed);
    }
    WIFI_OS_QUEUE_SEND_LAST_ITEM_SIZE.store(0, Ordering::Relaxed);
    WIFI_OS_QUEUE_SEND_LAST_ITEM_WORD0.store(0, Ordering::Relaxed);
    WIFI_OS_QUEUE_SEND_LAST_ITEM_WORD1.store(0, Ordering::Relaxed);
    WIFI_OS_QUEUE_SEND_LAST_ITEM_POINTEE_WORD0.store(0, Ordering::Relaxed);
    WIFI_OS_QUEUE_SEND_LAST_ITEM_POINTEE_WORD1.store(0, Ordering::Relaxed);
    WIFI_OS_QUEUE_SEND_LAST_TIMER_CALLBACK_PTR.store(0, Ordering::Relaxed);
    WIFI_OS_QUEUE_SEND_LAST_TIMER_ARG_PTR.store(0, Ordering::Relaxed);
    WIFI_OS_QUEUE_RECV_COUNT.store(0, Ordering::Relaxed);
    WIFI_OS_QUEUE_RECV_FIRST_TASK_PTR.store(0, Ordering::Relaxed);
    WIFI_OS_QUEUE_RECV_LAST_TASK_PTR.store(0, Ordering::Relaxed);
    WIFI_OS_QUEUE_RECV_TASK_CHANGES.store(0, Ordering::Relaxed);
    WIFI_OS_QUEUE_RECV_ISR_COUNT.store(0, Ordering::Relaxed);
    for slot in 0..WIFI_OS_QUEUE_SAMPLE_CAP {
        WIFI_OS_QUEUE_RECV_SAMPLE_QUEUES[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_RECV_SAMPLE_TASKS[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_RECV_SAMPLE_ITEM_WORD0[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_RECV_SAMPLE_ITEM_POINTEE_WORD0[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_RECV_SAMPLE_ITEM_POINTEE_WORD1[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_RECV_RECENT_ORDINALS[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_RECV_RECENT_QUEUES[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_RECV_RECENT_TASKS[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_RECV_RECENT_ITEM_WORD0[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_RECV_RECENT_ITEM_POINTEE_WORD0[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_RECV_RECENT_ITEM_POINTEE_WORD1[slot].store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_RECV_RECENT_CALLER_PTR[slot].store(0, Ordering::Relaxed);
    }
    WIFI_OS_QUEUE_RECV_LAST_ITEM_SIZE.store(0, Ordering::Relaxed);
    WIFI_OS_QUEUE_RECV_LAST_ITEM_WORD0.store(0, Ordering::Relaxed);
    WIFI_OS_QUEUE_RECV_LAST_ITEM_WORD1.store(0, Ordering::Relaxed);
    WIFI_OS_QUEUE_RECV_LAST_ITEM_POINTEE_WORD0.store(0, Ordering::Relaxed);
    WIFI_OS_QUEUE_RECV_LAST_ITEM_POINTEE_WORD1.store(0, Ordering::Relaxed);
    WIFI_OS_QUEUE_RECV_LAST_CALLER_PTR.store(0, Ordering::Relaxed);
    WIFI_OS_EVENT_POST_COUNT.store(0, Ordering::Relaxed);
}

#[cfg(feature = "wifi")]
pub fn wifi_os_diag_snapshot() -> WifiOsDiagSnapshot {
    WifiOsDiagSnapshot {
        sem_take: WIFI_OS_SEM_TAKE_COUNT.load(Ordering::Relaxed),
        sem_take_isr: WIFI_OS_SEM_TAKE_ISR_COUNT.load(Ordering::Relaxed),
        sem_give: WIFI_OS_SEM_GIVE_COUNT.load(Ordering::Relaxed),
        sem_give_isr: WIFI_OS_SEM_GIVE_ISR_COUNT.load(Ordering::Relaxed),
        queue_send: WIFI_OS_QUEUE_SEND_COUNT.load(Ordering::Relaxed),
        queue_send_first_task_ptr: WIFI_OS_QUEUE_SEND_FIRST_TASK_PTR.load(Ordering::Relaxed),
        queue_send_last_task_ptr: WIFI_OS_QUEUE_SEND_LAST_TASK_PTR.load(Ordering::Relaxed),
        queue_send_task_changes: WIFI_OS_QUEUE_SEND_TASK_CHANGES.load(Ordering::Relaxed),
        queue_send_isr: WIFI_OS_QUEUE_SEND_ISR_COUNT.load(Ordering::Relaxed),
        queue_send_isr_legacy_branch: WIFI_OS_QUEUE_SEND_ISR_LEGACY_BRANCH_COUNT
            .load(Ordering::Relaxed),
        queue_send_sample_queues: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_SEND_SAMPLE_QUEUES[idx].load(Ordering::Relaxed)
        }),
        queue_send_sample_tasks: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_SEND_SAMPLE_TASKS[idx].load(Ordering::Relaxed)
        }),
        queue_send_sample_item_word0: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_SEND_SAMPLE_ITEM_WORD0[idx].load(Ordering::Relaxed)
        }),
        queue_send_sample_item_pointee_word0: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_SEND_SAMPLE_ITEM_POINTEE_WORD0[idx].load(Ordering::Relaxed)
        }),
        queue_send_sample_item_pointee_word1: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_SEND_SAMPLE_ITEM_POINTEE_WORD1[idx].load(Ordering::Relaxed)
        }),
        queue_send_sample_timer_callback_ptr: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_SEND_SAMPLE_TIMER_CALLBACK_PTR[idx].load(Ordering::Relaxed)
        }),
        queue_send_sample_timer_arg_ptr: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_SEND_SAMPLE_TIMER_ARG_PTR[idx].load(Ordering::Relaxed)
        }),
        queue_send_recent_ordinals: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_SEND_RECENT_ORDINALS[idx].load(Ordering::Relaxed)
        }),
        queue_send_recent_queues: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_SEND_RECENT_QUEUES[idx].load(Ordering::Relaxed)
        }),
        queue_send_recent_tasks: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_SEND_RECENT_TASKS[idx].load(Ordering::Relaxed)
        }),
        queue_send_recent_item_word0: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_SEND_RECENT_ITEM_WORD0[idx].load(Ordering::Relaxed)
        }),
        queue_send_recent_item_pointee_word0: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_SEND_RECENT_ITEM_POINTEE_WORD0[idx].load(Ordering::Relaxed)
        }),
        queue_send_recent_item_pointee_word1: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_SEND_RECENT_ITEM_POINTEE_WORD1[idx].load(Ordering::Relaxed)
        }),
        queue_send_recent_timer_callback_ptr: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_SEND_RECENT_TIMER_CALLBACK_PTR[idx].load(Ordering::Relaxed)
        }),
        queue_send_recent_timer_arg_ptr: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_SEND_RECENT_TIMER_ARG_PTR[idx].load(Ordering::Relaxed)
        }),
        queue_send_last_item_size: WIFI_OS_QUEUE_SEND_LAST_ITEM_SIZE.load(Ordering::Relaxed),
        queue_send_last_item_word0: WIFI_OS_QUEUE_SEND_LAST_ITEM_WORD0.load(Ordering::Relaxed),
        queue_send_last_item_word1: WIFI_OS_QUEUE_SEND_LAST_ITEM_WORD1.load(Ordering::Relaxed),
        queue_send_last_item_pointee_word0: WIFI_OS_QUEUE_SEND_LAST_ITEM_POINTEE_WORD0
            .load(Ordering::Relaxed),
        queue_send_last_item_pointee_word1: WIFI_OS_QUEUE_SEND_LAST_ITEM_POINTEE_WORD1
            .load(Ordering::Relaxed),
        queue_send_last_timer_callback_ptr: WIFI_OS_QUEUE_SEND_LAST_TIMER_CALLBACK_PTR
            .load(Ordering::Relaxed),
        queue_send_last_timer_arg_ptr: WIFI_OS_QUEUE_SEND_LAST_TIMER_ARG_PTR
            .load(Ordering::Relaxed),
        queue_recv: WIFI_OS_QUEUE_RECV_COUNT.load(Ordering::Relaxed),
        queue_recv_first_task_ptr: WIFI_OS_QUEUE_RECV_FIRST_TASK_PTR.load(Ordering::Relaxed),
        queue_recv_last_task_ptr: WIFI_OS_QUEUE_RECV_LAST_TASK_PTR.load(Ordering::Relaxed),
        queue_recv_task_changes: WIFI_OS_QUEUE_RECV_TASK_CHANGES.load(Ordering::Relaxed),
        queue_recv_isr: WIFI_OS_QUEUE_RECV_ISR_COUNT.load(Ordering::Relaxed),
        queue_recv_sample_queues: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_RECV_SAMPLE_QUEUES[idx].load(Ordering::Relaxed)
        }),
        queue_recv_sample_tasks: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_RECV_SAMPLE_TASKS[idx].load(Ordering::Relaxed)
        }),
        queue_recv_sample_item_word0: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_RECV_SAMPLE_ITEM_WORD0[idx].load(Ordering::Relaxed)
        }),
        queue_recv_sample_item_pointee_word0: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_RECV_SAMPLE_ITEM_POINTEE_WORD0[idx].load(Ordering::Relaxed)
        }),
        queue_recv_sample_item_pointee_word1: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_RECV_SAMPLE_ITEM_POINTEE_WORD1[idx].load(Ordering::Relaxed)
        }),
        queue_recv_recent_ordinals: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_RECV_RECENT_ORDINALS[idx].load(Ordering::Relaxed)
        }),
        queue_recv_recent_queues: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_RECV_RECENT_QUEUES[idx].load(Ordering::Relaxed)
        }),
        queue_recv_recent_tasks: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_RECV_RECENT_TASKS[idx].load(Ordering::Relaxed)
        }),
        queue_recv_recent_item_word0: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_RECV_RECENT_ITEM_WORD0[idx].load(Ordering::Relaxed)
        }),
        queue_recv_recent_item_pointee_word0: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_RECV_RECENT_ITEM_POINTEE_WORD0[idx].load(Ordering::Relaxed)
        }),
        queue_recv_recent_item_pointee_word1: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_RECV_RECENT_ITEM_POINTEE_WORD1[idx].load(Ordering::Relaxed)
        }),
        queue_recv_recent_caller_ptr: core::array::from_fn(|idx| {
            WIFI_OS_QUEUE_RECV_RECENT_CALLER_PTR[idx].load(Ordering::Relaxed)
        }),
        queue_recv_last_item_size: WIFI_OS_QUEUE_RECV_LAST_ITEM_SIZE.load(Ordering::Relaxed),
        queue_recv_last_item_word0: WIFI_OS_QUEUE_RECV_LAST_ITEM_WORD0.load(Ordering::Relaxed),
        queue_recv_last_item_word1: WIFI_OS_QUEUE_RECV_LAST_ITEM_WORD1.load(Ordering::Relaxed),
        queue_recv_last_item_pointee_word0: WIFI_OS_QUEUE_RECV_LAST_ITEM_POINTEE_WORD0
            .load(Ordering::Relaxed),
        queue_recv_last_item_pointee_word1: WIFI_OS_QUEUE_RECV_LAST_ITEM_POINTEE_WORD1
            .load(Ordering::Relaxed),
        queue_recv_last_caller_ptr: WIFI_OS_QUEUE_RECV_LAST_CALLER_PTR.load(Ordering::Relaxed),
        event_post: WIFI_OS_EVENT_POST_COUNT.load(Ordering::Relaxed),
    }
}

#[cfg(feature = "wifi")]
pub fn reset_phy_common_clock_diag() {
    PHY_COMMON_CLOCK_ENABLE_CALLS.store(0, Ordering::Relaxed);
    PHY_COMMON_CLOCK_DISABLE_CALLS.store(0, Ordering::Relaxed);
    PHY_COMMON_CLOCK_ENABLE_REF.store(0, Ordering::Relaxed);
}

#[cfg(feature = "wifi")]
pub fn phy_common_clock_diag() -> PhyCommonClockDiag {
    PhyCommonClockDiag {
        enable_calls: PHY_COMMON_CLOCK_ENABLE_CALLS.load(Ordering::Relaxed),
        disable_calls: PHY_COMMON_CLOCK_DISABLE_CALLS.load(Ordering::Relaxed),
        ref_count: PHY_COMMON_CLOCK_ENABLE_REF.load(Ordering::Relaxed),
        real_enable: phy_common_clock_real_enable(),
    }
}

#[cfg(feature = "wifi")]
fn record_first_last_task_ptr(
    first: &AtomicUsize,
    last: &AtomicUsize,
    changes: &AtomicU32,
    ptr: usize,
) {
    if ptr == 0 {
        return;
    }
    let previous = last.swap(ptr, Ordering::Relaxed);
    if previous == 0 {
        first.store(ptr, Ordering::Relaxed);
    } else if previous != ptr {
        changes.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(feature = "wifi")]
fn record_queue_send_sample(
    queues: &[AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP],
    tasks: &[AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP],
    item_word0: &[AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP],
    item_pointee_word0: &[AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP],
    item_pointee_word1: &[AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP],
    timer_callback_ptr: &[AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP],
    timer_arg_ptr: &[AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP],
    ordinal: u32,
    queue_ptr: usize,
    task_ptr: usize,
    last_item_word0: u32,
    last_item_pointee_word0: u32,
    last_item_pointee_word1: u32,
    last_timer_callback_ptr: usize,
    last_timer_arg_ptr: usize,
) {
    let idx = ordinal as usize;
    if idx >= WIFI_OS_QUEUE_SAMPLE_CAP {
        return;
    }
    queues[idx].store(queue_ptr, Ordering::Relaxed);
    tasks[idx].store(task_ptr, Ordering::Relaxed);
    item_word0[idx].store(last_item_word0, Ordering::Relaxed);
    item_pointee_word0[idx].store(last_item_pointee_word0, Ordering::Relaxed);
    item_pointee_word1[idx].store(last_item_pointee_word1, Ordering::Relaxed);
    timer_callback_ptr[idx].store(last_timer_callback_ptr, Ordering::Relaxed);
    timer_arg_ptr[idx].store(last_timer_arg_ptr, Ordering::Relaxed);
}

#[cfg(feature = "wifi")]
fn record_queue_recv_sample(
    queues: &[AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP],
    tasks: &[AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP],
    item_word0: &[AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP],
    item_pointee_word0: &[AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP],
    item_pointee_word1: &[AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP],
    ordinal: u32,
    queue_ptr: usize,
    task_ptr: usize,
    last_item_word0: u32,
    last_item_pointee_word0: u32,
    last_item_pointee_word1: u32,
) {
    let idx = ordinal as usize;
    if idx >= WIFI_OS_QUEUE_SAMPLE_CAP {
        return;
    }
    queues[idx].store(queue_ptr, Ordering::Relaxed);
    tasks[idx].store(task_ptr, Ordering::Relaxed);
    item_word0[idx].store(last_item_word0, Ordering::Relaxed);
    item_pointee_word0[idx].store(last_item_pointee_word0, Ordering::Relaxed);
    item_pointee_word1[idx].store(last_item_pointee_word1, Ordering::Relaxed);
}

#[cfg(feature = "wifi")]
fn record_queue_recv_recent_sample(
    ordinals: &[AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP],
    queues: &[AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP],
    tasks: &[AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP],
    item_word0: &[AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP],
    item_pointee_word0: &[AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP],
    item_pointee_word1: &[AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP],
    caller_ptr: &[AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP],
    ordinal: u32,
    queue_ptr: usize,
    task_ptr: usize,
    last_item_word0: u32,
    last_item_pointee_word0: u32,
    last_item_pointee_word1: u32,
    last_caller_ptr: usize,
) {
    let idx = (ordinal as usize) % WIFI_OS_QUEUE_SAMPLE_CAP;
    ordinals[idx].store(ordinal, Ordering::Relaxed);
    queues[idx].store(queue_ptr, Ordering::Relaxed);
    tasks[idx].store(task_ptr, Ordering::Relaxed);
    item_word0[idx].store(last_item_word0, Ordering::Relaxed);
    item_pointee_word0[idx].store(last_item_pointee_word0, Ordering::Relaxed);
    item_pointee_word1[idx].store(last_item_pointee_word1, Ordering::Relaxed);
    caller_ptr[idx].store(last_caller_ptr, Ordering::Relaxed);
}

#[cfg(feature = "wifi")]
#[cfg(xtensa)]
fn current_queue_recv_caller_ptr() -> usize {
    let caller_ptr: usize;
    unsafe {
        core::arch::asm!("mov {0}, a0", out(reg) caller_ptr);
    }
    caller_ptr
}

#[cfg(feature = "wifi")]
#[cfg(not(xtensa))]
fn current_queue_recv_caller_ptr() -> usize {
    0
}

#[cfg(feature = "wifi")]
fn record_queue_send_recent_sample(
    ordinals: &[AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP],
    queues: &[AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP],
    tasks: &[AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP],
    item_word0: &[AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP],
    item_pointee_word0: &[AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP],
    item_pointee_word1: &[AtomicU32; WIFI_OS_QUEUE_SAMPLE_CAP],
    timer_callback_ptr: &[AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP],
    timer_arg_ptr: &[AtomicUsize; WIFI_OS_QUEUE_SAMPLE_CAP],
    ordinal: u32,
    queue_ptr: usize,
    task_ptr: usize,
    last_item_word0: u32,
    last_item_pointee_word0: u32,
    last_item_pointee_word1: u32,
    last_timer_callback_ptr: usize,
    last_timer_arg_ptr: usize,
) {
    let idx = (ordinal as usize) % WIFI_OS_QUEUE_SAMPLE_CAP;
    ordinals[idx].store(ordinal, Ordering::Relaxed);
    queues[idx].store(queue_ptr, Ordering::Relaxed);
    tasks[idx].store(task_ptr, Ordering::Relaxed);
    item_word0[idx].store(last_item_word0, Ordering::Relaxed);
    item_pointee_word0[idx].store(last_item_pointee_word0, Ordering::Relaxed);
    item_pointee_word1[idx].store(last_item_pointee_word1, Ordering::Relaxed);
    timer_callback_ptr[idx].store(last_timer_callback_ptr, Ordering::Relaxed);
    timer_arg_ptr[idx].store(last_timer_arg_ptr, Ordering::Relaxed);
}

#[cfg(feature = "wifi")]
fn current_timer_exec_snapshot() -> (usize, usize) {
    unsafe {
        (
            __esp_rtos_diag_timer_callback_current_ptr(),
            __esp_rtos_diag_timer_callback_current_arg_ptr(),
        )
    }
}

#[cfg(feature = "wifi")]
#[inline]
fn current_task_ptr_for_diag() -> usize {
    unsafe { __esp_rtos_diag_current_task_ptr_or_zero() }
}

#[cfg(feature = "wifi")]
fn record_queue_send_item_words(queue: *mut c_void, item: *const c_void) {
    if queue.is_null() || item.is_null() {
        return;
    }
    let item_size = unsafe { esp_rtos_queue_item_size(queue) };
    WIFI_OS_QUEUE_SEND_LAST_ITEM_SIZE.store(item_size, Ordering::Relaxed);

    let bytes = unsafe { core::slice::from_raw_parts(item.cast::<u8>(), item_size.min(8)) };
    let mut word0 = [0u8; 4];
    let mut word1 = [0u8; 4];
    let first_len = bytes.len().min(4);
    word0[..first_len].copy_from_slice(&bytes[..first_len]);
    if bytes.len() > 4 {
        let second_len = (bytes.len() - 4).min(4);
        word1[..second_len].copy_from_slice(&bytes[4..4 + second_len]);
    }
    WIFI_OS_QUEUE_SEND_LAST_ITEM_WORD0.store(u32::from_le_bytes(word0), Ordering::Relaxed);
    let word1 = u32::from_le_bytes(word1);
    WIFI_OS_QUEUE_SEND_LAST_ITEM_WORD1.store(word1, Ordering::Relaxed);

    let pointee_addr = word1 as usize;
    let pointee = pointee_addr as *const u8;
    if (0x3ff0_0000..0x4000_0000).contains(&pointee_addr) {
        let pointee_bytes = unsafe { core::slice::from_raw_parts(pointee, 8) };
        let mut pointee_word0 = [0u8; 4];
        let mut pointee_word1 = [0u8; 4];
        pointee_word0.copy_from_slice(&pointee_bytes[..4]);
        pointee_word1.copy_from_slice(&pointee_bytes[4..8]);
        WIFI_OS_QUEUE_SEND_LAST_ITEM_POINTEE_WORD0
            .store(u32::from_le_bytes(pointee_word0), Ordering::Relaxed);
        WIFI_OS_QUEUE_SEND_LAST_ITEM_POINTEE_WORD1
            .store(u32::from_le_bytes(pointee_word1), Ordering::Relaxed);
    } else {
        WIFI_OS_QUEUE_SEND_LAST_ITEM_POINTEE_WORD0.store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_SEND_LAST_ITEM_POINTEE_WORD1.store(0, Ordering::Relaxed);
    }
    let (timer_callback_ptr, timer_arg_ptr) = current_timer_exec_snapshot();
    WIFI_OS_QUEUE_SEND_LAST_TIMER_CALLBACK_PTR.store(timer_callback_ptr, Ordering::Relaxed);
    WIFI_OS_QUEUE_SEND_LAST_TIMER_ARG_PTR.store(timer_arg_ptr, Ordering::Relaxed);
}

#[cfg(feature = "wifi")]
fn queue_send_item_snapshot(queue: *mut c_void, item: *const c_void) -> (u32, u32, u32) {
    if queue.is_null() || item.is_null() {
        return (0, 0, 0);
    }
    let item_size = unsafe { esp_rtos_queue_item_size(queue) };
    let bytes = unsafe { core::slice::from_raw_parts(item.cast::<u8>(), item_size.min(8)) };
    let mut word0 = [0u8; 4];
    let mut word1 = [0u8; 4];
    let first_len = bytes.len().min(4);
    word0[..first_len].copy_from_slice(&bytes[..first_len]);
    if bytes.len() > 4 {
        let second_len = (bytes.len() - 4).min(4);
        word1[..second_len].copy_from_slice(&bytes[4..4 + second_len]);
    }

    let item_word0 = u32::from_le_bytes(word0);
    let pointee_addr = u32::from_le_bytes(word1) as usize;
    if (0x3ff0_0000..0x4000_0000).contains(&pointee_addr) {
        let pointee = pointee_addr as *const u8;
        let pointee_bytes = unsafe { core::slice::from_raw_parts(pointee, 8) };
        let mut pointee_word0 = [0u8; 4];
        let mut pointee_word1 = [0u8; 4];
        pointee_word0.copy_from_slice(&pointee_bytes[..4]);
        pointee_word1.copy_from_slice(&pointee_bytes[4..8]);
        (
            item_word0,
            u32::from_le_bytes(pointee_word0),
            u32::from_le_bytes(pointee_word1),
        )
    } else {
        (item_word0, 0, 0)
    }
}

#[cfg(feature = "wifi")]
fn record_queue_recv_item_words(queue: *mut c_void, item: *mut c_void) {
    if queue.is_null() || item.is_null() {
        return;
    }
    let item_size = unsafe { esp_rtos_queue_item_size(queue) };
    WIFI_OS_QUEUE_RECV_LAST_ITEM_SIZE.store(item_size, Ordering::Relaxed);
    let bytes = unsafe { core::slice::from_raw_parts(item.cast::<u8>(), item_size.min(8)) };
    let mut word0 = [0u8; 4];
    let mut word1 = [0u8; 4];
    let first_len = bytes.len().min(4);
    word0[..first_len].copy_from_slice(&bytes[..first_len]);
    if bytes.len() > 4 {
        let second_len = (bytes.len() - 4).min(4);
        word1[..second_len].copy_from_slice(&bytes[4..4 + second_len]);
    }
    WIFI_OS_QUEUE_RECV_LAST_ITEM_WORD0.store(u32::from_le_bytes(word0), Ordering::Relaxed);
    let word1 = u32::from_le_bytes(word1);
    WIFI_OS_QUEUE_RECV_LAST_ITEM_WORD1.store(word1, Ordering::Relaxed);
    let pointee_addr = word1 as usize;
    let pointee = pointee_addr as *const u8;
    if (0x3ff0_0000..0x4000_0000).contains(&pointee_addr) {
        let pointee_bytes = unsafe { core::slice::from_raw_parts(pointee, 8) };
        let mut pointee_word0 = [0u8; 4];
        let mut pointee_word1 = [0u8; 4];
        pointee_word0.copy_from_slice(&pointee_bytes[..4]);
        pointee_word1.copy_from_slice(&pointee_bytes[4..8]);
        WIFI_OS_QUEUE_RECV_LAST_ITEM_POINTEE_WORD0
            .store(u32::from_le_bytes(pointee_word0), Ordering::Relaxed);
        WIFI_OS_QUEUE_RECV_LAST_ITEM_POINTEE_WORD1
            .store(u32::from_le_bytes(pointee_word1), Ordering::Relaxed);
    } else {
        WIFI_OS_QUEUE_RECV_LAST_ITEM_POINTEE_WORD0.store(0, Ordering::Relaxed);
        WIFI_OS_QUEUE_RECV_LAST_ITEM_POINTEE_WORD1.store(0, Ordering::Relaxed);
    }
}

/// **************************************************************************
/// Name: esp_semphr_create
///
/// Description:
///   Create and initialize semaphore
///
/// Input Parameters:
///   max  - No mean
///   init - semaphore initialization value
///
/// Returned Value:
///   Semaphore data pointer
///
/// *************************************************************************
#[allow(unused)]
pub unsafe extern "C" fn semphr_create(max: u32, init: u32) -> *mut c_void {
    let sem = sem_create(max, init);
    wifi_init_runtime_trace("semphr_create");
    trace!("semphr_create - max {} init {}", max, init);
    sem
}

/// **************************************************************************
/// Name: esp_semphr_delete
///
/// Description:
///   Delete semaphore
///
/// Input Parameters:
///   semphr - Semaphore data pointer
///
/// Returned Value:
///   None
///
/// *************************************************************************
#[allow(unused)]
pub unsafe extern "C" fn semphr_delete(semphr: *mut c_void) {
    trace!("semphr_delete {:?}", semphr);
    sem_delete(semphr);
}

/// **************************************************************************
/// Name: esp_semphr_take
///
/// Description:
///   Wait semaphore within a certain period of time
///
/// Input Parameters:
///   semphr - Semaphore data pointer
///   ticks  - Wait system ticks
///
/// Returned Value:
///   True if success or false if fail
///
/// *************************************************************************
#[ram]
pub unsafe extern "C" fn semphr_take(semphr: *mut c_void, tick: u32) -> i32 {
    wifi_init_runtime_trace("semphr_take");
    #[cfg(feature = "wifi")]
    WIFI_OS_SEM_TAKE_COUNT.fetch_add(1, Ordering::Relaxed);
    trace!(">>>> semphr_take {:?} block_time_tick {}", semphr, tick);
    sem_take(semphr, blob_ticks_to_micros(tick))
}

#[ram]
pub unsafe extern "C" fn semphr_take_from_isr(
    semphr: *mut c_void,
    higher_priority_task_waken: *mut bool,
) -> i32 {
    #[cfg(feature = "wifi")]
    WIFI_OS_SEM_TAKE_ISR_COUNT.fetch_add(1, Ordering::Relaxed);
    trace!(">>>> semphr_take_from_isr {:?}", semphr);
    if wifi_use_legacy_semaphore_from_isr_diag_enabled() {
        if !higher_priority_task_waken.is_null() {
            unsafe { higher_priority_task_waken.write(false) };
        }
        semphr_take(semphr, 0)
    } else {
        sem_try_take_from_isr(semphr, higher_priority_task_waken)
    }
}

/// **************************************************************************
/// Name: esp_semphr_give
///
/// Description:
///   Post semaphore
///
/// Input Parameters:
///   semphr - Semaphore data pointer
///
/// Returned Value:
///   True if success or false if fail
///
/// *************************************************************************
#[ram]
pub unsafe extern "C" fn semphr_give(semphr: *mut c_void) -> i32 {
    wifi_init_runtime_trace("semphr_give");
    #[cfg(feature = "wifi")]
    WIFI_OS_SEM_GIVE_COUNT.fetch_add(1, Ordering::Relaxed);
    trace!(">>>> semphr_give {:?}", semphr);
    sem_give(semphr)
}

#[ram]
pub unsafe extern "C" fn semphr_give_from_isr(
    semphr: *mut c_void,
    higher_priority_task_waken: *mut bool,
) -> i32 {
    #[cfg(feature = "wifi")]
    WIFI_OS_SEM_GIVE_ISR_COUNT.fetch_add(1, Ordering::Relaxed);
    trace!(">>>> semphr_give_from_isr {:?}", semphr);
    if wifi_use_legacy_semaphore_from_isr_diag_enabled() {
        if !higher_priority_task_waken.is_null() {
            unsafe { higher_priority_task_waken.write(false) };
        }
        semphr_give(semphr)
    } else {
        sem_try_give_from_isr(semphr, higher_priority_task_waken)
    }
}

/// **************************************************************************
/// Name: esp_random_ulong
/// *************************************************************************
#[allow(unused)]
#[ram]
pub unsafe extern "C" fn random() -> c_ulong {
    trace!("random");

    let rng = hal::rng::Rng::new();
    rng.random()
}

/// **************************************************************************
/// Name: esp_wifi_read_mac
///
/// Description:
///   Read MAC address from efuse
///
/// Input Parameters:
///   mac  - MAC address buffer pointer
///   type - MAC address type
///
/// Returned Value:
///   0 if success or -1 if fail
///
/// *************************************************************************
pub unsafe extern "C" fn read_mac(mac: *mut u8, type_: u32) -> c_int {
    trace!("read_mac {:?} {}", mac, type_);

    let base_mac = hal::efuse::Efuse::mac_address();

    for (i, &byte) in base_mac.iter().enumerate() {
        unsafe {
            mac.add(i).write_volatile(byte);
        }
    }

    const ESP_MAC_WIFI_SOFTAP: u32 = 1;
    const ESP_MAC_BT: u32 = 2;

    unsafe {
        if type_ == ESP_MAC_WIFI_SOFTAP {
            let tmp = mac.offset(0).read_volatile();
            for i in 0..64 {
                mac.offset(0).write_volatile(tmp | 0x02);
                mac.offset(0)
                    .write_volatile(mac.offset(0).read_volatile() ^ (i << 2));

                if mac.offset(0).read_volatile() != tmp {
                    break;
                }
            }
        } else if type_ == ESP_MAC_BT {
            let tmp = mac.offset(0).read_volatile();
            for i in 0..64 {
                mac.offset(0).write_volatile(tmp | 0x02);
                mac.offset(0)
                    .write_volatile(mac.offset(0).read_volatile() ^ (i << 2));

                if mac.offset(0).read_volatile() != tmp {
                    break;
                }
            }

            mac.offset(5)
                .write_volatile(mac.offset(5).read_volatile() + 1);
        } else {
            return -1;
        }
    }

    0
}

// other functions
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __esp_radio_puts(s: *const c_char) {
    WIFI_LOG_CALLBACK_COUNT.fetch_add(1, Ordering::Relaxed);
    if wifi_log_callback_suppressed() {
        let _ = s;
        return;
    }
    unsafe {
        let cstr = str_from_c(s);
        info!("{}", cstr);
    }
}

static WIFI_INIT_CB_TRACE_COUNT: AtomicU32 = AtomicU32::new(0);
static WIFI_LOG_CALLBACK_COUNT: AtomicU32 = AtomicU32::new(0);

fn wifi_init_runtime_trace_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_NEW_TRACE_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_INIT_TRACE"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(option_env!("ESP_RADIO_INIT_TRACE"), Some(_))
}

fn wifi_log_callback_suppressed() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_SUPPRESS_PUTS_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn wifi_init_runtime_trace(message: &str) {
    if wifi_init_runtime_trace_enabled() {
        let _ = message;
        WIFI_INIT_CB_TRACE_COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

// #define ESP_EVENT_DEFINE_BASE(id) esp_event_base_t id = #id
#[unsafe(no_mangle)]
static mut __ESP_RADIO_WIFI_EVENT: esp_event_base_t = c"WIFI_EVENT".as_ptr();

#[cfg(feature = "wifi")]
pub unsafe extern "C" fn ets_timer_disarm(timer: *mut c_void) {
    crate::compat::timer_compat::compat_timer_disarm(timer.cast());
}

#[cfg(feature = "wifi")]
pub unsafe extern "C" fn ets_timer_done(timer: *mut c_void) {
    crate::compat::timer_compat::compat_timer_done(timer.cast());
}

#[cfg(feature = "wifi")]
pub unsafe extern "C" fn ets_timer_setfn(
    ptimer: *mut c_void,
    pfunction: *mut c_void,
    parg: *mut c_void,
) {
    wifi_init_runtime_trace("ets_timer_setfn");
    unsafe {
        crate::compat::timer_compat::compat_timer_setfn(
            ptimer.cast(),
            core::mem::transmute::<*mut c_void, unsafe extern "C" fn(*mut c_void)>(pfunction),
            parg,
        );
    }
}

#[cfg(feature = "wifi")]
#[cfg(xtensa)]
fn current_timer_wrapper_caller_ptr() -> usize {
    let caller_ptr: usize;
    unsafe {
        core::arch::asm!("mov {0}, a0", out(reg) caller_ptr);
    }
    caller_ptr
}

#[cfg(feature = "wifi")]
#[cfg(not(xtensa))]
fn current_timer_wrapper_caller_ptr() -> usize {
    0
}

#[cfg(feature = "wifi")]
pub unsafe extern "C" fn ets_timer_arm(timer: *mut c_void, ms: u32, repeat: bool) {
    wifi_init_runtime_trace("ets_timer_arm");
    crate::compat::timer_compat::record_wrapper_arm_call(
        timer as usize,
        current_timer_wrapper_caller_ptr(),
        ms.saturating_mul(1000),
        repeat,
    );
    crate::compat::timer_compat::compat_timer_arm(timer.cast(), ms, repeat);
}

#[cfg(feature = "wifi")]
pub unsafe extern "C" fn ets_timer_arm_us(timer: *mut c_void, us: u32, repeat: bool) {
    wifi_init_runtime_trace("ets_timer_arm_us");
    crate::compat::timer_compat::record_wrapper_arm_call(
        timer as usize,
        current_timer_wrapper_caller_ptr(),
        us,
        repeat,
    );
    crate::compat::timer_compat::compat_timer_arm_us(timer.cast(), us, repeat);
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __esp_radio_gettimeofday(tv: *mut timeval, _tz: *mut ()) -> i32 {
    if !tv.is_null() {
        unsafe {
            let microseconds = __esp_radio_esp_timer_get_time();
            (*tv).tv_sec = (microseconds / 1_000_000) as u64;
            (*tv).tv_usec = (microseconds % 1_000_000) as u32;
        }
    }

    0
}

/// **************************************************************************
/// Name: esp_timer_get_time
///
/// Description:
///   Get time in microseconds since boot.
///
/// Returned Value:
///   System time in micros
///
/// *************************************************************************
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __esp_radio_esp_timer_get_time() -> i64 {
    trace!("esp_timer_get_time");
    if wifi_use_legacy_esp_timer_get_time_diag_enabled() {
        esp_hal::time::Instant::now()
            .duration_since_epoch()
            .as_micros() as i64
    } else {
        // Just using IEEE802.15.4 doesn't need the current time. If we don't use `preempt::now`,
        // users will not need to have a scheduler in their firmware.
        cfg_if::cfg_if! {
            if #[cfg(any(feature = "wifi", feature = "ble"))] {
                crate::preempt::now() as i64
            } else {
                // In this case we don't have a scheduler, we can return esp-hal's timestamp.
                esp_hal::time::Instant::now().duration_since_epoch().as_micros() as i64
            }
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __esp_radio_esp_fill_random(dst: *mut u8, len: u32) {
    trace!("esp_fill_random");
    unsafe {
        let dst = core::slice::from_raw_parts_mut(dst, len as usize);

        let rng = esp_hal::rng::Rng::new();
        for chunk in dst.chunks_mut(4) {
            let bytes = rng.random().to_le_bytes();
            chunk.copy_from_slice(&bytes[..chunk.len()]);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __esp_radio_strrchr(_s: *const (), _c: u32) -> *const u8 {
    todo!("strrchr");
}

#[unsafe(no_mangle)]
static mut __ESP_RADIO_G_LOG_LEVEL: i32 = 0;

#[unsafe(no_mangle)]
pub static mut __ESP_RADIO_G_MISC_NVS: *mut u32 = &raw mut NVS as *mut u32;

pub static mut NVS: [u32; 15] = [0u32; 15];

// For some reason these are only necessary on Xtensa chips.
#[cfg(xtensa)]
#[unsafe(no_mangle)]
unsafe extern "C" fn __esp_radio_misc_nvs_deinit() {
    trace!("misc_nvs_deinit")
}

#[cfg(xtensa)]
#[unsafe(no_mangle)]
unsafe extern "C" fn __esp_radio_misc_nvs_init() -> i32 {
    trace!("misc_nvs_init");
    0
}

#[cfg(xtensa)]
#[unsafe(no_mangle)]
unsafe extern "C" fn __esp_radio_misc_nvs_restore() -> i32 {
    todo!("misc_nvs_restore")
}

// We're use either WIFI or BT here, since esp-radio also supports the ESP32-H2 as the only
// chip, with BT but without WIFI.
#[cfg(not(esp32h2))]
type ModemClockControllerPeripheral = esp_hal::peripherals::WIFI<'static>;
#[cfg(esp32h2)]
type ModemClockControllerPeripheral = esp_hal::peripherals::BT<'static>;

// Clock control is no-op because the wifi blobs don't symmetrically enable/disable the clock,
// causing an eventual overflow. Currently we are holding onto a guard ourselves while Wi-Fi/BT is
// active, so the blobs should not be able to disable the clock anyway.
//
// This might have some low-power issues, but we're not there yet anyway.
#[allow(unused)]
pub(crate) unsafe fn phy_enable_clock() {
    PHY_COMMON_CLOCK_ENABLE_CALLS.fetch_add(1, Ordering::Relaxed);
    if !phy_common_clock_real_enable() {
        return;
    }

    let count = PHY_COMMON_CLOCK_ENABLE_REF.fetch_add(1, Ordering::Acquire);
    if count == 0 {
        // Stealing the peripheral is safe here, as they must have been passed into the relevant
        // initialization functions for the Wi-Fi or BLE controller, if this code gets executed.
        let clock_guard = unsafe { hal::peripherals::WIFI::steal() }.enable_phy_clock();
        core::mem::forget(clock_guard);
    }
}

#[allow(unused)]
pub(crate) unsafe fn phy_disable_clock() {
    PHY_COMMON_CLOCK_DISABLE_CALLS.fetch_add(1, Ordering::Relaxed);
    if !phy_common_clock_real_enable() {
        return;
    }

    let count = PHY_COMMON_CLOCK_ENABLE_REF.fetch_sub(1, Ordering::Release);
    if count == 1 {
        unsafe { hal::peripherals::WIFI::steal() }.decrease_phy_clock_ref_count();
    }
}

pub(crate) fn enable_wifi_power_domain() {
    #[cfg(not(any(soc_has_pmu, esp32c2)))]
    {
        cfg_if::cfg_if! {
            if #[cfg(soc_has_lpwr)] {
                let rtc_cntl = esp_hal::peripherals::LPWR::regs();
            } else {
                let rtc_cntl = esp_hal::peripherals::RTC_CNTL::regs();
            }
        }

        rtc_cntl
            .dig_pwc()
            .modify(|_, w| w.wifi_force_pd().clear_bit());

        #[cfg(not(esp32))]
        unsafe {
            cfg_if::cfg_if! {
                if #[cfg(soc_has_apb_ctrl)] {
                    let syscon = esp_hal::peripherals::APB_CTRL::regs();
                } else {
                    let syscon = esp_hal::peripherals::SYSCON::regs();
                }
            }
            const WIFIBB_RST: u32 = 1 << 0; // Wi-Fi baseband
            const FE_RST: u32 = 1 << 1; // RF Frontend RST
            const WIFIMAC_RST: u32 = 1 << 2; // Wi-Fi MAC

            const BTBB_RST: u32 = 1 << 3; // Bluetooth Baseband
            const BTMAC_RST: u32 = 1 << 4; // deprecated
            const RW_BTMAC_RST: u32 = 1 << 9; // Bluetooth MAC
            const RW_BTMAC_REG_RST: u32 = 1 << 11; // Bluetooth MAC Regsiters
            const BTBB_REG_RST: u32 = 1 << 13; // Bluetooth Baseband Registers

            const MODEM_RESET_FIELD_WHEN_PU: u32 = WIFIBB_RST
                | FE_RST
                | WIFIMAC_RST
                | if cfg!(soc_has_bt) {
                    BTBB_RST | BTMAC_RST | RW_BTMAC_RST | RW_BTMAC_REG_RST | BTBB_RST
                } else {
                    0
                };

            syscon
                .wifi_rst_en()
                .modify(|r, w| w.bits(r.bits() | MODEM_RESET_FIELD_WHEN_PU));
            syscon
                .wifi_rst_en()
                .modify(|r, w| w.bits(r.bits() & !MODEM_RESET_FIELD_WHEN_PU));
        }

        rtc_cntl
            .dig_iso()
            .modify(|_, w| w.wifi_force_iso().clear_bit());
    }
}

/// Get calibration data.
///
/// Returns the last calibration result.
///
/// If you see the data is different than what was persisted before, consider persisting the new
/// data.
pub fn phy_calibration_data(data: &mut [u8; esp_phy::PHY_CALIBRATION_DATA_LENGTH]) {
    // Although we're ignoring the result here, this doesn't change the behavior, as this just
    // doesn't do anything in case an error is returned.
    let _ = esp_phy::backup_phy_calibration_data(data);
}

/// Set calibration data.
///
/// This will be used next time the phy gets initialized.
pub fn set_phy_calibration_data(data: &[u8; core::mem::size_of::<esp_phy_calibration_data_t>()]) {
    // Although we're ignoring the result here, this doesn't change the behavior, as this just
    // doesn't do anything in case an error is returned.
    let _ = esp_phy::set_phy_calibration_data(data);
}

/// **************************************************************************
/// Name: esp_queue_create
///
/// Description:
///   Create message queue
///
/// Input Parameters:
///   queue_len - queue message number
///   item_size - message size
///
/// Returned Value:
///   Message queue data pointer
///
/// *************************************************************************
pub unsafe extern "C" fn queue_create(queue_len: u32, item_size: u32) -> *mut c_void {
    let queue = crate::compat::queue::queue_create(queue_len as i32, item_size as i32).cast();
    wifi_init_runtime_trace("queue_create.after");
    queue
}

/// **************************************************************************
/// Name: esp_queue_delete
///
/// Description:
///   Delete message queue
///
/// Input Parameters:
///   queue - Message queue data pointer
///
/// Returned Value:
///   None
///
/// *************************************************************************
pub unsafe extern "C" fn queue_delete(queue: *mut c_void) {
    crate::compat::queue::queue_delete(queue.cast());
}

/// **************************************************************************
/// Name: esp_queue_send
///
/// Description:
///   Send message of low priority to queue within a certain period of time
///
/// Input Parameters:
///   queue - Message queue data pointer
///   item  - Message data pointer
///   ticks - Wait ticks
///
/// Returned Value:
///   True if success or false if fail
///
/// *************************************************************************
pub unsafe extern "C" fn queue_send(
    queue: *mut c_void,
    item: *mut c_void,
    block_time_tick: u32,
) -> i32 {
    let (item_word0, item_pointee_word0, item_pointee_word1) =
        queue_send_item_snapshot(queue, item.cast_const());
    wifi_init_runtime_trace("queue_send.before");
    #[cfg(feature = "wifi")]
    {
        let ordinal = WIFI_OS_QUEUE_SEND_COUNT.fetch_add(1, Ordering::Relaxed);
        let task_ptr = current_task_ptr_for_diag();
        let (timer_callback_ptr, timer_arg_ptr) = current_timer_exec_snapshot();
        record_first_last_task_ptr(
            &WIFI_OS_QUEUE_SEND_FIRST_TASK_PTR,
            &WIFI_OS_QUEUE_SEND_LAST_TASK_PTR,
            &WIFI_OS_QUEUE_SEND_TASK_CHANGES,
            task_ptr,
        );
        record_queue_send_sample(
            &WIFI_OS_QUEUE_SEND_SAMPLE_QUEUES,
            &WIFI_OS_QUEUE_SEND_SAMPLE_TASKS,
            &WIFI_OS_QUEUE_SEND_SAMPLE_ITEM_WORD0,
            &WIFI_OS_QUEUE_SEND_SAMPLE_ITEM_POINTEE_WORD0,
            &WIFI_OS_QUEUE_SEND_SAMPLE_ITEM_POINTEE_WORD1,
            &WIFI_OS_QUEUE_SEND_SAMPLE_TIMER_CALLBACK_PTR,
            &WIFI_OS_QUEUE_SEND_SAMPLE_TIMER_ARG_PTR,
            ordinal,
            queue as usize,
            task_ptr,
            item_word0,
            item_pointee_word0,
            item_pointee_word1,
            timer_callback_ptr,
            timer_arg_ptr,
        );
        record_queue_send_recent_sample(
            &WIFI_OS_QUEUE_SEND_RECENT_ORDINALS,
            &WIFI_OS_QUEUE_SEND_RECENT_QUEUES,
            &WIFI_OS_QUEUE_SEND_RECENT_TASKS,
            &WIFI_OS_QUEUE_SEND_RECENT_ITEM_WORD0,
            &WIFI_OS_QUEUE_SEND_RECENT_ITEM_POINTEE_WORD0,
            &WIFI_OS_QUEUE_SEND_RECENT_ITEM_POINTEE_WORD1,
            &WIFI_OS_QUEUE_SEND_RECENT_TIMER_CALLBACK_PTR,
            &WIFI_OS_QUEUE_SEND_RECENT_TIMER_ARG_PTR,
            ordinal,
            queue as usize,
            task_ptr,
            item_word0,
            item_pointee_word0,
            item_pointee_word1,
            timer_callback_ptr,
            timer_arg_ptr,
        );
        record_queue_send_item_words(queue, item.cast_const());
    }
    let rc = crate::compat::queue::queue_send_to_back(
        queue.cast(),
        item.cast_const(),
        blob_ticks_to_micros(block_time_tick),
    );
    wifi_init_runtime_trace("queue_send.after");
    rc
}

/// **************************************************************************
/// Name: esp_queue_send_from_isr
///
/// Description:
///   Send message of low priority to queue in ISR within
///   a certain period of time
///
/// Input Parameters:
///   queue - Message queue data pointer
///   item  - Message data pointer
///   hptw  - No mean
///
/// Returned Value:
///   True if success or false if fail
///
/// *************************************************************************
pub unsafe extern "C" fn queue_send_from_isr(
    queue: *mut c_void,
    item: *mut c_void,
    higher_priority_task_waken: *mut c_void,
) -> i32 {
    #[cfg(feature = "wifi")]
    WIFI_OS_QUEUE_SEND_ISR_COUNT.fetch_add(1, Ordering::Relaxed);
    if wifi_use_legacy_queue_send_from_isr_diag_enabled() {
        #[cfg(feature = "wifi")]
        WIFI_OS_QUEUE_SEND_ISR_LEGACY_BRANCH_COUNT.fetch_add(1, Ordering::Relaxed);
        if !higher_priority_task_waken.is_null() {
            unsafe { *(higher_priority_task_waken as *mut u32) = 1 };
        }
        return unsafe { queue_send_to_back(queue, item, 1000) };
    }
    crate::compat::queue::queue_try_send_to_back_from_isr(
        queue.cast(),
        item.cast_const(),
        higher_priority_task_waken.cast(),
    )
}

/// **************************************************************************
/// Name: esp_queue_send_to_back
///
/// Description:
///   Send message of low priority to queue within a certain period of time
///
/// Input Parameters:
///   queue - Message queue data pointer
///   item  - Message data pointer
///   ticks - Wait ticks
///
/// Returned Value:
///   True if success or false if fail
///
/// *************************************************************************
pub unsafe extern "C" fn queue_send_to_back(
    queue: *mut c_void,
    item: *mut c_void,
    block_time_tick: u32,
) -> i32 {
    wifi_init_runtime_trace("queue_send_to_back.before");
    #[cfg(feature = "wifi")]
    {
        let ordinal = WIFI_OS_QUEUE_SEND_COUNT.fetch_add(1, Ordering::Relaxed);
        let task_ptr = current_task_ptr_for_diag();
        let (timer_callback_ptr, timer_arg_ptr) = current_timer_exec_snapshot();
        let (item_word0, item_pointee_word0, item_pointee_word1) =
            queue_send_item_snapshot(queue, item.cast_const());
        record_first_last_task_ptr(
            &WIFI_OS_QUEUE_SEND_FIRST_TASK_PTR,
            &WIFI_OS_QUEUE_SEND_LAST_TASK_PTR,
            &WIFI_OS_QUEUE_SEND_TASK_CHANGES,
            task_ptr,
        );
        record_queue_send_sample(
            &WIFI_OS_QUEUE_SEND_SAMPLE_QUEUES,
            &WIFI_OS_QUEUE_SEND_SAMPLE_TASKS,
            &WIFI_OS_QUEUE_SEND_SAMPLE_ITEM_WORD0,
            &WIFI_OS_QUEUE_SEND_SAMPLE_ITEM_POINTEE_WORD0,
            &WIFI_OS_QUEUE_SEND_SAMPLE_ITEM_POINTEE_WORD1,
            &WIFI_OS_QUEUE_SEND_SAMPLE_TIMER_CALLBACK_PTR,
            &WIFI_OS_QUEUE_SEND_SAMPLE_TIMER_ARG_PTR,
            ordinal,
            queue as usize,
            task_ptr,
            item_word0,
            item_pointee_word0,
            item_pointee_word1,
            timer_callback_ptr,
            timer_arg_ptr,
        );
        record_queue_send_recent_sample(
            &WIFI_OS_QUEUE_SEND_RECENT_ORDINALS,
            &WIFI_OS_QUEUE_SEND_RECENT_QUEUES,
            &WIFI_OS_QUEUE_SEND_RECENT_TASKS,
            &WIFI_OS_QUEUE_SEND_RECENT_ITEM_WORD0,
            &WIFI_OS_QUEUE_SEND_RECENT_ITEM_POINTEE_WORD0,
            &WIFI_OS_QUEUE_SEND_RECENT_ITEM_POINTEE_WORD1,
            &WIFI_OS_QUEUE_SEND_RECENT_TIMER_CALLBACK_PTR,
            &WIFI_OS_QUEUE_SEND_RECENT_TIMER_ARG_PTR,
            ordinal,
            queue as usize,
            task_ptr,
            item_word0,
            item_pointee_word0,
            item_pointee_word1,
            timer_callback_ptr,
            timer_arg_ptr,
        );
        record_queue_send_item_words(queue, item.cast_const());
    }
    let rc = crate::compat::queue::queue_send_to_back(
        queue.cast(),
        item,
        blob_ticks_to_micros(block_time_tick),
    );
    wifi_init_runtime_trace("queue_send_to_back.after");
    rc
}

/// **************************************************************************
/// Name: esp_queue_send_from_to_front
///
/// Description:
///   Send message of high priority to queue within a certain period of time
///
/// Input Parameters:
///   queue - Message queue data pointer
///   item  - Message data pointer
///   ticks - Wait ticks
///
/// Returned Value:
///   True if success or false if fail
///
/// *************************************************************************
pub unsafe extern "C" fn queue_send_to_front(
    queue: *mut c_void,
    item: *mut c_void,
    block_time_tick: u32,
) -> i32 {
    wifi_init_runtime_trace("queue_send_to_front.before");
    #[cfg(feature = "wifi")]
    {
        let ordinal = WIFI_OS_QUEUE_SEND_COUNT.fetch_add(1, Ordering::Relaxed);
        let task_ptr = current_task_ptr_for_diag();
        let (timer_callback_ptr, timer_arg_ptr) = current_timer_exec_snapshot();
        let (item_word0, item_pointee_word0, item_pointee_word1) =
            queue_send_item_snapshot(queue, item.cast_const());
        record_first_last_task_ptr(
            &WIFI_OS_QUEUE_SEND_FIRST_TASK_PTR,
            &WIFI_OS_QUEUE_SEND_LAST_TASK_PTR,
            &WIFI_OS_QUEUE_SEND_TASK_CHANGES,
            task_ptr,
        );
        record_queue_send_sample(
            &WIFI_OS_QUEUE_SEND_SAMPLE_QUEUES,
            &WIFI_OS_QUEUE_SEND_SAMPLE_TASKS,
            &WIFI_OS_QUEUE_SEND_SAMPLE_ITEM_WORD0,
            &WIFI_OS_QUEUE_SEND_SAMPLE_ITEM_POINTEE_WORD0,
            &WIFI_OS_QUEUE_SEND_SAMPLE_ITEM_POINTEE_WORD1,
            &WIFI_OS_QUEUE_SEND_SAMPLE_TIMER_CALLBACK_PTR,
            &WIFI_OS_QUEUE_SEND_SAMPLE_TIMER_ARG_PTR,
            ordinal,
            queue as usize,
            task_ptr,
            item_word0,
            item_pointee_word0,
            item_pointee_word1,
            timer_callback_ptr,
            timer_arg_ptr,
        );
        record_queue_send_recent_sample(
            &WIFI_OS_QUEUE_SEND_RECENT_ORDINALS,
            &WIFI_OS_QUEUE_SEND_RECENT_QUEUES,
            &WIFI_OS_QUEUE_SEND_RECENT_TASKS,
            &WIFI_OS_QUEUE_SEND_RECENT_ITEM_WORD0,
            &WIFI_OS_QUEUE_SEND_RECENT_ITEM_POINTEE_WORD0,
            &WIFI_OS_QUEUE_SEND_RECENT_ITEM_POINTEE_WORD1,
            &WIFI_OS_QUEUE_SEND_RECENT_TIMER_CALLBACK_PTR,
            &WIFI_OS_QUEUE_SEND_RECENT_TIMER_ARG_PTR,
            ordinal,
            queue as usize,
            task_ptr,
            item_word0,
            item_pointee_word0,
            item_pointee_word1,
            timer_callback_ptr,
            timer_arg_ptr,
        );
        record_queue_send_item_words(queue, item.cast_const());
    }
    let rc = crate::compat::queue::queue_send_to_front(
        queue.cast(),
        item,
        blob_ticks_to_micros(block_time_tick),
    );
    wifi_init_runtime_trace("queue_send_to_front.after");
    rc
}

/// **************************************************************************
/// Name: esp_queue_recv
///
/// Description:
///   Receive message from queue within a certain period of time
///
/// Input Parameters:
///   queue - Message queue data pointer
///   item  - Message data pointer
///   ticks - Wait ticks
///
/// Returned Value:
///   True if success or false if fail
///
/// *************************************************************************
pub unsafe extern "C" fn queue_recv(
    queue: *mut c_void,
    item: *mut c_void,
    block_time_ms: u32,
) -> i32 {
    wifi_init_runtime_trace("queue_recv.before");
    let rc =
        crate::compat::queue::queue_receive(queue.cast(), item, blob_ticks_to_micros(block_time_ms));
    wifi_init_runtime_trace("queue_recv.after");
    #[cfg(feature = "wifi")]
    if rc != 0 {
        let ordinal = WIFI_OS_QUEUE_RECV_COUNT.fetch_add(1, Ordering::Relaxed);
        let task_ptr = current_task_ptr_for_diag();
        let caller_ptr = current_queue_recv_caller_ptr();
        record_first_last_task_ptr(
            &WIFI_OS_QUEUE_RECV_FIRST_TASK_PTR,
            &WIFI_OS_QUEUE_RECV_LAST_TASK_PTR,
            &WIFI_OS_QUEUE_RECV_TASK_CHANGES,
            task_ptr,
        );
        record_queue_recv_item_words(queue, item);
        WIFI_OS_QUEUE_RECV_LAST_CALLER_PTR.store(caller_ptr, Ordering::Relaxed);
        record_queue_recv_sample(
            &WIFI_OS_QUEUE_RECV_SAMPLE_QUEUES,
            &WIFI_OS_QUEUE_RECV_SAMPLE_TASKS,
            &WIFI_OS_QUEUE_RECV_SAMPLE_ITEM_WORD0,
            &WIFI_OS_QUEUE_RECV_SAMPLE_ITEM_POINTEE_WORD0,
            &WIFI_OS_QUEUE_RECV_SAMPLE_ITEM_POINTEE_WORD1,
            ordinal,
            queue as usize,
            task_ptr,
            WIFI_OS_QUEUE_RECV_LAST_ITEM_WORD0.load(Ordering::Relaxed),
            WIFI_OS_QUEUE_RECV_LAST_ITEM_POINTEE_WORD0.load(Ordering::Relaxed),
            WIFI_OS_QUEUE_RECV_LAST_ITEM_POINTEE_WORD1.load(Ordering::Relaxed),
        );
        record_queue_recv_recent_sample(
            &WIFI_OS_QUEUE_RECV_RECENT_ORDINALS,
            &WIFI_OS_QUEUE_RECV_RECENT_QUEUES,
            &WIFI_OS_QUEUE_RECV_RECENT_TASKS,
            &WIFI_OS_QUEUE_RECV_RECENT_ITEM_WORD0,
            &WIFI_OS_QUEUE_RECV_RECENT_ITEM_POINTEE_WORD0,
            &WIFI_OS_QUEUE_RECV_RECENT_ITEM_POINTEE_WORD1,
            &WIFI_OS_QUEUE_RECV_RECENT_CALLER_PTR,
            ordinal,
            queue as usize,
            task_ptr,
            WIFI_OS_QUEUE_RECV_LAST_ITEM_WORD0.load(Ordering::Relaxed),
            WIFI_OS_QUEUE_RECV_LAST_ITEM_POINTEE_WORD0.load(Ordering::Relaxed),
            WIFI_OS_QUEUE_RECV_LAST_ITEM_POINTEE_WORD1.load(Ordering::Relaxed),
            caller_ptr,
        );
    }
    rc
}

pub unsafe extern "C" fn queue_recv_from_isr(
    queue: *mut c_void,
    item: *mut c_void,
    higher_priority_task_waken: *mut c_void,
) -> i32 {
    #[cfg(feature = "wifi")]
    WIFI_OS_QUEUE_RECV_ISR_COUNT.fetch_add(1, Ordering::Relaxed);
    crate::compat::queue::queue_try_receive_from_isr(
        queue.cast(),
        item,
        higher_priority_task_waken.cast(),
    )
}

/// **************************************************************************
/// Name: esp_queue_msg_waiting
///
/// Description:
///   Get message number in the message queue
///
/// Input Parameters:
///   queue - Message queue data pointer
///
/// Returned Value:
///   Message number
///
/// *************************************************************************
pub unsafe extern "C" fn queue_msg_waiting(queue: *mut c_void) -> u32 {
    crate::compat::queue::queue_messages_waiting(queue.cast())
}

#[allow(unused)]
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __esp_radio_esp_event_post(
    event_base: *const c_char,
    event_id: i32,
    event_data: *mut c_void,
    event_data_size: usize,
    ticks_to_wait: u32,
) -> i32 {
    #[cfg(feature = "wifi")]
    WIFI_OS_EVENT_POST_COUNT.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "wifi")]
    return unsafe {
        crate::wifi::event_post(
            event_base,
            event_id,
            event_data,
            event_data_size,
            ticks_to_wait,
        )
    };

    #[cfg(not(feature = "wifi"))]
    return -1;
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn __esp_radio_vTaskDelay(ticks: u32) {
    unsafe {
        crate::compat::common::__esp_radio_usleep(crate::time::blob_ticks_to_micros(ticks));
    }
}
