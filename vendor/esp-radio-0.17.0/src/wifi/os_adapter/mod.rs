#[cfg_attr(esp32, path = "esp32.rs")]
#[cfg_attr(esp32c2, path = "esp32c2.rs")]
#[cfg_attr(esp32c3, path = "esp32c3.rs")]
#[cfg_attr(esp32c6, path = "esp32c6.rs")]
#[cfg_attr(esp32h2, path = "esp32h2.rs")]
#[cfg_attr(esp32s2, path = "esp32s2.rs")]
#[cfg_attr(esp32s3, path = "esp32s3.rs")]
pub(crate) mod os_adapter_chip_specific;

use allocator_api2::boxed::Box;
use enumset::EnumSet;
use esp_phy::PhyController;
use esp_sync::{NonReentrantMutex, RawMutex};
use portable_atomic::{AtomicU32, AtomicUsize, Ordering};

const DIAG_RECENT_CAP: usize = 8;

use super::WifiEvent;
use crate::{
    binary::c_types::*,
    compat::{
        common::{str_from_c, thread_sem_get},
        malloc::{InternalMemory, calloc_internal},
    },
    hal::{clock::ModemClockController, peripherals::WIFI},
    memory_fence::memory_fence,
    time::{blob_ticks_to_micros, millis_to_blob_ticks},
};

fn wifi_init_runtime_trace_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_NEW_TRACE_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_INIT_TRACE"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(option_env!("ESP_RADIO_INIT_TRACE"), Some(_))
}

fn wifi_init_runtime_trace(message: &str) {
    if wifi_init_runtime_trace_enabled() {
        let _ = message;
    }
}

fn wifi_use_legacy_phy_enable_diag_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_PHY_ENABLE_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("ESP_RADIO_USE_LEGACY_PHY_ENABLE_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("MEDITAMER_WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn wifi_use_legacy_wifi_alloc_diag_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_WIFI_ALLOC_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("ESP_RADIO_USE_LEGACY_WIFI_ALLOC_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("MEDITAMER_WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn wifi_use_legacy_task_delay_diag_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_TASK_DELAY_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("ESP_RADIO_USE_LEGACY_TASK_DELAY_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("MEDITAMER_WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn wifi_use_legacy_coex_status_get_diag_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_COEX_STATUS_GET_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("ESP_RADIO_USE_LEGACY_COEX_STATUS_GET_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn wifi_use_legacy_task_yield_from_isr_diag_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_TASK_YIELD_FROM_ISR_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("ESP_RADIO_USE_LEGACY_TASK_YIELD_FROM_ISR_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn record_recent_ptr(
    ordinals: &[AtomicU32; DIAG_RECENT_CAP],
    ptrs: &[AtomicUsize; DIAG_RECENT_CAP],
    value: usize,
    count: u32,
) {
    let ordinal = count + 1;
    let idx = (ordinal as usize - 1) % DIAG_RECENT_CAP;
    ordinals[idx].store(ordinal, Ordering::Relaxed);
    ptrs[idx].store(value, Ordering::Relaxed);
}

static WIFI_LOCK: RawMutex = RawMutex::new();
static WIFI_SCAN_DONE_EVENTPOST_COUNT: AtomicU32 = AtomicU32::new(0);
static WIFI_SCAN_DONE_EVENTPOST_STATUS: AtomicU32 = AtomicU32::new(0);
static WIFI_SCAN_DONE_EVENTPOST_NUMBER: AtomicU32 = AtomicU32::new(0);
static WIFI_SCAN_DONE_EVENTPOST_ID: AtomicU32 = AtomicU32::new(0);
static WIFI_SCAN_DONE_EVENTPOST_AP_NUM_RC: AtomicU32 = AtomicU32::new(0);
static WIFI_SCAN_DONE_EVENTPOST_AP_NUM: AtomicU32 = AtomicU32::new(0);
static WIFI_THREAD_SEM_GET_COUNT: AtomicU32 = AtomicU32::new(0);
static WIFI_TASK_DELAY_COUNT: AtomicU32 = AtomicU32::new(0);
static WIFI_TASK_DELAY_MAX_TICK: AtomicU32 = AtomicU32::new(0);
static WIFI_TASK_MS_TO_TICK_COUNT: AtomicU32 = AtomicU32::new(0);
static WIFI_TASK_MS_TO_TICK_MAX_MS: AtomicU32 = AtomicU32::new(0);
static WIFI_EVENT_GROUP_CREATE_COUNT: AtomicU32 = AtomicU32::new(0);
static WIFI_EVENT_GROUP_SET_BITS_COUNT: AtomicU32 = AtomicU32::new(0);
static WIFI_EVENT_GROUP_CLEAR_BITS_COUNT: AtomicU32 = AtomicU32::new(0);
static WIFI_EVENT_GROUP_WAIT_BITS_COUNT: AtomicU32 = AtomicU32::new(0);
static WIFI_THREAD_SEM_LAST_PTR: AtomicUsize = AtomicUsize::new(0);
static WIFI_THREAD_SEM_FIRST_PTR: AtomicUsize = AtomicUsize::new(0);
static WIFI_THREAD_SEM_PTR_CHANGE_COUNT: AtomicU32 = AtomicU32::new(0);
static WIFI_THREAD_SEM_LAST_TASK_PTR: AtomicUsize = AtomicUsize::new(0);
static WIFI_THREAD_SEM_FIRST_TASK_PTR: AtomicUsize = AtomicUsize::new(0);
static WIFI_THREAD_SEM_TASK_CHANGE_COUNT: AtomicU32 = AtomicU32::new(0);
static WIFI_TASK_GET_CURRENT_TASK_LAST_PTR: AtomicUsize = AtomicUsize::new(0);
static WIFI_TASK_GET_CURRENT_TASK_FIRST_PTR: AtomicUsize = AtomicUsize::new(0);
static WIFI_TASK_GET_CURRENT_TASK_CHANGE_COUNT: AtomicU32 = AtomicU32::new(0);
static WIFI_TASK_GET_CURRENT_TASK_COUNT: AtomicU32 = AtomicU32::new(0);
static WIFI_TASK_GET_CURRENT_TASK_RECENT_ORDINALS: [AtomicU32; DIAG_RECENT_CAP] =
    [const { AtomicU32::new(0) }; DIAG_RECENT_CAP];
static WIFI_TASK_GET_CURRENT_TASK_RECENT_PTRS: [AtomicUsize; DIAG_RECENT_CAP] =
    [const { AtomicUsize::new(0) }; DIAG_RECENT_CAP];
static WIFI_OS_MALLOC_COUNT: AtomicU32 = AtomicU32::new(0);
static WIFI_OS_MALLOC_TOTAL_SIZE: AtomicU32 = AtomicU32::new(0);
static WIFI_OS_MALLOC_MAX_SIZE: AtomicU32 = AtomicU32::new(0);
static WIFI_OS_MALLOC_LAST_SIZE: AtomicU32 = AtomicU32::new(0);
static WIFI_OS_MALLOC_INTERNAL_COUNT: AtomicU32 = AtomicU32::new(0);
static WIFI_OS_MALLOC_INTERNAL_TOTAL_SIZE: AtomicU32 = AtomicU32::new(0);
static WIFI_OS_MALLOC_INTERNAL_MAX_SIZE: AtomicU32 = AtomicU32::new(0);
static WIFI_OS_MALLOC_INTERNAL_LAST_SIZE: AtomicU32 = AtomicU32::new(0);
static WIFI_OS_CALLOC_INTERNAL_COUNT: AtomicU32 = AtomicU32::new(0);
static WIFI_OS_CALLOC_INTERNAL_TOTAL_SIZE: AtomicU32 = AtomicU32::new(0);
static WIFI_OS_CALLOC_INTERNAL_MAX_SIZE: AtomicU32 = AtomicU32::new(0);
static WIFI_OS_CALLOC_INTERNAL_LAST_SIZE: AtomicU32 = AtomicU32::new(0);
static WIFI_OS_WIFI_MALLOC_COUNT: AtomicU32 = AtomicU32::new(0);
static WIFI_OS_WIFI_MALLOC_TOTAL_SIZE: AtomicU32 = AtomicU32::new(0);
static WIFI_OS_WIFI_MALLOC_MAX_SIZE: AtomicU32 = AtomicU32::new(0);
static WIFI_OS_WIFI_MALLOC_LAST_SIZE: AtomicU32 = AtomicU32::new(0);
static WIFI_OS_WIFI_CALLOC_COUNT: AtomicU32 = AtomicU32::new(0);
static WIFI_OS_WIFI_CALLOC_TOTAL_SIZE: AtomicU32 = AtomicU32::new(0);
static WIFI_OS_WIFI_CALLOC_MAX_SIZE: AtomicU32 = AtomicU32::new(0);
static WIFI_OS_WIFI_CALLOC_LAST_SIZE: AtomicU32 = AtomicU32::new(0);
static WIFI_OS_FREE_COUNT: AtomicU32 = AtomicU32::new(0);
static WIFI_OS_FREE_LAST_PTR: AtomicUsize = AtomicUsize::new(0);
static WIFI_PHY_ENABLE_COUNT: AtomicU32 = AtomicU32::new(0);
static WIFI_PHY_DISABLE_COUNT: AtomicU32 = AtomicU32::new(0);
static WIFI_TASK_CREATE_COUNT: AtomicU32 = AtomicU32::new(0);
static WIFI_TASK_CREATE_RECENT_ORDINALS: [AtomicU32; DIAG_RECENT_CAP] =
    [const { AtomicU32::new(0) }; DIAG_RECENT_CAP];
static WIFI_TASK_CREATE_RECENT_TASK_FUNC_PTRS: [AtomicUsize; DIAG_RECENT_CAP] =
    [const { AtomicUsize::new(0) }; DIAG_RECENT_CAP];
static WIFI_TASK_CREATE_RECENT_NAME_TAGS: [AtomicU32; DIAG_RECENT_CAP] =
    [const { AtomicU32::new(0) }; DIAG_RECENT_CAP];
static WIFI_TASK_CREATE_RECENT_NAME_LENS: [AtomicU32; DIAG_RECENT_CAP] =
    [const { AtomicU32::new(0) }; DIAG_RECENT_CAP];
static WIFI_TASK_CREATE_RECENT_STACK_DEPTHS: [AtomicU32; DIAG_RECENT_CAP] =
    [const { AtomicU32::new(0) }; DIAG_RECENT_CAP];
static WIFI_TASK_CREATE_RECENT_PARAM_PTRS: [AtomicUsize; DIAG_RECENT_CAP] =
    [const { AtomicUsize::new(0) }; DIAG_RECENT_CAP];
static WIFI_TASK_CREATE_RECENT_PRIOS: [AtomicU32; DIAG_RECENT_CAP] =
    [const { AtomicU32::new(0) }; DIAG_RECENT_CAP];
static WIFI_TASK_CREATE_RECENT_CORE_IDS: [AtomicU32; DIAG_RECENT_CAP] =
    [const { AtomicU32::new(0) }; DIAG_RECENT_CAP];
static WIFI_TASK_CREATE_RECENT_TASK_PTRS: [AtomicUsize; DIAG_RECENT_CAP] =
    [const { AtomicUsize::new(0) }; DIAG_RECENT_CAP];

#[derive(Clone, Copy)]
pub struct WifiScanDoneEventPostDiag {
    pub count: u32,
    pub status: u32,
    pub number: u32,
    pub scan_id: u32,
    pub ap_num_rc: u32,
    pub ap_num: u32,
}

#[derive(Clone, Copy)]
pub struct WifiAdapterPrimitiveDiag {
    pub thread_sem_get_count: u32,
    pub thread_sem_first_ptr: usize,
    pub thread_sem_last_ptr: usize,
    pub thread_sem_ptr_change_count: u32,
    pub thread_sem_first_task_ptr: usize,
    pub thread_sem_last_task_ptr: usize,
    pub thread_sem_task_change_count: u32,
    pub task_delay_count: u32,
    pub task_delay_max_tick: u32,
    pub task_ms_to_tick_count: u32,
    pub task_ms_to_tick_max_ms: u32,
    pub task_get_current_task_count: u32,
    pub task_get_current_task_first_ptr: usize,
    pub task_get_current_task_last_ptr: usize,
    pub task_get_current_task_change_count: u32,
    pub task_get_current_task_recent_ordinals: [u32; DIAG_RECENT_CAP],
    pub task_get_current_task_recent_ptrs: [usize; DIAG_RECENT_CAP],
    pub event_group_create_count: u32,
    pub event_group_set_bits_count: u32,
    pub event_group_clear_bits_count: u32,
    pub event_group_wait_bits_count: u32,
    pub malloc_count: u32,
    pub malloc_total_size: u32,
    pub malloc_max_size: u32,
    pub malloc_last_size: u32,
    pub malloc_internal_count: u32,
    pub malloc_internal_total_size: u32,
    pub malloc_internal_max_size: u32,
    pub malloc_internal_last_size: u32,
    pub calloc_internal_count: u32,
    pub calloc_internal_total_size: u32,
    pub calloc_internal_max_size: u32,
    pub calloc_internal_last_size: u32,
    pub wifi_malloc_count: u32,
    pub wifi_malloc_total_size: u32,
    pub wifi_malloc_max_size: u32,
    pub wifi_malloc_last_size: u32,
    pub wifi_calloc_count: u32,
    pub wifi_calloc_total_size: u32,
    pub wifi_calloc_max_size: u32,
    pub wifi_calloc_last_size: u32,
    pub free_count: u32,
    pub free_last_ptr: usize,
    pub phy_enable_count: u32,
    pub phy_disable_count: u32,
}

#[derive(Clone, Copy)]
pub struct WifiTaskCreateDiag {
    pub count: u32,
    pub recent_ordinals: [u32; DIAG_RECENT_CAP],
    pub recent_task_func_ptrs: [usize; DIAG_RECENT_CAP],
    pub recent_name_tags: [u32; DIAG_RECENT_CAP],
    pub recent_name_lens: [u32; DIAG_RECENT_CAP],
    pub recent_stack_depths: [u32; DIAG_RECENT_CAP],
    pub recent_param_ptrs: [usize; DIAG_RECENT_CAP],
    pub recent_prios: [u32; DIAG_RECENT_CAP],
    pub recent_core_ids: [u32; DIAG_RECENT_CAP],
    pub recent_task_ptrs: [usize; DIAG_RECENT_CAP],
}

fn encode_name_tag(name: &str) -> u32 {
    let bytes = name.as_bytes();
    let mut tag = 0u32;
    let mut idx = 0usize;
    while idx < bytes.len() && idx < 4 {
        tag |= (bytes[idx] as u32) << (idx * 8);
        idx += 1;
    }
    tag
}

fn record_wifi_task_create(
    task_func: usize,
    task_name: &str,
    stack_depth: u32,
    param: usize,
    prio: u32,
    core_id: Option<u32>,
    task_ptr: usize,
) {
    let ordinal = WIFI_TASK_CREATE_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    let idx = (ordinal as usize - 1) % DIAG_RECENT_CAP;
    WIFI_TASK_CREATE_RECENT_ORDINALS[idx].store(ordinal, Ordering::Relaxed);
    WIFI_TASK_CREATE_RECENT_TASK_FUNC_PTRS[idx].store(task_func, Ordering::Relaxed);
    WIFI_TASK_CREATE_RECENT_NAME_TAGS[idx].store(encode_name_tag(task_name), Ordering::Relaxed);
    WIFI_TASK_CREATE_RECENT_NAME_LENS[idx]
        .store(task_name.len().min(u32::MAX as usize) as u32, Ordering::Relaxed);
    WIFI_TASK_CREATE_RECENT_STACK_DEPTHS[idx].store(stack_depth, Ordering::Relaxed);
    WIFI_TASK_CREATE_RECENT_PARAM_PTRS[idx].store(param, Ordering::Relaxed);
    WIFI_TASK_CREATE_RECENT_PRIOS[idx].store(prio, Ordering::Relaxed);
    WIFI_TASK_CREATE_RECENT_CORE_IDS[idx].store(core_id.unwrap_or(u32::MAX), Ordering::Relaxed);
    WIFI_TASK_CREATE_RECENT_TASK_PTRS[idx].store(task_ptr, Ordering::Relaxed);
}

pub fn reset_wifi_scan_done_eventpost_diag() {
    WIFI_SCAN_DONE_EVENTPOST_COUNT.store(0, Ordering::Relaxed);
    WIFI_SCAN_DONE_EVENTPOST_STATUS.store(0, Ordering::Relaxed);
    WIFI_SCAN_DONE_EVENTPOST_NUMBER.store(0, Ordering::Relaxed);
    WIFI_SCAN_DONE_EVENTPOST_ID.store(0, Ordering::Relaxed);
    WIFI_SCAN_DONE_EVENTPOST_AP_NUM_RC.store(0, Ordering::Relaxed);
    WIFI_SCAN_DONE_EVENTPOST_AP_NUM.store(0, Ordering::Relaxed);
    WIFI_THREAD_SEM_GET_COUNT.store(0, Ordering::Relaxed);
    WIFI_TASK_DELAY_COUNT.store(0, Ordering::Relaxed);
    WIFI_TASK_DELAY_MAX_TICK.store(0, Ordering::Relaxed);
    WIFI_TASK_MS_TO_TICK_COUNT.store(0, Ordering::Relaxed);
    WIFI_TASK_MS_TO_TICK_MAX_MS.store(0, Ordering::Relaxed);
    WIFI_EVENT_GROUP_CREATE_COUNT.store(0, Ordering::Relaxed);
    WIFI_EVENT_GROUP_SET_BITS_COUNT.store(0, Ordering::Relaxed);
    WIFI_EVENT_GROUP_CLEAR_BITS_COUNT.store(0, Ordering::Relaxed);
    WIFI_EVENT_GROUP_WAIT_BITS_COUNT.store(0, Ordering::Relaxed);
    WIFI_THREAD_SEM_LAST_PTR.store(0, Ordering::Relaxed);
    WIFI_THREAD_SEM_FIRST_PTR.store(0, Ordering::Relaxed);
    WIFI_THREAD_SEM_PTR_CHANGE_COUNT.store(0, Ordering::Relaxed);
    WIFI_THREAD_SEM_LAST_TASK_PTR.store(0, Ordering::Relaxed);
    WIFI_THREAD_SEM_FIRST_TASK_PTR.store(0, Ordering::Relaxed);
    WIFI_THREAD_SEM_TASK_CHANGE_COUNT.store(0, Ordering::Relaxed);
    WIFI_TASK_GET_CURRENT_TASK_LAST_PTR.store(0, Ordering::Relaxed);
    WIFI_TASK_GET_CURRENT_TASK_FIRST_PTR.store(0, Ordering::Relaxed);
    WIFI_TASK_GET_CURRENT_TASK_CHANGE_COUNT.store(0, Ordering::Relaxed);
    WIFI_TASK_GET_CURRENT_TASK_COUNT.store(0, Ordering::Relaxed);
    for idx in 0..DIAG_RECENT_CAP {
        WIFI_TASK_GET_CURRENT_TASK_RECENT_ORDINALS[idx].store(0, Ordering::Relaxed);
        WIFI_TASK_GET_CURRENT_TASK_RECENT_PTRS[idx].store(0, Ordering::Relaxed);
    }
    WIFI_OS_MALLOC_COUNT.store(0, Ordering::Relaxed);
    WIFI_OS_MALLOC_TOTAL_SIZE.store(0, Ordering::Relaxed);
    WIFI_OS_MALLOC_MAX_SIZE.store(0, Ordering::Relaxed);
    WIFI_OS_MALLOC_LAST_SIZE.store(0, Ordering::Relaxed);
    WIFI_OS_MALLOC_INTERNAL_COUNT.store(0, Ordering::Relaxed);
    WIFI_OS_MALLOC_INTERNAL_TOTAL_SIZE.store(0, Ordering::Relaxed);
    WIFI_OS_MALLOC_INTERNAL_MAX_SIZE.store(0, Ordering::Relaxed);
    WIFI_OS_MALLOC_INTERNAL_LAST_SIZE.store(0, Ordering::Relaxed);
    WIFI_OS_CALLOC_INTERNAL_COUNT.store(0, Ordering::Relaxed);
    WIFI_OS_CALLOC_INTERNAL_TOTAL_SIZE.store(0, Ordering::Relaxed);
    WIFI_OS_CALLOC_INTERNAL_MAX_SIZE.store(0, Ordering::Relaxed);
    WIFI_OS_CALLOC_INTERNAL_LAST_SIZE.store(0, Ordering::Relaxed);
    WIFI_OS_WIFI_MALLOC_COUNT.store(0, Ordering::Relaxed);
    WIFI_OS_WIFI_MALLOC_TOTAL_SIZE.store(0, Ordering::Relaxed);
    WIFI_OS_WIFI_MALLOC_MAX_SIZE.store(0, Ordering::Relaxed);
    WIFI_OS_WIFI_MALLOC_LAST_SIZE.store(0, Ordering::Relaxed);
    WIFI_OS_WIFI_CALLOC_COUNT.store(0, Ordering::Relaxed);
    WIFI_OS_WIFI_CALLOC_TOTAL_SIZE.store(0, Ordering::Relaxed);
    WIFI_OS_WIFI_CALLOC_MAX_SIZE.store(0, Ordering::Relaxed);
    WIFI_OS_WIFI_CALLOC_LAST_SIZE.store(0, Ordering::Relaxed);
    WIFI_OS_FREE_COUNT.store(0, Ordering::Relaxed);
    WIFI_OS_FREE_LAST_PTR.store(0, Ordering::Relaxed);
    WIFI_PHY_ENABLE_COUNT.store(0, Ordering::Relaxed);
    WIFI_PHY_DISABLE_COUNT.store(0, Ordering::Relaxed);
}

pub fn reset_wifi_task_create_diag() {
    WIFI_TASK_CREATE_COUNT.store(0, Ordering::Relaxed);
    for idx in 0..DIAG_RECENT_CAP {
        WIFI_TASK_CREATE_RECENT_ORDINALS[idx].store(0, Ordering::Relaxed);
        WIFI_TASK_CREATE_RECENT_TASK_FUNC_PTRS[idx].store(0, Ordering::Relaxed);
        WIFI_TASK_CREATE_RECENT_NAME_TAGS[idx].store(0, Ordering::Relaxed);
        WIFI_TASK_CREATE_RECENT_NAME_LENS[idx].store(0, Ordering::Relaxed);
        WIFI_TASK_CREATE_RECENT_STACK_DEPTHS[idx].store(0, Ordering::Relaxed);
        WIFI_TASK_CREATE_RECENT_PARAM_PTRS[idx].store(0, Ordering::Relaxed);
        WIFI_TASK_CREATE_RECENT_PRIOS[idx].store(0, Ordering::Relaxed);
        WIFI_TASK_CREATE_RECENT_CORE_IDS[idx].store(0, Ordering::Relaxed);
        WIFI_TASK_CREATE_RECENT_TASK_PTRS[idx].store(0, Ordering::Relaxed);
    }
}

pub fn wifi_scan_done_eventpost_diag() -> WifiScanDoneEventPostDiag {
    WifiScanDoneEventPostDiag {
        count: WIFI_SCAN_DONE_EVENTPOST_COUNT.load(Ordering::Relaxed),
        status: WIFI_SCAN_DONE_EVENTPOST_STATUS.load(Ordering::Relaxed),
        number: WIFI_SCAN_DONE_EVENTPOST_NUMBER.load(Ordering::Relaxed),
        scan_id: WIFI_SCAN_DONE_EVENTPOST_ID.load(Ordering::Relaxed),
        ap_num_rc: WIFI_SCAN_DONE_EVENTPOST_AP_NUM_RC.load(Ordering::Relaxed),
        ap_num: WIFI_SCAN_DONE_EVENTPOST_AP_NUM.load(Ordering::Relaxed),
    }
}

pub fn wifi_adapter_primitive_diag() -> WifiAdapterPrimitiveDiag {
    WifiAdapterPrimitiveDiag {
        thread_sem_get_count: WIFI_THREAD_SEM_GET_COUNT.load(Ordering::Relaxed),
        thread_sem_first_ptr: WIFI_THREAD_SEM_FIRST_PTR.load(Ordering::Relaxed),
        thread_sem_last_ptr: WIFI_THREAD_SEM_LAST_PTR.load(Ordering::Relaxed),
        thread_sem_ptr_change_count: WIFI_THREAD_SEM_PTR_CHANGE_COUNT.load(Ordering::Relaxed),
        thread_sem_first_task_ptr: WIFI_THREAD_SEM_FIRST_TASK_PTR.load(Ordering::Relaxed),
        thread_sem_last_task_ptr: WIFI_THREAD_SEM_LAST_TASK_PTR.load(Ordering::Relaxed),
        thread_sem_task_change_count: WIFI_THREAD_SEM_TASK_CHANGE_COUNT.load(Ordering::Relaxed),
        task_delay_count: WIFI_TASK_DELAY_COUNT.load(Ordering::Relaxed),
        task_delay_max_tick: WIFI_TASK_DELAY_MAX_TICK.load(Ordering::Relaxed),
        task_ms_to_tick_count: WIFI_TASK_MS_TO_TICK_COUNT.load(Ordering::Relaxed),
        task_ms_to_tick_max_ms: WIFI_TASK_MS_TO_TICK_MAX_MS.load(Ordering::Relaxed),
        task_get_current_task_count: WIFI_TASK_GET_CURRENT_TASK_COUNT.load(Ordering::Relaxed),
        task_get_current_task_first_ptr: WIFI_TASK_GET_CURRENT_TASK_FIRST_PTR.load(Ordering::Relaxed),
        task_get_current_task_last_ptr: WIFI_TASK_GET_CURRENT_TASK_LAST_PTR.load(Ordering::Relaxed),
        task_get_current_task_change_count: WIFI_TASK_GET_CURRENT_TASK_CHANGE_COUNT
            .load(Ordering::Relaxed),
        task_get_current_task_recent_ordinals: core::array::from_fn(|idx| {
            WIFI_TASK_GET_CURRENT_TASK_RECENT_ORDINALS[idx].load(Ordering::Relaxed)
        }),
        task_get_current_task_recent_ptrs: core::array::from_fn(|idx| {
            WIFI_TASK_GET_CURRENT_TASK_RECENT_PTRS[idx].load(Ordering::Relaxed)
        }),
        event_group_create_count: WIFI_EVENT_GROUP_CREATE_COUNT.load(Ordering::Relaxed),
        event_group_set_bits_count: WIFI_EVENT_GROUP_SET_BITS_COUNT.load(Ordering::Relaxed),
        event_group_clear_bits_count: WIFI_EVENT_GROUP_CLEAR_BITS_COUNT.load(Ordering::Relaxed),
        event_group_wait_bits_count: WIFI_EVENT_GROUP_WAIT_BITS_COUNT.load(Ordering::Relaxed),
        malloc_count: WIFI_OS_MALLOC_COUNT.load(Ordering::Relaxed),
        malloc_total_size: WIFI_OS_MALLOC_TOTAL_SIZE.load(Ordering::Relaxed),
        malloc_max_size: WIFI_OS_MALLOC_MAX_SIZE.load(Ordering::Relaxed),
        malloc_last_size: WIFI_OS_MALLOC_LAST_SIZE.load(Ordering::Relaxed),
        malloc_internal_count: WIFI_OS_MALLOC_INTERNAL_COUNT.load(Ordering::Relaxed),
        malloc_internal_total_size: WIFI_OS_MALLOC_INTERNAL_TOTAL_SIZE.load(Ordering::Relaxed),
        malloc_internal_max_size: WIFI_OS_MALLOC_INTERNAL_MAX_SIZE.load(Ordering::Relaxed),
        malloc_internal_last_size: WIFI_OS_MALLOC_INTERNAL_LAST_SIZE.load(Ordering::Relaxed),
        calloc_internal_count: WIFI_OS_CALLOC_INTERNAL_COUNT.load(Ordering::Relaxed),
        calloc_internal_total_size: WIFI_OS_CALLOC_INTERNAL_TOTAL_SIZE.load(Ordering::Relaxed),
        calloc_internal_max_size: WIFI_OS_CALLOC_INTERNAL_MAX_SIZE.load(Ordering::Relaxed),
        calloc_internal_last_size: WIFI_OS_CALLOC_INTERNAL_LAST_SIZE.load(Ordering::Relaxed),
        wifi_malloc_count: WIFI_OS_WIFI_MALLOC_COUNT.load(Ordering::Relaxed),
        wifi_malloc_total_size: WIFI_OS_WIFI_MALLOC_TOTAL_SIZE.load(Ordering::Relaxed),
        wifi_malloc_max_size: WIFI_OS_WIFI_MALLOC_MAX_SIZE.load(Ordering::Relaxed),
        wifi_malloc_last_size: WIFI_OS_WIFI_MALLOC_LAST_SIZE.load(Ordering::Relaxed),
        wifi_calloc_count: WIFI_OS_WIFI_CALLOC_COUNT.load(Ordering::Relaxed),
        wifi_calloc_total_size: WIFI_OS_WIFI_CALLOC_TOTAL_SIZE.load(Ordering::Relaxed),
        wifi_calloc_max_size: WIFI_OS_WIFI_CALLOC_MAX_SIZE.load(Ordering::Relaxed),
        wifi_calloc_last_size: WIFI_OS_WIFI_CALLOC_LAST_SIZE.load(Ordering::Relaxed),
        free_count: WIFI_OS_FREE_COUNT.load(Ordering::Relaxed),
        free_last_ptr: WIFI_OS_FREE_LAST_PTR.load(Ordering::Relaxed),
        phy_enable_count: WIFI_PHY_ENABLE_COUNT.load(Ordering::Relaxed),
        phy_disable_count: WIFI_PHY_DISABLE_COUNT.load(Ordering::Relaxed),
    }
}

pub fn wifi_task_create_diag() -> WifiTaskCreateDiag {
    WifiTaskCreateDiag {
        count: WIFI_TASK_CREATE_COUNT.load(Ordering::Relaxed),
        recent_ordinals: core::array::from_fn(|idx| {
            WIFI_TASK_CREATE_RECENT_ORDINALS[idx].load(Ordering::Relaxed)
        }),
        recent_task_func_ptrs: core::array::from_fn(|idx| {
            WIFI_TASK_CREATE_RECENT_TASK_FUNC_PTRS[idx].load(Ordering::Relaxed)
        }),
        recent_name_tags: core::array::from_fn(|idx| {
            WIFI_TASK_CREATE_RECENT_NAME_TAGS[idx].load(Ordering::Relaxed)
        }),
        recent_name_lens: core::array::from_fn(|idx| {
            WIFI_TASK_CREATE_RECENT_NAME_LENS[idx].load(Ordering::Relaxed)
        }),
        recent_stack_depths: core::array::from_fn(|idx| {
            WIFI_TASK_CREATE_RECENT_STACK_DEPTHS[idx].load(Ordering::Relaxed)
        }),
        recent_param_ptrs: core::array::from_fn(|idx| {
            WIFI_TASK_CREATE_RECENT_PARAM_PTRS[idx].load(Ordering::Relaxed)
        }),
        recent_prios: core::array::from_fn(|idx| {
            WIFI_TASK_CREATE_RECENT_PRIOS[idx].load(Ordering::Relaxed)
        }),
        recent_core_ids: core::array::from_fn(|idx| {
            WIFI_TASK_CREATE_RECENT_CORE_IDS[idx].load(Ordering::Relaxed)
        }),
        recent_task_ptrs: core::array::from_fn(|idx| {
            WIFI_TASK_CREATE_RECENT_TASK_PTRS[idx].load(Ordering::Relaxed)
        }),
    }
}

fn record_first_last_ptr(first: &AtomicUsize, last: &AtomicUsize, changes: &AtomicU32, ptr: usize) {
    if ptr == 0 {
        return;
    }
    let previous = last.swap(ptr, Ordering::Relaxed);
    if previous == 0 {
        first.store(ptr, Ordering::Relaxed);
        return;
    }
    if previous != ptr {
        changes.fetch_add(1, Ordering::Relaxed);
    }
}

fn record_alloc(
    count: &AtomicU32,
    total_size: &AtomicU32,
    max_size: &AtomicU32,
    last_size: &AtomicU32,
    size: usize,
) {
    let size_u32 = size.min(u32::MAX as usize) as u32;
    count.fetch_add(1, Ordering::Relaxed);
    total_size.fetch_add(size_u32, Ordering::Relaxed);
    let mut current = max_size.load(Ordering::Relaxed);
    while size_u32 > current {
        match max_size.compare_exchange_weak(
            current,
            size_u32,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }
    last_size.store(size_u32, Ordering::Relaxed);
}

const fn wifi_use_real_isr_check_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_USE_REAL_ISR_CHECK"),
        Some(_)
    ) || matches!(option_env!("ESP_RADIO_USE_REAL_ISR_CHECK"), Some(_))
}

// useful for waiting for events - clear and wait for the event bit to be set
// again
pub(crate) static WIFI_EVENTS: NonReentrantMutex<EnumSet<WifiEvent>> =
    NonReentrantMutex::new(enumset::enum_set!());

/// **************************************************************************
/// Name: wifi_env_is_chip
///
/// Description:
///   Config chip environment
///
/// Returned Value:
///   True if on chip or false if on FPGA.
///
/// *************************************************************************
pub unsafe extern "C" fn env_is_chip() -> bool {
    true
}

/// **************************************************************************
/// Name: wifi_set_intr
///
/// Description:
///   Do nothing
///
/// Input Parameters:
///     cpu_no      - The CPU which the interrupt number belongs.
///     intr_source - The interrupt hardware source number.
///     intr_num    - The interrupt number CPU.
///     intr_prio   - The interrupt priority.
///
/// Returned Value:
///     None
///
/// *************************************************************************
pub unsafe extern "C" fn set_intr(cpu_no: i32, intr_source: u32, intr_num: u32, intr_prio: i32) {
    trace!(
        "set_intr {} {} {} {}",
        cpu_no, intr_source, intr_num, intr_prio
    );
    unsafe {
        crate::wifi::os_adapter::os_adapter_chip_specific::set_intr(
            cpu_no,
            intr_source,
            intr_num,
            intr_prio,
        );
    }
}

/// **************************************************************************
/// Name: wifi_clear_intr
///
/// Description:
///   Don't support
///
/// *************************************************************************
pub unsafe extern "C" fn clear_intr(intr_source: u32, intr_num: u32) {
    // original code does nothing here
    trace!("clear_intr called {} {}", intr_source, intr_num);
}

pub static mut ISR_INTERRUPT_1: (*mut c_void, *mut c_void) =
    (core::ptr::null_mut(), core::ptr::null_mut());

/// **************************************************************************
/// Name: esp32c3_ints_on
///
/// Description:
///   Enable Wi-Fi interrupt
///
/// Input Parameters:
///   mask - No mean
///
/// Returned Value:
///   None
///
/// *************************************************************************
pub unsafe extern "C" fn ints_on(mask: u32) {
    trace!("chip_ints_on {:x}", mask);

    crate::wifi::os_adapter::os_adapter_chip_specific::chip_ints_on(mask);
}

/// **************************************************************************
/// Name: esp32c3_ints_off
///
/// Description:
///   Disable Wi-Fi interrupt
///
/// Input Parameters:
///   mask - No mean
///
/// Returned Value:
///   None
///
/// *************************************************************************
pub unsafe extern "C" fn ints_off(mask: u32) {
    trace!("chip_ints_off {:x}", mask);

    crate::wifi::os_adapter::os_adapter_chip_specific::chip_ints_off(mask);
}

/// **************************************************************************
/// Name: wifi_is_from_isr
///
/// Description:
///   Check current is in interrupt
///
/// Input Parameters:
///   None
///
/// Returned Value:
///   true if in interrupt or false if not
///
/// *************************************************************************
pub unsafe extern "C" fn is_from_isr() -> bool {
    if wifi_use_real_isr_check_enabled() {
        crate::is_interrupts_disabled()
    } else {
        true
    }
}

/// **************************************************************************
/// Name: esp_spin_lock_create
///
/// Description:
///   Create spin lock in SMP mode
///
/// Input Parameters:
///   None
///
/// Returned Value:
///   Spin lock data pointer
///
/// *************************************************************************
pub unsafe extern "C" fn spin_lock_create() -> *mut c_void {
    let ptr = crate::compat::semaphore::sem_create(1, 1);

    trace!("spin_lock_create {:?}", ptr);
    ptr as *mut c_void
}

/// **************************************************************************
/// Name: esp_spin_lock_delete
///
/// Description:
///   Delete spin lock
///
/// Input Parameters:
///   lock - Spin lock data pointer
///
/// Returned Value:
///   None
///
/// *************************************************************************
pub unsafe extern "C" fn spin_lock_delete(lock: *mut c_void) {
    trace!("spin_lock_delete {:?}", lock);

    crate::compat::semaphore::sem_delete(lock);
}

/// **************************************************************************
/// Name: esp_wifi_int_disable
///
/// Description:
///   Enter critical section by disabling interrupts and taking the spin lock
///   if in SMP mode.
///
/// Input Parameters:
///   wifi_int_mux - Spin lock data pointer
///
/// Returned Value:
///   CPU PS value.
///
/// *************************************************************************
pub unsafe extern "C" fn wifi_int_disable(_wifi_int_mux: *mut c_void) -> u32 {
    trace!("wifi_int_disable");
    // TODO: can we use wifi_int_mux?
    let token = unsafe { WIFI_LOCK.acquire() };
    unsafe { core::mem::transmute::<esp_sync::RestoreState, u32>(token) }
}

/// **************************************************************************
/// Name: esp_wifi_int_restore
///
/// Description:
///   Exit from critical section by enabling interrupts and releasing the spin
///   lock if in SMP mode.
///
/// Input Parameters:
///   wifi_int_mux - Spin lock data pointer
///   tmp          - CPU PS value.
///
/// Returned Value:
///   None
///
/// *************************************************************************
pub unsafe extern "C" fn wifi_int_restore(_wifi_int_mux: *mut c_void, tmp: u32) {
    trace!("wifi_int_restore");
    let token = unsafe { core::mem::transmute::<u32, esp_sync::RestoreState>(tmp) };
    unsafe { WIFI_LOCK.release(token) }
}

/// **************************************************************************
/// Name: esp_task_yield_from_isr
///
/// Description:
///   Do nothing in NuttX
///
/// Input Parameters:
///   None
///
/// Returned Value:
///   None
///
/// *************************************************************************
pub unsafe extern "C" fn task_yield_from_isr() {
    trace!("task_yield_from_isr");
    if wifi_use_legacy_task_yield_from_isr_diag_enabled() {
        crate::preempt::yield_task();
    } else {
        crate::preempt::yield_task_from_isr();
    }
}

/// **************************************************************************
/// Name: esp_thread_semphr_get
///
/// Description:
///   Get thread self's semaphore
///
/// Input Parameters:
///   None
///
/// Returned Value:
///   Semaphore data pointer
///
/// *************************************************************************
pub unsafe extern "C" fn wifi_thread_semphr_get() -> *mut c_void {
    wifi_init_runtime_trace("wifi_thread_semphr_get.before");
    WIFI_THREAD_SEM_GET_COUNT.fetch_add(1, Ordering::Relaxed);
    let task_ptr = crate::preempt::current_task() as *mut c_void;
    let sem_ptr = thread_sem_get();
    record_first_last_ptr(
        &WIFI_THREAD_SEM_FIRST_TASK_PTR,
        &WIFI_THREAD_SEM_LAST_TASK_PTR,
        &WIFI_THREAD_SEM_TASK_CHANGE_COUNT,
        task_ptr as usize,
    );
    record_first_last_ptr(
        &WIFI_THREAD_SEM_FIRST_PTR,
        &WIFI_THREAD_SEM_LAST_PTR,
        &WIFI_THREAD_SEM_PTR_CHANGE_COUNT,
        sem_ptr as usize,
    );
    wifi_init_runtime_trace("wifi_thread_semphr_get.after");
    sem_ptr
}

/// **************************************************************************
/// Name: esp_mutex_create
///
/// Description:
///   Create mutex
///
/// Input Parameters:
///   None
///
/// Returned Value:
///   Mutex data pointer
///
/// *************************************************************************
pub unsafe extern "C" fn mutex_create() -> *mut c_void {
    let mutex = crate::compat::mutex::mutex_create(false);
    wifi_init_runtime_trace("mutex_create");
    trace!("mutex_create");
    mutex
}

/// **************************************************************************
/// Name: esp_recursive_mutex_create
///
/// Description:
///   Create recursive mutex
///
/// Input Parameters:
///   None
///
/// Returned Value:
///   Recursive mutex data pointer
///
/// *************************************************************************
pub unsafe extern "C" fn recursive_mutex_create() -> *mut c_void {
    let mutex = crate::compat::mutex::mutex_create(true);
    wifi_init_runtime_trace("recursive_mutex_create");
    trace!("recursive_mutex_create");
    mutex
}

/// **************************************************************************
/// Name: esp_mutex_delete
///
/// Description:
///   Delete mutex
///
/// Input Parameters:
///   mutex_data - mutex data pointer
///
/// Returned Value:
///   None
///
/// *************************************************************************
pub unsafe extern "C" fn mutex_delete(mutex: *mut c_void) {
    crate::compat::mutex::mutex_delete(mutex);
}

/// **************************************************************************
/// Name: esp_mutex_lock
///
/// Description:
///   Lock mutex
///
/// Input Parameters:
///   mutex_data - mutex data pointer
///
/// Returned Value:
///   True if success or false if fail
///
/// *************************************************************************
pub unsafe extern "C" fn mutex_lock(mutex: *mut c_void) -> i32 {
    wifi_init_runtime_trace("mutex_lock");
    crate::compat::mutex::mutex_lock(mutex)
}

/// **************************************************************************
/// Name: esp_mutex_unlock
///
/// Description:
///   Unlock mutex
///
/// Input Parameters:
///   mutex_data - mutex data pointer
///
/// Returned Value:
///   True if success or false if fail
///
/// *************************************************************************
pub unsafe extern "C" fn mutex_unlock(mutex: *mut c_void) -> i32 {
    wifi_init_runtime_trace("mutex_unlock");
    crate::compat::mutex::mutex_unlock(mutex)
}

/// **************************************************************************
/// Name: esp_event_group_create
///
/// Description:
///   Don't support
///
/// *************************************************************************
pub unsafe extern "C" fn event_group_create() -> *mut c_void {
    WIFI_EVENT_GROUP_CREATE_COUNT.fetch_add(1, Ordering::Relaxed);
    todo!("event_group_create")
}

/// **************************************************************************
/// Name: esp_event_group_delete
///
/// Description:
///   Don't support
///
/// *************************************************************************
pub unsafe extern "C" fn event_group_delete(_event: *mut c_void) {
    todo!("event_group_delete")
}

/// **************************************************************************
/// Name: esp_event_group_set_bits
///
/// Description:
///   Don't support
///
/// *************************************************************************
pub unsafe extern "C" fn event_group_set_bits(_event: *mut c_void, _bits: u32) -> u32 {
    WIFI_EVENT_GROUP_SET_BITS_COUNT.fetch_add(1, Ordering::Relaxed);
    todo!("event_group_set_bits")
}

/// **************************************************************************
/// Name: esp_event_group_clear_bits
///
/// Description:
///   Don't support
///
/// *************************************************************************
pub unsafe extern "C" fn event_group_clear_bits(_event: *mut c_void, _bits: u32) -> u32 {
    WIFI_EVENT_GROUP_CLEAR_BITS_COUNT.fetch_add(1, Ordering::Relaxed);
    todo!("event_group_clear_bits")
}

/// **************************************************************************
/// Name: esp_event_group_wait_bits
///
/// Description:
///   Don't support
///
/// *************************************************************************
pub unsafe extern "C" fn event_group_wait_bits(
    _event: *mut c_void,
    _bits_to_wait_for: u32,
    _clear_on_exit: c_int,
    _wait_for_all_bits: c_int,
    _block_time_tick: u32,
) -> u32 {
    WIFI_EVENT_GROUP_WAIT_BITS_COUNT.fetch_add(1, Ordering::Relaxed);
    todo!("event_group_wait_bits")
}

fn atomic_update_max(target: &AtomicU32, candidate: u32) {
    let mut current = target.load(Ordering::Relaxed);
    while candidate > current {
        match target.compare_exchange(current, candidate, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }
}

fn common_task_create(
    task_func: *mut c_void,
    name: *const c_char,
    stack_depth: u32,
    param: *mut c_void,
    prio: u32,
    task_handle: *mut c_void,
    core_id: Option<u32>,
) -> i32 {
    wifi_init_runtime_trace("common_task_create.before");
    let task_name = unsafe { str_from_c(name as _) };
    trace!(
        "task_create task_func {:?} name {} stack_depth {} param {:?} prio {}, task_handle {:?} core_id {:?}",
        task_func, task_name, stack_depth, param, prio, task_handle, core_id
    );

    unsafe {
        let task_func = core::mem::transmute::<
            *mut c_void,
            extern "C" fn(*mut esp_wifi_sys::c_types::c_void),
        >(task_func);

        let task = crate::preempt::task_create(
            task_name,
            task_func,
            param,
            prio,
            core_id,
            stack_depth as usize,
        );
        record_wifi_task_create(
            task_func as usize,
            task_name,
            stack_depth,
            param as usize,
            prio,
            core_id,
            task as usize,
        );
        *(task_handle as *mut usize) = task as usize;
        wifi_init_runtime_trace("common_task_create.after");

        1
    }
}

/// **************************************************************************
/// Name: esp_task_create_pinned_to_core
///
/// Description:
///   Create task and bind it to target CPU, the task will run when it
///   is created
///
/// Input Parameters:
///   entry       - Task entry
///   name        - Task name
///   stack_depth - Task stack size
///   param       - Task private data
///   prio        - Task priority
///   task_handle - Task handle pointer which is used to pause, resume
///                 and delete the task
///   core_id     - CPU which the task runs in
///
/// Returned Value:
///   True if success or false if fail
///
/// *************************************************************************
pub unsafe extern "C" fn task_create_pinned_to_core(
    task_func: *mut c_void,
    name: *const c_char,
    stack_depth: u32,
    param: *mut c_void,
    prio: u32,
    task_handle: *mut c_void,
    core_id: u32,
) -> i32 {
    common_task_create(
        task_func,
        name,
        stack_depth,
        param,
        prio,
        task_handle,
        if core_id < 2 { Some(core_id) } else { None },
    )
}

/// **************************************************************************
/// Name: esp_task_create
///
/// Description:
///   Create task and the task will run when it is created
///
/// Input Parameters:
///   entry       - Task entry
///   name        - Task name
///   stack_depth - Task stack size
///   param       - Task private data
///   prio        - Task priority
///   task_handle - Task handle pointer which is used to pause, resume
///                 and delete the task
///
/// Returned Value:
///   True if success or false if fail
///
/// *************************************************************************
pub unsafe extern "C" fn task_create(
    task_func: *mut c_void,
    name: *const c_char,
    stack_depth: u32,
    param: *mut c_void,
    prio: u32,
    task_handle: *mut c_void,
) -> i32 {
    common_task_create(task_func, name, stack_depth, param, prio, task_handle, None)
}

/// **************************************************************************
/// Name: esp_task_delete
///
/// Description:
///   Delete the target task
///
/// Input Parameters:
///   task_handle - Task handle pointer which is used to pause, resume
///                 and delete the task
///
/// Returned Value:
///   None
///
/// *************************************************************************
pub unsafe extern "C" fn task_delete(task_handle: *mut c_void) {
    trace!("task delete called for {:?}", task_handle);

    unsafe {
        crate::preempt::schedule_task_deletion(task_handle);
    }
}

/// **************************************************************************
/// Name: esp_task_delay
///
/// Description:
///   Current task wait for some ticks
///
/// Input Parameters:
///   tick - Waiting ticks
///
/// Returned Value:
///   None
///
/// *************************************************************************
pub unsafe extern "C" fn task_delay(tick: u32) {
    wifi_init_runtime_trace("task_delay.before");
    trace!("task_delay tick {}", tick);
    WIFI_TASK_DELAY_COUNT.fetch_add(1, Ordering::Relaxed);
    atomic_update_max(&WIFI_TASK_DELAY_MAX_TICK, tick);
    if wifi_use_legacy_task_delay_diag_enabled() {
        let wait_us = u64::from(blob_ticks_to_micros(tick));
        let start_us = esp_hal::time::Instant::now().duration_since_epoch().as_micros();
        while esp_hal::time::Instant::now()
            .duration_since_epoch()
            .as_micros()
            .saturating_sub(start_us)
            < wait_us
        {
            crate::preempt::yield_task();
        }
    } else {
        crate::preempt::usleep(blob_ticks_to_micros(tick))
    }
    wifi_init_runtime_trace("task_delay.after");
}

/// **************************************************************************
/// Name: esp_task_ms_to_tick
///
/// Description:
///   Transform from milliseconds to system ticks
///
/// Input Parameters:
///   ms - Milliseconds
///
/// Returned Value:
///   System ticks
///
/// *************************************************************************
pub unsafe extern "C" fn task_ms_to_tick(ms: u32) -> i32 {
    wifi_init_runtime_trace("task_ms_to_tick.before");
    trace!("task_ms_to_tick ms {}", ms);
    WIFI_TASK_MS_TO_TICK_COUNT.fetch_add(1, Ordering::Relaxed);
    atomic_update_max(&WIFI_TASK_MS_TO_TICK_MAX_MS, ms);
    let ticks = millis_to_blob_ticks(ms) as i32;
    wifi_init_runtime_trace("task_ms_to_tick.after");
    ticks
}

/// **************************************************************************
/// Name: esp_task_get_current_task
///
/// Description:
///   Retrieves the current task
///
/// Returned Value:
///   A pointer to the current task
///
/// *************************************************************************
pub unsafe extern "C" fn task_get_current_task() -> *mut c_void {
    let res = crate::preempt::current_task() as *mut c_void;
    let count = WIFI_TASK_GET_CURRENT_TASK_COUNT.fetch_add(1, Ordering::Relaxed);
    record_first_last_ptr(
        &WIFI_TASK_GET_CURRENT_TASK_FIRST_PTR,
        &WIFI_TASK_GET_CURRENT_TASK_LAST_PTR,
        &WIFI_TASK_GET_CURRENT_TASK_CHANGE_COUNT,
        res as usize,
    );
    record_recent_ptr(
        &WIFI_TASK_GET_CURRENT_TASK_RECENT_ORDINALS,
        &WIFI_TASK_GET_CURRENT_TASK_RECENT_PTRS,
        res as usize,
        count,
    );
    trace!("task get current task - return {:?}", res);

    res
}

/// **************************************************************************
/// Name: esp_task_get_max_priority
///
/// Description:
///   Get OS task maximum priority
///
/// Input Parameters:
///   None
///
/// Returned Value:
///   Task maximum priority
///
/// *************************************************************************
pub unsafe extern "C" fn task_get_max_priority() -> i32 {
    trace!("task_get_max_priority");
    crate::preempt::max_task_priority() as i32
}

/// **************************************************************************
/// Name: esp_malloc
///
/// Description:
///   Allocate a block of memory
///
/// Input Parameters:
///   size - memory size
///
/// Returned Value:
///   Memory pointer
///
/// *************************************************************************
pub unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
    record_alloc(
        &WIFI_OS_MALLOC_COUNT,
        &WIFI_OS_MALLOC_TOTAL_SIZE,
        &WIFI_OS_MALLOC_MAX_SIZE,
        &WIFI_OS_MALLOC_LAST_SIZE,
        size,
    );
    unsafe { crate::compat::malloc::malloc(size).cast() }
}

/// **************************************************************************
/// Name: esp_free
///
/// Description:
///   Free a block of memory
///
/// Input Parameters:
///   ptr - memory block
///
/// Returned Value:
///   No
///
/// *************************************************************************
pub unsafe extern "C" fn free(p: *mut c_void) {
    WIFI_OS_FREE_COUNT.fetch_add(1, Ordering::Relaxed);
    WIFI_OS_FREE_LAST_PTR.store(p as usize, Ordering::Relaxed);
    unsafe {
        crate::compat::malloc::free(p.cast());
    }
}

/// **************************************************************************
/// Name: esp_event_post
///
/// Description:
///   Active work queue and let the work to process the cached event
///
/// Input Parameters:
///   event_base      - Event set name
///   event_id        - Event ID
///   event_data      - Event private data
///   event_data_size - Event data size
///   ticks           - Waiting system ticks
///
/// Returned Value:
///   0 if success or -1 if fail
///
/// *************************************************************************
pub unsafe extern "C" fn event_post(
    event_base: *const c_char,
    event_id: i32,
    event_data: *mut c_void,
    event_data_size: usize,
    ticks_to_wait: u32,
) -> i32 {
    trace!(
        "event_post {:?} {} {:?} {} {:?}",
        event_base, event_id, event_data, event_data_size, ticks_to_wait
    );
    use num_traits::FromPrimitive;

    let event = unwrap!(WifiEvent::from_i32(event_id));
    trace!("EVENT: {:?}", event);

    if matches!(event, WifiEvent::ScanDone) {
        let mut ap_num: u16 = 0;
        let ap_num_rc = unsafe { crate::binary::include::esp_wifi_scan_get_ap_num(&mut ap_num) };
        let scan = unsafe {
            &*(event_data.cast::<crate::binary::include::wifi_event_sta_scan_done_t>())
        };
        WIFI_SCAN_DONE_EVENTPOST_COUNT.fetch_add(1, Ordering::Relaxed);
        WIFI_SCAN_DONE_EVENTPOST_STATUS.store(scan.status, Ordering::Relaxed);
        WIFI_SCAN_DONE_EVENTPOST_NUMBER.store(u32::from(scan.number), Ordering::Relaxed);
        WIFI_SCAN_DONE_EVENTPOST_ID.store(u32::from(scan.scan_id), Ordering::Relaxed);
        WIFI_SCAN_DONE_EVENTPOST_AP_NUM_RC.store(ap_num_rc as u32, Ordering::Relaxed);
        WIFI_SCAN_DONE_EVENTPOST_AP_NUM.store(u32::from(ap_num), Ordering::Relaxed);
    }

    WIFI_EVENTS.with(|events| events.insert(event));

    let handled =
        unsafe { super::event::dispatch_event_handler(event, event_data, event_data_size) };

    super::state::update_state(event, handled);

    event.waker().wake();

    match event {
        WifiEvent::StaConnected | WifiEvent::StaDisconnected => {
            crate::wifi::embassy::STA_LINK_STATE_WAKER.wake();
        }

        WifiEvent::ApStart | WifiEvent::ApStop => {
            crate::wifi::embassy::AP_LINK_STATE_WAKER.wake();
        }

        _ => {}
    }

    memory_fence();

    0
}

/// **************************************************************************
/// Name: esp_get_free_heap_size
///
/// Description:
///   Get free heap size by byte
///
/// Input Parameters:
///   None
///
/// Returned Value:
///   Free heap size
///
/// *************************************************************************
pub unsafe extern "C" fn get_free_heap_size() -> u32 {
    unsafe { crate::compat::malloc::get_free_internal_heap_size() as u32 }
}

/// **************************************************************************
/// Name: esp_rand
///
/// Description:
///   Get random data of type uint32_t
///
/// Input Parameters:
///   None
///
/// Returned Value:
///   Random data
///
/// *************************************************************************
pub unsafe extern "C" fn rand() -> u32 {
    unsafe { crate::common_adapter::random() }
}

/// **************************************************************************
/// Name: esp_dport_access_stall_other_cpu_start
///
/// Description:
///   Don't support
///
/// *************************************************************************
pub unsafe extern "C" fn dport_access_stall_other_cpu_start_wrap() {
    trace!("dport_access_stall_other_cpu_start_wrap")
}

/// **************************************************************************
/// Name: esp_dport_access_stall_other_cpu_end
///
/// Description:
///   Don't support
///
/// *************************************************************************
pub unsafe extern "C" fn dport_access_stall_other_cpu_end_wrap() {
    trace!("dport_access_stall_other_cpu_end_wrap")
}
/// **************************************************************************
/// Name: wifi_apb80m_request
///
/// Description:
///   Take Wi-Fi lock in auto-sleep
///
/// *************************************************************************
pub unsafe extern "C" fn wifi_apb80m_request() {
    trace!("wifi_apb80m_request - no-op")
}
/// **************************************************************************
/// Name: wifi_apb80m_release
///
/// Description:
///   Release Wi-Fi lock in auto-sleep
///
/// *************************************************************************
pub unsafe extern "C" fn wifi_apb80m_release() {
    trace!("wifi_apb80m_release - no-op")
}

/// **************************************************************************
/// Name: esp32c3_phy_disable
///
/// Description:
///   Deinitialize PHY hardware
///
/// Input Parameters:
///   None
///
/// Returned Value:
///   None
///
/// *************************************************************************
pub unsafe extern "C" fn phy_disable() {
    trace!("phy_disable");
    WIFI_PHY_DISABLE_COUNT.fetch_add(1, Ordering::Relaxed);
    #[cfg(esp32)]
    if wifi_use_legacy_phy_enable_diag_enabled() {
        unsafe { crate::wifi::phy_legacy_esp32::phy_disable() };
        return;
    }
    unsafe { WIFI::steal() }.decrease_phy_ref_count();
}

/// **************************************************************************
/// Name: esp32c3_phy_enable
///
/// Description:
///   Initialize PHY hardware
///
/// Input Parameters:
///   None
///
/// Returned Value:
///   None
///
/// *************************************************************************
pub unsafe extern "C" fn phy_enable() {
    // quite some code needed here
    trace!("phy_enable");
    WIFI_PHY_ENABLE_COUNT.fetch_add(1, Ordering::Relaxed);
    #[cfg(esp32)]
    if wifi_use_legacy_phy_enable_diag_enabled() {
        unsafe { crate::wifi::phy_legacy_esp32::phy_enable() };
        return;
    }
    core::mem::forget(unsafe { WIFI::steal() }.enable_phy());
}

/// **************************************************************************
/// Name: wifi_phy_update_country_info
///
/// Description:
///   Don't support
///
/// *************************************************************************
#[allow(clippy::unnecessary_cast)]
pub unsafe extern "C" fn phy_update_country_info(country: *const c_char) -> c_int {
    unsafe {
        // not implemented in original code
        trace!("phy_update_country_info {}", str_from_c(country.cast()));
        -1
    }
}

/// **************************************************************************
/// Name: wifi_reset_mac
///
/// Description:
///   Reset Wi-Fi hardware MAC
///
/// Input Parameters:
///   None
///
/// Returned Value:
///   None
///
/// *************************************************************************
pub unsafe extern "C" fn wifi_reset_mac() {
    trace!("wifi_reset_mac");
    // stealing WIFI is safe, since it is passed into the initialization function of the BLE
    // controller.
    unsafe { WIFI::steal() }.reset_wifi_mac();
}

/// **************************************************************************
/// Name: wifi_clock_enable
///
/// Description:
///   Enable Wi-Fi clock
///
/// Input Parameters:
///   None
///
/// Returned Value:
///   None
///
/// *************************************************************************
pub unsafe extern "C" fn wifi_clock_enable() {
    trace!("wifi_clock_enable");
    // stealing WIFI is safe, since it is passed into the initialization function of the BLE
    // controller.
    unsafe { WIFI::steal() }.enable_modem_clock(true);
}

/// **************************************************************************
/// Name: wifi_clock_disable
///
/// Description:
///   Disable Wi-Fi clock
///
/// Input Parameters:
///   None
///
/// Returned Value:
///   None
///
/// *************************************************************************
pub unsafe extern "C" fn wifi_clock_disable() {
    trace!("wifi_clock_disable");
    // stealing WIFI is safe, since it is passed into the initialization function of the BLE
    // controller.
    unsafe { WIFI::steal() }.enable_modem_clock(false);
}

/// **************************************************************************
/// Name: wifi_rtc_enable_iso
///
/// Description:
///   Don't support
///
/// *************************************************************************
pub unsafe extern "C" fn wifi_rtc_enable_iso() {
    todo!("wifi_rtc_enable_iso")
}

/// **************************************************************************
/// Name: wifi_rtc_disable_iso
///
/// Description:
///   Don't support
///
/// *************************************************************************
pub unsafe extern "C" fn wifi_rtc_disable_iso() {
    todo!("wifi_rtc_disable_iso")
}

/// **************************************************************************
/// Name: esp_nvs_set_i8
///
/// Description:
///   Save data of type int8_t into file system
///
/// Input Parameters:
///   handle - NVS handle
///   key    - Data index
///   value  - Stored data
///
/// Returned Value:
///   0 if success or -1 if fail
///
/// *************************************************************************
pub unsafe extern "C" fn nvs_set_i8(_handle: u32, _key: *const c_char, _value: i8) -> c_int {
    debug!("nvs_set_i8");
    -1
}

/// **************************************************************************
/// Name: esp_nvs_get_i8
///
/// Description:
///   Read data of type int8_t from file system
///
/// Input Parameters:
///   handle    - NVS handle
///   key       - Data index
///   out_value - Read buffer pointer
///
/// Returned Value:
///   0 if success or -1 if fail
///
/// *************************************************************************
pub unsafe extern "C" fn nvs_get_i8(
    _handle: u32,
    _key: *const c_char,
    _out_value: *mut i8,
) -> c_int {
    todo!("nvs_get_i8")
}

/// **************************************************************************
/// Name: esp_nvs_set_u8
///
/// Description:
///   Save data of type uint8_t into file system
///
/// Input Parameters:
///   handle - NVS handle
///   key    - Data index
///   value  - Stored data
///
/// Returned Value:
///   0 if success or -1 if fail
///
/// *************************************************************************
pub unsafe extern "C" fn nvs_set_u8(_handle: u32, _key: *const c_char, _value: u8) -> c_int {
    todo!("nvs_set_u8")
}

/// **************************************************************************
/// Name: esp_nvs_get_u8
///
/// Description:
///   Read data of type uint8_t from file system
///
/// Input Parameters:
///   handle    - NVS handle
///   key       - Data index
///   out_value - Read buffer pointer
///
/// Returned Value:
///   0 if success or -1 if fail
///
/// *************************************************************************
pub unsafe extern "C" fn nvs_get_u8(
    _handle: u32,
    _key: *const c_char,
    _out_value: *mut u8,
) -> c_int {
    todo!("nvs_get_u8")
}

/// **************************************************************************
/// Name: esp_nvs_set_u16
///
/// Description:
///   Save data of type uint16_t into file system
///
/// Input Parameters:
///   handle - NVS handle
///   key    - Data index
///   value  - Stored data
///
/// Returned Value:
///   0 if success or -1 if fail
///
/// *************************************************************************
pub unsafe extern "C" fn nvs_set_u16(_handle: u32, _key: *const c_char, _value: u16) -> c_int {
    todo!("nvs_set_u16")
}

/// **************************************************************************
/// Name: esp_nvs_get_u16
///
/// Description:
///   Read data of type uint16_t from file system
///
/// Input Parameters:
///   handle    - NVS handle
///   key       - Data index
///   out_value - Read buffer pointer
///
/// Returned Value:
///   0 if success or -1 if fail
///
/// *************************************************************************
pub unsafe extern "C" fn nvs_get_u16(
    _handle: u32,
    _key: *const c_char,
    _out_value: *mut u16,
) -> c_int {
    todo!("nvs_get_u16")
}

/// **************************************************************************
/// Name: esp_nvs_open
///
/// Description:
///   Create a file system storage data object
///
/// Input Parameters:
///   name       - Storage index
///   open_mode  - Storage mode
///   out_handle - Storage handle
///
/// Returned Value:
///   0 if success or -1 if fail
///
/// *************************************************************************
pub unsafe extern "C" fn nvs_open(
    _name: *const c_char,
    _open_mode: u32,
    _out_handle: *mut u32,
) -> c_int {
    todo!("nvs_open")
}

/// **************************************************************************
/// Name: esp_nvs_close
///
/// Description:
///   Close storage data object and free resource
///
/// Input Parameters:
///   handle - NVS handle
///
/// Returned Value:
///   0 if success or -1 if fail
///
/// *************************************************************************
pub unsafe extern "C" fn nvs_close(_handle: u32) {
    todo!("nvs_close")
}

/// **************************************************************************
/// Name: esp_nvs_commit
///
/// Description:
///   This function has no practical effect
///
/// *************************************************************************
pub unsafe extern "C" fn nvs_commit(_handle: u32) -> c_int {
    todo!("nvs_commit")
}

/// **************************************************************************
/// Name: esp_nvs_set_blob
///
/// Description:
///   Save a block of data into file system
///
/// Input Parameters:
///   handle - NVS handle
///   key    - Data index
///   value  - Stored buffer pointer
///   length - Buffer length
///
/// Returned Value:
///   0 if success or -1 if fail
///
/// *************************************************************************
pub unsafe extern "C" fn nvs_set_blob(
    _handle: u32,
    _key: *const c_char,
    _value: *const c_void,
    _length: usize,
) -> c_int {
    todo!("nvs_set_blob")
}

/// **************************************************************************
/// Name: esp_nvs_get_blob
///
/// Description:
///   Read a block of data from file system
///
/// Input Parameters:
///   handle    - NVS handle
///   key       - Data index
///   out_value - Read buffer pointer
///   length    - Buffer length
///
/// Returned Value:
///   0 if success or -1 if fail
///
/// *************************************************************************
pub unsafe extern "C" fn nvs_get_blob(
    _handle: u32,
    _key: *const c_char,
    _out_value: *mut c_void,
    _length: *mut usize,
) -> c_int {
    todo!("nvs_get_blob")
}

/// **************************************************************************
/// Name: esp_nvs_erase_key
///
/// Description:
///   Read a block of data from file system
///
/// Input Parameters:
///   handle    - NVS handle
///   key       - Data index
///
/// Returned Value:
///   0 if success or -1 if fail
///
/// *************************************************************************
pub unsafe extern "C" fn nvs_erase_key(_handle: u32, _key: *const c_char) -> c_int {
    todo!("nvs_erase_key")
}

/// **************************************************************************
/// Name: esp_get_random
///
/// Description:
///   Fill random data int given buffer of given length
///
/// Input Parameters:
///   buf - buffer pointer
///   len - buffer length
///
/// Returned Value:
///   0 if success or -1 if fail
///
/// *************************************************************************
pub unsafe extern "C" fn get_random(buf: *mut u8, len: usize) -> c_int {
    trace!("get_random");
    unsafe {
        crate::common_adapter::__esp_radio_esp_fill_random(buf, len as u32);
    }
    0
}

/// **************************************************************************
/// Name: esp_get_time
///
/// Description:
///   Get std C time
///
/// Input Parameters:
///   t - buffer to store time of type timeval
///
/// Returned Value:
///   0 if success or -1 if fail
///
/// *************************************************************************
pub unsafe extern "C" fn get_time(_t: *mut c_void) -> c_int {
    todo!("get_time")
}

/// **************************************************************************
/// Name: esp_log_write
///
/// Description:
///   Output log with by format string and its arguments
///
/// Input Parameters:
///   level  - log level, no mean here
///   tag    - log TAG, no mean here
///   format - format string
///
/// Returned Value:
///   None
///
/// *************************************************************************
#[cfg(feature = "sys-logs")]
pub unsafe extern "C" fn log_write(
    level: u32,
    _tag: *const c_char,
    format: *const c_char,
    args: ...
) {
    unsafe {
        crate::binary::log::syslog(level, format as _, args);
    }
}

/// **************************************************************************
/// Name: esp_log_writev
///
/// Description:
///   Output log with by format string and its arguments
///
/// Input Parameters:
///   level  - log level, no mean here
///   tag    - log TAG, no mean here
///   format - format string
///   args   - arguments list
///
/// Returned Value:
///   None
///
/// *************************************************************************
#[cfg(feature = "sys-logs")]
#[allow(improper_ctypes_definitions)]
pub unsafe extern "C" fn log_writev(
    level: u32,
    _tag: *const c_char,
    format: *const c_char,
    args: crate::binary::include::va_list,
) {
    unsafe {
        crate::binary::log::syslog(
            level,
            format as _,
            core::mem::transmute::<crate::binary::include::va_list, core::ffi::VaListImpl<'_>>(
                args,
            ),
        );
    }
}

/// **************************************************************************
/// Name: esp_log_timestamp
///
/// Description:
///   Get system time by millim second
///
/// Input Parameters:
///   None
///
/// Returned Value:
///   System time
///
/// *************************************************************************
pub unsafe extern "C" fn log_timestamp() -> u32 {
    esp_hal::time::Instant::now()
        .duration_since_epoch()
        .as_millis() as u32
}

/// **************************************************************************
/// Name: esp_malloc_internal
///
/// Description:
///   Drivers allocate a block of memory
///
/// Input Parameters:
///   size - memory size
///
/// Returned Value:
///   Memory pointer
///
/// *************************************************************************
pub unsafe extern "C" fn malloc_internal(size: usize) -> *mut c_void {
    record_alloc(
        &WIFI_OS_MALLOC_INTERNAL_COUNT,
        &WIFI_OS_MALLOC_INTERNAL_TOTAL_SIZE,
        &WIFI_OS_MALLOC_INTERNAL_MAX_SIZE,
        &WIFI_OS_MALLOC_INTERNAL_LAST_SIZE,
        size,
    );
    if wifi_use_legacy_wifi_alloc_diag_enabled() {
        unsafe { crate::compat::malloc::malloc(size).cast() }
    } else {
        unsafe { crate::compat::malloc::malloc_internal(size).cast() }
    }
}

/// **************************************************************************
/// Name: esp_realloc_internal
///
/// Description:
///   Drivers allocate a block of memory by old memory block
///
/// Input Parameters:
///   ptr  - old memory pointer
///   size - memory size
///
/// Returned Value:
///   New memory pointer
///
/// *************************************************************************
pub unsafe extern "C" fn realloc_internal(ptr: *mut c_void, size: usize) -> *mut c_void {
    unsafe { crate::compat::malloc::realloc_internal(ptr.cast(), size).cast() }
}

/// **************************************************************************
/// Name: esp_calloc_internal
///
/// Description:
///   Drivers allocate some continuous blocks of memory
///
/// Input Parameters:
///   n    - memory block number
///   size - memory block size
///
/// Returned Value:
///   New memory pointer
///
/// *************************************************************************
pub unsafe extern "C" fn calloc_internal_wrapper(n: usize, size: usize) -> *mut c_void {
    record_alloc(
        &WIFI_OS_CALLOC_INTERNAL_COUNT,
        &WIFI_OS_CALLOC_INTERNAL_TOTAL_SIZE,
        &WIFI_OS_CALLOC_INTERNAL_MAX_SIZE,
        &WIFI_OS_CALLOC_INTERNAL_LAST_SIZE,
        n.saturating_mul(size),
    );
    if wifi_use_legacy_wifi_alloc_diag_enabled() {
        unsafe { crate::compat::malloc::calloc(n as u32, size).cast() }
    } else {
        unsafe { calloc_internal(n as u32, size) as *mut c_void }
    }
}

/// **************************************************************************
/// Name: esp_zalloc_internal
///
/// Description:
///   Drivers allocate a block of memory and clear it with 0
///
/// Input Parameters:
///   size - memory size
///
/// Returned Value:
///   New memory pointer
///
/// *************************************************************************
pub unsafe extern "C" fn zalloc_internal(size: usize) -> *mut c_void {
    if wifi_use_legacy_wifi_alloc_diag_enabled() {
        unsafe { crate::compat::malloc::calloc(size as u32, 1usize).cast() }
    } else {
        unsafe { calloc_internal(size as u32, 1usize) as *mut c_void }
    }
}

/// **************************************************************************
/// Name: esp_wifi_malloc
///
/// Description:
///   Applications allocate a block of memory
///
/// Input Parameters:
///   size - memory size
///
/// Returned Value:
///   Memory pointer
///
/// *************************************************************************
pub unsafe extern "C" fn wifi_malloc(size: usize) -> *mut c_void {
    record_alloc(
        &WIFI_OS_WIFI_MALLOC_COUNT,
        &WIFI_OS_WIFI_MALLOC_TOTAL_SIZE,
        &WIFI_OS_WIFI_MALLOC_MAX_SIZE,
        &WIFI_OS_WIFI_MALLOC_LAST_SIZE,
        size,
    );
    unsafe { malloc_internal(size) }
}

/// **************************************************************************
/// Name: esp_wifi_realloc
///
/// Description:
///   Applications allocate a block of memory by old memory block
///
/// Input Parameters:
///   ptr  - old memory pointer
///   size - memory size
///
/// Returned Value:
///   New memory pointer
///
/// *************************************************************************
pub unsafe extern "C" fn wifi_realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    unsafe { realloc_internal(ptr, size) }
}

/// **************************************************************************
/// Name: esp_wifi_calloc
///
/// Description:
///   Applications allocate some continuous blocks of memory
///
/// Input Parameters:
///   n    - memory block number
///   size - memory block size
///
/// Returned Value:
///   New memory pointer
///
/// *************************************************************************
pub unsafe extern "C" fn wifi_calloc(n: usize, size: usize) -> *mut c_void {
    trace!("wifi_calloc {} {}", n, size);
    record_alloc(
        &WIFI_OS_WIFI_CALLOC_COUNT,
        &WIFI_OS_WIFI_CALLOC_TOTAL_SIZE,
        &WIFI_OS_WIFI_CALLOC_MAX_SIZE,
        &WIFI_OS_WIFI_CALLOC_LAST_SIZE,
        n.saturating_mul(size),
    );
    if wifi_use_legacy_wifi_alloc_diag_enabled() {
        unsafe { crate::compat::malloc::calloc(n as u32, size).cast() }
    } else {
        unsafe { calloc_internal(n as u32, size) as *mut c_void }
    }
}

/// **************************************************************************
/// Name: esp_wifi_zalloc
///
/// Description:
///   Applications allocate a block of memory and clear it with 0
///
/// Input Parameters:
///   size - memory size
///
/// Returned Value:
///   New memory pointer
///
/// *************************************************************************
pub unsafe extern "C" fn wifi_zalloc(size: usize) -> *mut c_void {
    if wifi_use_legacy_wifi_alloc_diag_enabled() {
        unsafe { crate::compat::malloc::calloc(size as u32, 1usize).cast() }
    } else {
        unsafe { wifi_calloc(size, 1) }
    }
}

/// **************************************************************************
/// Name: esp_wifi_create_queue
///
/// Description:
///   Create Wi-Fi static message queue
///
/// Input Parameters:
///   queue_len - queue message number
///   item_size - message size
///
/// Returned Value:
///   Wi-Fi static message queue data pointer
///
/// *************************************************************************
pub unsafe extern "C" fn wifi_create_queue(queue_len: c_int, item_size: c_int) -> *mut c_void {
    wifi_init_runtime_trace("wifi_create_queue.before");
    // Legacy esp-wifi 0.15.1 carries a supplicant queue-size workaround here.
    let (queue_len, item_size) = if queue_len == 3 && item_size == 4 {
        (3, 8)
    } else {
        (queue_len, item_size)
    };
    let queue = crate::compat::queue::queue_create(queue_len, item_size);

    let queue_ptr: *mut *mut c_void = Box::leak(Box::new_in(queue, InternalMemory));
    wifi_init_runtime_trace("wifi_create_queue.after");

    queue_ptr.cast()
}

/// **************************************************************************
/// Name: esp_wifi_delete_queue
///
/// Description:
///   Delete Wi-Fi static message queue
///
/// Input Parameters:
///   queue - Wi-Fi static message queue data pointer
///
/// Returned Value:
///   None
///
/// *************************************************************************
pub unsafe extern "C" fn wifi_delete_queue(queue: *mut c_void) {
    let queue_ptr: *mut *mut c_void = queue.cast();

    let boxed = unsafe { Box::from_raw_in(queue_ptr, InternalMemory) };

    crate::compat::queue::queue_delete(*boxed)
}

/// **************************************************************************
/// Name: wifi_coex_deinit
///
/// Description:
///   Don't support
///
/// *************************************************************************
pub unsafe extern "C" fn coex_deinit() {
    trace!("coex_deinit");

    #[cfg(coex)]
    unsafe {
        crate::binary::include::coex_deinit()
    };
}

/// **************************************************************************
/// Name: wifi_coex_enable
///
/// Description:
///   Don't support
///
/// *************************************************************************
pub unsafe extern "C" fn coex_enable() -> c_int {
    trace!("coex_enable");

    #[cfg(coex)]
    return unsafe { crate::binary::include::coex_enable() };

    #[cfg(not(coex))]
    0
}

/// **************************************************************************
/// Name: wifi_coex_disable
///
/// Description:
///   Don't support
///
/// *************************************************************************
pub unsafe extern "C" fn coex_disable() {
    trace!("coex_disable");

    #[cfg(coex)]
    unsafe {
        crate::binary::include::coex_disable()
    };
}

/// **************************************************************************
/// Name: esp_coex_status_get
///
/// Description:
///   Don't support
///
/// *************************************************************************
pub unsafe extern "C" fn coex_status_get() -> u32 {
    trace!("coex_status_get");

    #[cfg(coex)]
    {
        if wifi_use_legacy_coex_status_get_diag_enabled() {
            return 0;
        }
        return unsafe { crate::binary::include::coex_status_get(0b1) }; // COEX_STATUS_GET_WIFI_BITMAP
    }

    #[cfg(not(coex))]
    0
}

/// **************************************************************************
/// Name: esp_coex_wifi_request
///
/// Description:
///   Don't support
///
/// *************************************************************************
#[cfg_attr(not(coex), allow(unused_variables))]
pub unsafe extern "C" fn coex_wifi_request(event: u32, latency: u32, duration: u32) -> c_int {
    trace!("coex_wifi_request");

    #[cfg(coex)]
    return unsafe { crate::binary::include::coex_wifi_request(event, latency, duration) };

    #[cfg(not(coex))]
    0
}

/// **************************************************************************
/// Name: esp_coex_wifi_release
///
/// Description:
///   Don't support
///
/// *************************************************************************
#[cfg_attr(not(coex), allow(unused_variables))]
pub unsafe extern "C" fn coex_wifi_release(event: u32) -> c_int {
    trace!("coex_wifi_release");

    #[cfg(coex)]
    return unsafe { crate::binary::include::coex_wifi_release(event) };

    #[cfg(not(coex))]
    0
}

/// **************************************************************************
/// Name: wifi_coex_wifi_set_channel
///
/// Description:
///   Don't support
///
/// *************************************************************************
#[cfg_attr(not(coex), allow(unused_variables))]
pub unsafe extern "C" fn coex_wifi_channel_set(primary: u8, secondary: u8) -> c_int {
    trace!("coex_wifi_channel_set");

    #[cfg(coex)]
    return unsafe { crate::binary::include::coex_wifi_channel_set(primary, secondary) };

    #[cfg(not(coex))]
    0
}

/// **************************************************************************
/// Name: wifi_coex_get_event_duration
///
/// Description:
///   Don't support
///
/// *************************************************************************
#[cfg_attr(not(coex), allow(unused_variables))]
pub unsafe extern "C" fn coex_event_duration_get(event: u32, duration: *mut u32) -> c_int {
    trace!("coex_event_duration_get");

    #[cfg(coex)]
    return unsafe { crate::binary::include::coex_event_duration_get(event, duration) };

    #[cfg(not(coex))]
    0
}

/// **************************************************************************
/// Name: wifi_coex_get_pti
///
/// Description:
///   Don't support
///
/// *************************************************************************
#[cfg(any(esp32c3, esp32c2, esp32c6, esp32s3))]
#[cfg_attr(not(coex), allow(unused_variables))]
pub unsafe extern "C" fn coex_pti_get(event: u32, pti: *mut u8) -> c_int {
    trace!("coex_pti_get");

    #[cfg(coex)]
    return unsafe { crate::binary::include::coex_pti_get(event, pti) };

    #[cfg(not(coex))]
    0
}

#[cfg(any(esp32, esp32s2))]
pub unsafe extern "C" fn coex_pti_get(event: u32, pti: *mut u8) -> c_int {
    trace!("coex_pti_get {} {:?}", event, pti);
    0
}

/// **************************************************************************
/// Name: wifi_coex_clear_schm_status_bit
///
/// Description:
///   Don't support
///
/// *************************************************************************
#[allow(unused_variables)]
pub unsafe extern "C" fn coex_schm_status_bit_clear(type_: u32, status: u32) {
    trace!("coex_schm_status_bit_clear");

    #[cfg(coex)]
    unsafe {
        crate::binary::include::coex_schm_status_bit_clear(type_, status)
    };
}

/// **************************************************************************
/// Name: wifi_coex_set_schm_status_bit
///
/// Description:
///   Don't support
///
/// *************************************************************************
#[allow(unused_variables)]
pub unsafe extern "C" fn coex_schm_status_bit_set(type_: u32, status: u32) {
    trace!("coex_schm_status_bit_set");

    #[cfg(coex)]
    unsafe {
        crate::binary::include::coex_schm_status_bit_set(type_, status)
    };
}

/// **************************************************************************
/// Name: wifi_coex_set_schm_interval
///
/// Description:
///   Don't support
///
/// *************************************************************************
#[allow(unused_variables)]
pub unsafe extern "C" fn coex_schm_interval_set(interval: u32) -> c_int {
    trace!("coex_schm_interval_set");

    #[cfg(coex)]
    return unsafe { crate::binary::include::coex_schm_interval_set(interval) };

    #[cfg(not(coex))]
    0
}

/// **************************************************************************
/// Name: wifi_coex_get_schm_interval
///
/// Description:
///   Don't support
///
/// *************************************************************************
#[allow(unused_variables)]
pub unsafe extern "C" fn coex_schm_interval_get() -> u32 {
    trace!("coex_schm_interval_get");

    #[cfg(coex)]
    return unsafe { crate::binary::include::coex_schm_interval_get() };

    #[cfg(not(coex))]
    0
}

/// **************************************************************************
/// Name: wifi_coex_get_schm_curr_period
///
/// Description:
///   Don't support
///
/// *************************************************************************
#[allow(unused_variables)]
pub unsafe extern "C" fn coex_schm_curr_period_get() -> u8 {
    trace!("coex_schm_curr_period_get");

    #[cfg(coex)]
    return unsafe { crate::binary::include::coex_schm_curr_period_get() };

    #[cfg(not(coex))]
    0
}

/// **************************************************************************
/// Name: wifi_coex_get_schm_curr_phase
///
/// Description:
///   Don't support
///
/// *************************************************************************
#[allow(unused_variables)]
pub unsafe extern "C" fn coex_schm_curr_phase_get() -> *mut c_void {
    trace!("coex_schm_curr_phase_get");

    #[cfg(coex)]
    return unsafe { crate::binary::include::coex_schm_curr_phase_get() };

    #[cfg(not(coex))]
    return core::ptr::null_mut();
}

pub unsafe extern "C" fn coex_schm_process_restart_wrapper() -> esp_wifi_sys::c_types::c_int {
    trace!("coex_schm_process_restart_wrapper");

    #[cfg(not(coex))]
    return 0;

    #[cfg(coex)]
    unsafe {
        crate::binary::include::coex_schm_process_restart()
    }
}

#[allow(unused_variables)]
pub unsafe extern "C" fn coex_schm_register_cb_wrapper(
    arg1: esp_wifi_sys::c_types::c_int,
    cb: ::core::option::Option<
        unsafe extern "C" fn(arg1: esp_wifi_sys::c_types::c_int) -> esp_wifi_sys::c_types::c_int,
    >,
) -> esp_wifi_sys::c_types::c_int {
    trace!("coex_schm_register_cb_wrapper {} {:?}", arg1, cb);

    #[cfg(not(coex))]
    return 0;

    #[cfg(coex)]
    unsafe {
        crate::binary::include::coex_schm_register_callback(
            arg1 as u32,
            unwrap!(cb) as *const esp_wifi_sys::c_types::c_void
                as *mut esp_wifi_sys::c_types::c_void,
        )
    }
}

pub unsafe extern "C" fn coex_schm_flexible_period_set(period: u8) -> i32 {
    trace!("coex_schm_flexible_period_set {}", period);

    #[cfg(coex)]
    unsafe {
        unsafe extern "C" {
            fn coex_schm_flexible_period_set(period: u8) -> i32;
        }

        coex_schm_flexible_period_set(period)
    }

    #[cfg(not(coex))]
    0
}

pub unsafe extern "C" fn coex_schm_flexible_period_get() -> u8 {
    trace!("coex_schm_flexible_period_get");

    #[cfg(coex)]
    unsafe {
        unsafe extern "C" {
            fn coex_schm_flexible_period_get() -> u8;
        }

        coex_schm_flexible_period_get()
    }

    #[cfg(not(coex))]
    0
}

pub unsafe extern "C" fn coex_register_start_cb(
    _cb: Option<unsafe extern "C" fn() -> esp_wifi_sys::c_types::c_int>,
) -> esp_wifi_sys::c_types::c_int {
    #[cfg(coex)]
    return unsafe { esp_wifi_sys::include::coex_register_start_cb(_cb) };

    #[cfg(not(coex))]
    0
}

pub unsafe extern "C" fn coex_schm_get_phase_by_idx(
    _phase_idx: i32,
) -> *mut esp_wifi_sys::c_types::c_void {
    #[cfg(coex)]
    return unsafe { crate::binary::include::coex_schm_get_phase_by_idx(_phase_idx) };

    #[cfg(not(coex))]
    core::ptr::null_mut()
}

/// **************************************************************************
/// Name: esp_clk_slowclk_cal_get_wrapper
///
/// Description:
///   Get the calibration value of RTC slow clock
///
/// Input Parameters:
///   None
///
/// Returned Value:
///   The calibration value obtained using rtc_clk_cal
///
/// *************************************************************************
#[allow(unused)]
pub unsafe extern "C" fn slowclk_cal_get() -> u32 {
    trace!("slowclk_cal_get");

    // TODO not hardcode this

    #[cfg(esp32s2)]
    return 44462;

    #[cfg(esp32s3)]
    return 44462;

    #[cfg(esp32c3)]
    return 28639;

    #[cfg(esp32c2)]
    return 28639;

    #[cfg(any(esp32c6, esp32h2))]
    return 0;

    #[cfg(esp32)]
    return 28639;
}
