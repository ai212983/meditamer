use alloc::boxed::Box;
use core::{
    cell::{RefCell, UnsafeCell},
    ffi::c_void,
    ptr::NonNull,
};

use esp_hal::time::{Duration, Instant};
use esp_radio_rtos_driver::{
    register_timer_implementation,
    timer::{TimerImplementation, TimerPtr},
};
use portable_atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use esp_sync::NonReentrantMutex;

use crate::{
    SCHEDULER,
    esp_radio::{
        backend_legacy_port_runtime_enabled, legacy_builtin_scheduler_runtime_mode_enabled,
        legacy_scheduler,
    },
    task::{TaskExt, TaskPtr},
};

unsafe extern "C" {
    fn __esp_radio_diag_legacy_timer_compat_enabled() -> bool;
    fn __esp_radio_diag_process_legacy_timer_compat_due() -> bool;
    fn __esp_radio_diag_legacy_timer_compat_next_due_us() -> u32;
}

static TIMER_QUEUE: TimerQueue = TimerQueue::new();
const TIMER_EXEC_RING_CAP: usize = 6;
const TIMER_ARM_RING_CAP: usize = 6;
static TIMER_CALLBACK_EXEC_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_TASK_ENTRY_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_TASK_RESUME_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_TASK_LOOP_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_TASK_LEGACY_COMPAT_BRANCH_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_TASK_LEGACY_DRIVER_BRANCH_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_TASK_DEFAULT_BRANCH_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_TASK_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_TASK_CREATE_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_TASK_CREATE_FROM_ENSURE_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_TASK_CREATE_FROM_WAKE_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_TASK_CREATE_FROM_ENQUEUE_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_TASK_CREATE_LAST_MODE: AtomicU32 = AtomicU32::new(0);
static TIMER_TASK_CREATE_LAST_SOURCE: AtomicU32 = AtomicU32::new(0);
static TIMER_TASK_CREATE_LAST_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_TASK_PROCESS_SKIP_INACTIVE_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_TASK_PROCESS_SKIP_NOT_DUE_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_TASK_PROCESS_LAST_SKIP_CALLBACK_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_TASK_PROCESS_LAST_SKIP_ARG_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_TASK_PROCESS_LAST_SKIP_NOW_US: AtomicU32 = AtomicU32::new(0);
static TIMER_TASK_PROCESS_LAST_SKIP_DUE_US: AtomicU32 = AtomicU32::new(0);
static TIMER_TASK_MARK_READY_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_TASK_POP_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_TASK_SELECTED_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_TASK_SLEEP_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_TASK_SLEEP_TRUE_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_TASK_SLEEP_FALSE_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_TASK_SLEEP_LAST_TASK_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_TASK_SLEEP_LAST_WAKE_AT_US: AtomicU64 = AtomicU64::new(0);
static TIMER_TASK_SLEEP_LAST_RESULT: AtomicBool = AtomicBool::new(false);
static TIMER_TASK_SLEEP_TASK_MISMATCH_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_CALLBACK_CURRENT_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_CALLBACK_CURRENT_ARG_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_CALLBACK_LAST_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_CALLBACK_LAST_ARG_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_CALLBACK_LAST_EXEC_AT_US: AtomicU32 = AtomicU32::new(0);
static TIMER_CALLBACK_LAST_DUE_AT_US: AtomicU32 = AtomicU32::new(0);
static TIMER_CALLBACK_LAST_TIMEOUT_US: AtomicU32 = AtomicU32::new(0);
static TIMER_CALLBACK_LAST_LATENESS_US: AtomicU32 = AtomicU32::new(0);
static TIMER_CALLBACK_MAX_LATENESS_US: AtomicU32 = AtomicU32::new(0);
static TIMER_CALLBACK_SIDEEFFECT_ARM_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_CALLBACK_SIDEEFFECT_DISARM_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_CALLBACK_SIDEEFFECT_LAST_KIND: AtomicU32 = AtomicU32::new(0);
static TIMER_CALLBACK_SIDEEFFECT_LAST_CURRENT_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_CALLBACK_SIDEEFFECT_LAST_CURRENT_ARG_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_CALLBACK_SIDEEFFECT_LAST_TARGET_TIMER_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_CALLBACK_SIDEEFFECT_LAST_TARGET_CALLBACK_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_CALLBACK_SIDEEFFECT_LAST_TARGET_ARG_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_CALLBACK_SIDEEFFECT_LAST_TIMEOUT_US: AtomicU64 = AtomicU64::new(0);
static TIMER_CALLBACK_SIDEEFFECT_LAST_REPEAT: AtomicBool = AtomicBool::new(false);
static TIMER_CALLBACK_RECENT_ORDINALS: [AtomicU32; TIMER_EXEC_RING_CAP] =
    [const { AtomicU32::new(0) }; TIMER_EXEC_RING_CAP];
static TIMER_CALLBACK_RECENT_PTRS: [AtomicUsize; TIMER_EXEC_RING_CAP] =
    [const { AtomicUsize::new(0) }; TIMER_EXEC_RING_CAP];
static TIMER_CALLBACK_RECENT_ARG_PTRS: [AtomicUsize; TIMER_EXEC_RING_CAP] =
    [const { AtomicUsize::new(0) }; TIMER_EXEC_RING_CAP];
static TIMER_CALLBACK_RECENT_EXEC_AT_US: [AtomicU32; TIMER_EXEC_RING_CAP] =
    [const { AtomicU32::new(0) }; TIMER_EXEC_RING_CAP];
static TIMER_CALLBACK_RECENT_DUE_AT_US: [AtomicU32; TIMER_EXEC_RING_CAP] =
    [const { AtomicU32::new(0) }; TIMER_EXEC_RING_CAP];
static TIMER_CALLBACK_RECENT_TIMEOUT_US: [AtomicU32; TIMER_EXEC_RING_CAP] =
    [const { AtomicU32::new(0) }; TIMER_EXEC_RING_CAP];
static TIMER_CALLBACK_RECENT_LATENESS_US: [AtomicU32; TIMER_EXEC_RING_CAP] =
    [const { AtomicU32::new(0) }; TIMER_EXEC_RING_CAP];
static TIMER_ARM_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_ARM_RECENT_ORDINALS: [AtomicU32; TIMER_ARM_RING_CAP] =
    [const { AtomicU32::new(0) }; TIMER_ARM_RING_CAP];
static TIMER_ARM_RECENT_TIMER_PTRS: [AtomicUsize; TIMER_ARM_RING_CAP] =
    [const { AtomicUsize::new(0) }; TIMER_ARM_RING_CAP];
static TIMER_ARM_RECENT_CALLBACK_PTRS: [AtomicUsize; TIMER_ARM_RING_CAP] =
    [const { AtomicUsize::new(0) }; TIMER_ARM_RING_CAP];
static TIMER_ARM_RECENT_ARG_PTRS: [AtomicUsize; TIMER_ARM_RING_CAP] =
    [const { AtomicUsize::new(0) }; TIMER_ARM_RING_CAP];
static TIMER_ARM_RECENT_CALLER_PTRS: [AtomicUsize; TIMER_ARM_RING_CAP] =
    [const { AtomicUsize::new(0) }; TIMER_ARM_RING_CAP];
static TIMER_ARM_RECENT_TIMEOUT_US: [AtomicU64; TIMER_ARM_RING_CAP] =
    [const { AtomicU64::new(0) }; TIMER_ARM_RING_CAP];
static TIMER_ARM_RECENT_PERIODIC: [AtomicBool; TIMER_ARM_RING_CAP] =
    [const { AtomicBool::new(false) }; TIMER_ARM_RING_CAP];

#[cfg(xtensa)]
fn current_arm_caller_ptr() -> usize {
    let caller_ptr: usize;
    unsafe {
        core::arch::asm!("mov {0}, a0", out(reg) caller_ptr);
    }
    caller_ptr
}

#[cfg(not(xtensa))]
fn current_arm_caller_ptr() -> usize {
    0
}

fn record_timer_arm(
    timer_ptr: usize,
    callback_ptr: usize,
    arg_ptr: usize,
    caller_ptr: usize,
    timeout_us: u64,
    periodic: bool,
) {
    let ordinal = TIMER_ARM_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    let idx = (ordinal as usize) % TIMER_ARM_RING_CAP;
    TIMER_ARM_RECENT_ORDINALS[idx].store(ordinal, Ordering::Relaxed);
    TIMER_ARM_RECENT_TIMER_PTRS[idx].store(timer_ptr, Ordering::Relaxed);
    TIMER_ARM_RECENT_CALLBACK_PTRS[idx].store(callback_ptr, Ordering::Relaxed);
    TIMER_ARM_RECENT_ARG_PTRS[idx].store(arg_ptr, Ordering::Relaxed);
    TIMER_ARM_RECENT_CALLER_PTRS[idx].store(caller_ptr, Ordering::Relaxed);
    TIMER_ARM_RECENT_TIMEOUT_US[idx].store(timeout_us, Ordering::Relaxed);
    TIMER_ARM_RECENT_PERIODIC[idx].store(periodic, Ordering::Relaxed);
}

fn use_legacy_timer_loop_diag_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RTOS_USE_LEGACY_TIMER_LOOP_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("ESP_RTOS_USE_LEGACY_TIMER_LOOP_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn use_legacy_timer_task_driver_diag_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RTOS_USE_LEGACY_TIMER_TASK_DRIVER_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("ESP_RTOS_USE_LEGACY_TIMER_TASK_DRIVER_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || backend_legacy_port_runtime_enabled()
}

fn use_legacy_timer_compat_driver_diag_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_TIMER_COMPAT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("ESP_RADIO_USE_LEGACY_TIMER_COMPAT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || backend_legacy_port_runtime_enabled()
}

fn use_exact_legacy_timer_compat_loop_diag_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RTOS_USE_EXACT_LEGACY_TIMER_COMPAT_LOOP_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("ESP_RTOS_USE_EXACT_LEGACY_TIMER_COMPAT_LOOP_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
/// Snapshot of timer callback execution state inside the esp-rtos timer queue.
pub struct TimerCallbackExecDiag {
    /// Total callbacks executed since the last reset.
    pub callback_count: u32,
    /// Callback pointer currently executing, or `0` when not in a timer callback.
    pub current_callback_ptr: usize,
    /// Current callback argument pointer, or `0` when not in a timer callback.
    pub current_arg_ptr: usize,
    /// Most recently executed callback pointer.
    pub last_callback_ptr: usize,
    /// Most recently executed callback argument pointer.
    pub last_arg_ptr: usize,
    /// Timestamp when the most recent callback actually executed.
    pub last_exec_at_us: u32,
    /// Scheduled due timestamp for the most recent callback execution.
    pub last_due_at_us: u32,
    /// Timeout value used when the most recent callback was armed.
    pub last_timeout_us: u32,
    /// How late the most recent callback executed relative to its due time.
    pub last_lateness_us: u32,
    /// Maximum observed lateness since the last reset.
    pub max_lateness_us: u32,
    /// Recent callback execution ordinals.
    pub recent_ordinals: [u32; TIMER_EXEC_RING_CAP],
    /// Recent callback pointers.
    pub recent_callback_ptrs: [usize; TIMER_EXEC_RING_CAP],
    /// Recent callback arg pointers.
    pub recent_arg_ptrs: [usize; TIMER_EXEC_RING_CAP],
    /// Recent execution timestamps.
    pub recent_exec_at_us: [u32; TIMER_EXEC_RING_CAP],
    /// Recent due timestamps.
    pub recent_due_at_us: [u32; TIMER_EXEC_RING_CAP],
    /// Recent timeout values.
    pub recent_timeout_us: [u32; TIMER_EXEC_RING_CAP],
    /// Recent lateness values.
    pub recent_lateness_us: [u32; TIMER_EXEC_RING_CAP],
    /// Number of `arm()` calls issued from inside a current-substrate timer callback.
    pub sideeffect_arm_count: u32,
    /// Number of `disarm()` calls issued from inside a current-substrate timer callback.
    pub sideeffect_disarm_count: u32,
    /// Last side-effect kind: `0` none, `1` arm, `2` disarm.
    pub sideeffect_last_kind: u32,
    /// Current callback pointer that caused the last side effect.
    pub sideeffect_last_current_ptr: usize,
    /// Current callback arg pointer that caused the last side effect.
    pub sideeffect_last_current_arg_ptr: usize,
    /// Target timer pointer of the last side effect.
    pub sideeffect_last_target_timer_ptr: usize,
    /// Target callback pointer of the last side effect.
    pub sideeffect_last_target_callback_ptr: usize,
    /// Target callback arg pointer of the last side effect.
    pub sideeffect_last_target_arg_ptr: usize,
    /// Timeout associated with the last side-effect arm.
    pub sideeffect_last_timeout_us: u64,
    /// Repeat flag associated with the last side-effect arm.
    pub sideeffect_last_repeat: bool,
}

pub fn reset_timer_callback_exec_diag() {
    TIMER_CALLBACK_EXEC_COUNT.store(0, Ordering::Relaxed);
    TIMER_TASK_ENTRY_COUNT.store(0, Ordering::Relaxed);
    TIMER_TASK_RESUME_COUNT.store(0, Ordering::Relaxed);
    TIMER_TASK_LOOP_COUNT.store(0, Ordering::Relaxed);
    TIMER_TASK_LEGACY_COMPAT_BRANCH_COUNT.store(0, Ordering::Relaxed);
    TIMER_TASK_LEGACY_DRIVER_BRANCH_COUNT.store(0, Ordering::Relaxed);
    TIMER_TASK_DEFAULT_BRANCH_COUNT.store(0, Ordering::Relaxed);
    TIMER_TASK_CREATE_COUNT.store(0, Ordering::Relaxed);
    TIMER_TASK_CREATE_FROM_ENSURE_COUNT.store(0, Ordering::Relaxed);
    TIMER_TASK_CREATE_FROM_WAKE_COUNT.store(0, Ordering::Relaxed);
    TIMER_TASK_CREATE_FROM_ENQUEUE_COUNT.store(0, Ordering::Relaxed);
    TIMER_TASK_CREATE_LAST_MODE.store(0, Ordering::Relaxed);
    TIMER_TASK_CREATE_LAST_SOURCE.store(0, Ordering::Relaxed);
    TIMER_TASK_CREATE_LAST_PTR.store(0, Ordering::Relaxed);
    TIMER_TASK_PROCESS_SKIP_INACTIVE_COUNT.store(0, Ordering::Relaxed);
    TIMER_TASK_PROCESS_SKIP_NOT_DUE_COUNT.store(0, Ordering::Relaxed);
    TIMER_TASK_PROCESS_LAST_SKIP_CALLBACK_PTR.store(0, Ordering::Relaxed);
    TIMER_TASK_PROCESS_LAST_SKIP_ARG_PTR.store(0, Ordering::Relaxed);
    TIMER_TASK_PROCESS_LAST_SKIP_NOW_US.store(0, Ordering::Relaxed);
    TIMER_TASK_PROCESS_LAST_SKIP_DUE_US.store(0, Ordering::Relaxed);
    TIMER_TASK_MARK_READY_COUNT.store(0, Ordering::Relaxed);
    TIMER_TASK_POP_COUNT.store(0, Ordering::Relaxed);
    TIMER_TASK_SELECTED_COUNT.store(0, Ordering::Relaxed);
    TIMER_TASK_SLEEP_COUNT.store(0, Ordering::Relaxed);
    TIMER_TASK_SLEEP_TRUE_COUNT.store(0, Ordering::Relaxed);
    TIMER_TASK_SLEEP_FALSE_COUNT.store(0, Ordering::Relaxed);
    TIMER_TASK_SLEEP_LAST_TASK_PTR.store(0, Ordering::Relaxed);
    TIMER_TASK_SLEEP_LAST_WAKE_AT_US.store(0, Ordering::Relaxed);
    TIMER_TASK_SLEEP_LAST_RESULT.store(false, Ordering::Relaxed);
    TIMER_TASK_SLEEP_TASK_MISMATCH_COUNT.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_CURRENT_PTR.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_CURRENT_ARG_PTR.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_LAST_PTR.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_LAST_ARG_PTR.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_LAST_EXEC_AT_US.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_LAST_DUE_AT_US.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_LAST_TIMEOUT_US.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_LAST_LATENESS_US.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_MAX_LATENESS_US.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_SIDEEFFECT_ARM_COUNT.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_SIDEEFFECT_DISARM_COUNT.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_SIDEEFFECT_LAST_KIND.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_SIDEEFFECT_LAST_CURRENT_PTR.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_SIDEEFFECT_LAST_CURRENT_ARG_PTR.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_SIDEEFFECT_LAST_TARGET_TIMER_PTR.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_SIDEEFFECT_LAST_TARGET_CALLBACK_PTR.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_SIDEEFFECT_LAST_TARGET_ARG_PTR.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_SIDEEFFECT_LAST_TIMEOUT_US.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_SIDEEFFECT_LAST_REPEAT.store(false, Ordering::Relaxed);
    for idx in 0..TIMER_EXEC_RING_CAP {
        TIMER_CALLBACK_RECENT_ORDINALS[idx].store(0, Ordering::Relaxed);
        TIMER_CALLBACK_RECENT_PTRS[idx].store(0, Ordering::Relaxed);
        TIMER_CALLBACK_RECENT_ARG_PTRS[idx].store(0, Ordering::Relaxed);
        TIMER_CALLBACK_RECENT_EXEC_AT_US[idx].store(0, Ordering::Relaxed);
        TIMER_CALLBACK_RECENT_DUE_AT_US[idx].store(0, Ordering::Relaxed);
        TIMER_CALLBACK_RECENT_TIMEOUT_US[idx].store(0, Ordering::Relaxed);
        TIMER_CALLBACK_RECENT_LATENESS_US[idx].store(0, Ordering::Relaxed);
    }
    TIMER_ARM_COUNT.store(0, Ordering::Relaxed);
    for idx in 0..TIMER_ARM_RING_CAP {
        TIMER_ARM_RECENT_ORDINALS[idx].store(0, Ordering::Relaxed);
        TIMER_ARM_RECENT_TIMER_PTRS[idx].store(0, Ordering::Relaxed);
        TIMER_ARM_RECENT_CALLBACK_PTRS[idx].store(0, Ordering::Relaxed);
        TIMER_ARM_RECENT_ARG_PTRS[idx].store(0, Ordering::Relaxed);
        TIMER_ARM_RECENT_CALLER_PTRS[idx].store(0, Ordering::Relaxed);
        TIMER_ARM_RECENT_TIMEOUT_US[idx].store(0, Ordering::Relaxed);
        TIMER_ARM_RECENT_PERIODIC[idx].store(false, Ordering::Relaxed);
    }
}

pub fn timer_task_entry_count() -> u32 {
    TIMER_TASK_ENTRY_COUNT.load(Ordering::Relaxed)
}

pub fn timer_task_resume_count() -> u32 {
    TIMER_TASK_RESUME_COUNT.load(Ordering::Relaxed)
}

pub fn timer_task_loop_count() -> u32 {
    TIMER_TASK_LOOP_COUNT.load(Ordering::Relaxed)
}

pub fn timer_task_legacy_compat_branch_count() -> u32 {
    TIMER_TASK_LEGACY_COMPAT_BRANCH_COUNT.load(Ordering::Relaxed)
}

pub fn timer_task_legacy_driver_branch_count() -> u32 {
    TIMER_TASK_LEGACY_DRIVER_BRANCH_COUNT.load(Ordering::Relaxed)
}

pub fn timer_task_default_branch_count() -> u32 {
    TIMER_TASK_DEFAULT_BRANCH_COUNT.load(Ordering::Relaxed)
}

pub fn timer_task_create_count() -> u32 {
    TIMER_TASK_CREATE_COUNT.load(Ordering::Relaxed)
}

pub fn timer_task_create_from_ensure_count() -> u32 {
    TIMER_TASK_CREATE_FROM_ENSURE_COUNT.load(Ordering::Relaxed)
}

pub fn timer_task_create_from_wake_count() -> u32 {
    TIMER_TASK_CREATE_FROM_WAKE_COUNT.load(Ordering::Relaxed)
}

pub fn timer_task_create_from_enqueue_count() -> u32 {
    TIMER_TASK_CREATE_FROM_ENQUEUE_COUNT.load(Ordering::Relaxed)
}

pub fn timer_task_create_last_mode() -> u32 {
    TIMER_TASK_CREATE_LAST_MODE.load(Ordering::Relaxed)
}

pub fn timer_task_create_last_source() -> u32 {
    TIMER_TASK_CREATE_LAST_SOURCE.load(Ordering::Relaxed)
}

pub fn timer_task_create_last_ptr() -> usize {
    TIMER_TASK_CREATE_LAST_PTR.load(Ordering::Relaxed)
}

pub fn timer_task_process_skip_inactive_count() -> u32 {
    TIMER_TASK_PROCESS_SKIP_INACTIVE_COUNT.load(Ordering::Relaxed)
}

pub fn timer_task_process_skip_not_due_count() -> u32 {
    TIMER_TASK_PROCESS_SKIP_NOT_DUE_COUNT.load(Ordering::Relaxed)
}

pub fn timer_task_process_last_skip_callback_ptr() -> usize {
    TIMER_TASK_PROCESS_LAST_SKIP_CALLBACK_PTR.load(Ordering::Relaxed)
}

pub fn timer_task_process_last_skip_arg_ptr() -> usize {
    TIMER_TASK_PROCESS_LAST_SKIP_ARG_PTR.load(Ordering::Relaxed)
}

pub fn timer_task_process_last_skip_now_us() -> u32 {
    TIMER_TASK_PROCESS_LAST_SKIP_NOW_US.load(Ordering::Relaxed)
}

pub fn timer_task_process_last_skip_due_us() -> u32 {
    TIMER_TASK_PROCESS_LAST_SKIP_DUE_US.load(Ordering::Relaxed)
}

pub fn timer_task_mark_ready_count() -> u32 {
    TIMER_TASK_MARK_READY_COUNT.load(Ordering::Relaxed)
}

pub fn timer_task_pop_count() -> u32 {
    TIMER_TASK_POP_COUNT.load(Ordering::Relaxed)
}

pub fn timer_task_selected_count() -> u32 {
    TIMER_TASK_SELECTED_COUNT.load(Ordering::Relaxed)
}

pub fn timer_task_sleep_count() -> u32 {
    TIMER_TASK_SLEEP_COUNT.load(Ordering::Relaxed)
}

pub fn timer_task_sleep_true_count() -> u32 {
    TIMER_TASK_SLEEP_TRUE_COUNT.load(Ordering::Relaxed)
}

pub fn timer_task_sleep_false_count() -> u32 {
    TIMER_TASK_SLEEP_FALSE_COUNT.load(Ordering::Relaxed)
}

pub fn timer_task_sleep_last_task_ptr() -> usize {
    TIMER_TASK_SLEEP_LAST_TASK_PTR.load(Ordering::Relaxed)
}

pub fn timer_task_sleep_last_wake_at_us() -> u64 {
    TIMER_TASK_SLEEP_LAST_WAKE_AT_US.load(Ordering::Relaxed)
}

pub fn timer_task_sleep_last_result() -> bool {
    TIMER_TASK_SLEEP_LAST_RESULT.load(Ordering::Relaxed)
}

pub fn timer_task_sleep_task_mismatch_count() -> u32 {
    TIMER_TASK_SLEEP_TASK_MISMATCH_COUNT.load(Ordering::Relaxed)
}

pub fn timer_task_ptr() -> usize {
    TIMER_TASK_PTR.load(Ordering::Relaxed)
}

pub fn note_timer_task_mark_ready(task: TaskPtr) {
    if TIMER_TASK_PTR.load(Ordering::Relaxed) == task.as_ptr() as usize {
        TIMER_TASK_MARK_READY_COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn note_timer_task_popped(task: TaskPtr) {
    if TIMER_TASK_PTR.load(Ordering::Relaxed) == task.as_ptr() as usize {
        TIMER_TASK_POP_COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn note_timer_task_selected(task: TaskPtr) {
    if TIMER_TASK_PTR.load(Ordering::Relaxed) == task.as_ptr() as usize {
        TIMER_TASK_SELECTED_COUNT.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn timer_callback_exec_diag() -> TimerCallbackExecDiag {
    let mut recent_ordinals = [0u32; TIMER_EXEC_RING_CAP];
    let mut recent_callback_ptrs = [0usize; TIMER_EXEC_RING_CAP];
    let mut recent_arg_ptrs = [0usize; TIMER_EXEC_RING_CAP];
    let mut recent_exec_at_us = [0u32; TIMER_EXEC_RING_CAP];
    let mut recent_due_at_us = [0u32; TIMER_EXEC_RING_CAP];
    let mut recent_timeout_us = [0u32; TIMER_EXEC_RING_CAP];
    let mut recent_lateness_us = [0u32; TIMER_EXEC_RING_CAP];
    for idx in 0..TIMER_EXEC_RING_CAP {
        recent_ordinals[idx] = TIMER_CALLBACK_RECENT_ORDINALS[idx].load(Ordering::Relaxed);
        recent_callback_ptrs[idx] = TIMER_CALLBACK_RECENT_PTRS[idx].load(Ordering::Relaxed);
        recent_arg_ptrs[idx] = TIMER_CALLBACK_RECENT_ARG_PTRS[idx].load(Ordering::Relaxed);
        recent_exec_at_us[idx] = TIMER_CALLBACK_RECENT_EXEC_AT_US[idx].load(Ordering::Relaxed);
        recent_due_at_us[idx] = TIMER_CALLBACK_RECENT_DUE_AT_US[idx].load(Ordering::Relaxed);
        recent_timeout_us[idx] =
            TIMER_CALLBACK_RECENT_TIMEOUT_US[idx].load(Ordering::Relaxed);
        recent_lateness_us[idx] =
            TIMER_CALLBACK_RECENT_LATENESS_US[idx].load(Ordering::Relaxed);
    }
    TimerCallbackExecDiag {
        callback_count: TIMER_CALLBACK_EXEC_COUNT.load(Ordering::Relaxed),
        current_callback_ptr: TIMER_CALLBACK_CURRENT_PTR.load(Ordering::Relaxed),
        current_arg_ptr: TIMER_CALLBACK_CURRENT_ARG_PTR.load(Ordering::Relaxed),
        last_callback_ptr: TIMER_CALLBACK_LAST_PTR.load(Ordering::Relaxed),
        last_arg_ptr: TIMER_CALLBACK_LAST_ARG_PTR.load(Ordering::Relaxed),
        last_exec_at_us: TIMER_CALLBACK_LAST_EXEC_AT_US.load(Ordering::Relaxed),
        last_due_at_us: TIMER_CALLBACK_LAST_DUE_AT_US.load(Ordering::Relaxed),
        last_timeout_us: TIMER_CALLBACK_LAST_TIMEOUT_US.load(Ordering::Relaxed),
        last_lateness_us: TIMER_CALLBACK_LAST_LATENESS_US.load(Ordering::Relaxed),
        max_lateness_us: TIMER_CALLBACK_MAX_LATENESS_US.load(Ordering::Relaxed),
        recent_ordinals,
        recent_callback_ptrs,
        recent_arg_ptrs,
        recent_exec_at_us,
        recent_due_at_us,
        recent_timeout_us,
        recent_lateness_us,
        sideeffect_arm_count: TIMER_CALLBACK_SIDEEFFECT_ARM_COUNT.load(Ordering::Relaxed),
        sideeffect_disarm_count: TIMER_CALLBACK_SIDEEFFECT_DISARM_COUNT.load(Ordering::Relaxed),
        sideeffect_last_kind: TIMER_CALLBACK_SIDEEFFECT_LAST_KIND.load(Ordering::Relaxed),
        sideeffect_last_current_ptr: TIMER_CALLBACK_SIDEEFFECT_LAST_CURRENT_PTR
            .load(Ordering::Relaxed),
        sideeffect_last_current_arg_ptr: TIMER_CALLBACK_SIDEEFFECT_LAST_CURRENT_ARG_PTR
            .load(Ordering::Relaxed),
        sideeffect_last_target_timer_ptr: TIMER_CALLBACK_SIDEEFFECT_LAST_TARGET_TIMER_PTR
            .load(Ordering::Relaxed),
        sideeffect_last_target_callback_ptr: TIMER_CALLBACK_SIDEEFFECT_LAST_TARGET_CALLBACK_PTR
            .load(Ordering::Relaxed),
        sideeffect_last_target_arg_ptr: TIMER_CALLBACK_SIDEEFFECT_LAST_TARGET_ARG_PTR
            .load(Ordering::Relaxed),
        sideeffect_last_timeout_us: TIMER_CALLBACK_SIDEEFFECT_LAST_TIMEOUT_US
            .load(Ordering::Relaxed),
        sideeffect_last_repeat: TIMER_CALLBACK_SIDEEFFECT_LAST_REPEAT.load(Ordering::Relaxed),
    }
}

pub fn timer_live_callback_ptr(timer_ptr: usize) -> usize {
    let Some(timer_ptr) = NonNull::new(timer_ptr as *mut ()) else {
        return 0;
    };
    let timer_ptr = timer_ptr.cast();
    let timer = unsafe { Timer::from_ptr(timer_ptr) };
    timer.callback_ptr
}

pub fn timer_live_callback_arg_ptr(timer_ptr: usize) -> usize {
    let Some(timer_ptr) = NonNull::new(timer_ptr as *mut ()) else {
        return 0;
    };
    let timer_ptr = timer_ptr.cast();
    let timer = unsafe { Timer::from_ptr(timer_ptr) };
    timer.callback_arg_ptr
}

pub fn timer_live_is_active(timer_ptr: usize) -> bool {
    let Some(timer_ptr) = NonNull::new(timer_ptr as *mut ()) else {
        return false;
    };
    let timer_ptr = timer_ptr.cast();
    let timer = unsafe { Timer::from_ptr(timer_ptr) };
    TIMER_QUEUE.inner.with(|q| timer.is_active(q))
}

pub fn timer_live_started_us(timer_ptr: usize) -> u64 {
    let Some(timer_ptr) = NonNull::new(timer_ptr as *mut ()) else {
        return 0;
    };
    let timer_ptr = timer_ptr.cast();
    let timer = unsafe { Timer::from_ptr(timer_ptr) };
    TIMER_QUEUE.inner.with(|q| timer.properties(q).started)
}

pub fn timer_live_next_due_us(timer_ptr: usize) -> u64 {
    let Some(timer_ptr) = NonNull::new(timer_ptr as *mut ()) else {
        return 0;
    };
    let timer_ptr = timer_ptr.cast();
    let timer = unsafe { Timer::from_ptr(timer_ptr) };
    TIMER_QUEUE.inner.with(|q| timer.properties(q).next_due)
}

pub fn timer_live_period_us(timer_ptr: usize) -> u64 {
    let Some(timer_ptr) = NonNull::new(timer_ptr as *mut ()) else {
        return 0;
    };
    let timer_ptr = timer_ptr.cast();
    let timer = unsafe { Timer::from_ptr(timer_ptr) };
    TIMER_QUEUE.inner.with(|q| timer.properties(q).period)
}

pub fn timer_live_periodic(timer_ptr: usize) -> bool {
    let Some(timer_ptr) = NonNull::new(timer_ptr as *mut ()) else {
        return false;
    };
    let timer_ptr = timer_ptr.cast();
    let timer = unsafe { Timer::from_ptr(timer_ptr) };
    TIMER_QUEUE.inner.with(|q| timer.properties(q).periodic)
}

pub fn timer_arm_count() -> u32 {
    TIMER_ARM_COUNT.load(Ordering::Relaxed)
}

pub fn timer_arm_recent_ordinal(index: usize) -> u32 {
    TIMER_ARM_RECENT_ORDINALS[index].load(Ordering::Relaxed)
}

pub fn timer_arm_recent_timer_ptr(index: usize) -> usize {
    TIMER_ARM_RECENT_TIMER_PTRS[index].load(Ordering::Relaxed)
}

pub fn timer_arm_recent_callback_ptr(index: usize) -> usize {
    TIMER_ARM_RECENT_CALLBACK_PTRS[index].load(Ordering::Relaxed)
}

pub fn timer_arm_recent_arg_ptr(index: usize) -> usize {
    TIMER_ARM_RECENT_ARG_PTRS[index].load(Ordering::Relaxed)
}

pub fn timer_arm_recent_caller_ptr(index: usize) -> usize {
    TIMER_ARM_RECENT_CALLER_PTRS[index].load(Ordering::Relaxed)
}

pub fn timer_arm_recent_timeout_us(index: usize) -> u64 {
    TIMER_ARM_RECENT_TIMEOUT_US[index].load(Ordering::Relaxed)
}

pub fn timer_arm_recent_periodic(index: usize) -> bool {
    TIMER_ARM_RECENT_PERIODIC[index].load(Ordering::Relaxed)
}

fn note_timer_task_created(task: TimerTaskHandle, source: u32, mode: u32) {
    TIMER_TASK_CREATE_COUNT.fetch_add(1, Ordering::Relaxed);
    match source {
        1 => {
            TIMER_TASK_CREATE_FROM_ENSURE_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        2 => {
            TIMER_TASK_CREATE_FROM_WAKE_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        3 => {
            TIMER_TASK_CREATE_FROM_ENQUEUE_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        _ => {}
    }
    TIMER_TASK_CREATE_LAST_SOURCE.store(source, Ordering::Relaxed);
    TIMER_TASK_CREATE_LAST_MODE.store(mode, Ordering::Relaxed);
    TIMER_TASK_CREATE_LAST_PTR.store(task.ptr(), Ordering::Relaxed);
}

pub fn ensure_timer_task() {
    TIMER_QUEUE.inner.with(|q| {
        if q.task.is_none() {
            let task = create_timer_task();
            note_timer_task_created(task, 1, task.mode_code());
            TIMER_TASK_PTR.store(task.ptr(), Ordering::Relaxed);
            q.task = Some(task);
            q.next_wakeup = u64::MAX;
        }
    });
}

pub fn ensure_legacy_timer_task() {
    TIMER_QUEUE.inner.with(|q| {
        if q.task.is_none() {
            let task = create_legacy_timer_task();
            TIMER_TASK_PTR.store(task.ptr(), Ordering::Relaxed);
            q.task = Some(task);
            q.next_wakeup = u64::MAX;
        }
    });
}

pub fn wake_timer_task() {
    TIMER_QUEUE.inner.with(|q| {
        if let Some(task) = q.task {
            TIMER_TASK_RESUME_COUNT.fetch_add(1, Ordering::Relaxed);
            wake_timer_task_handle(task);
        } else {
            let task = create_timer_task();
            note_timer_task_created(task, 2, task.mode_code());
            TIMER_TASK_PTR.store(task.ptr(), Ordering::Relaxed);
            q.task = Some(task);
            q.next_wakeup = u64::MAX;
        }
    });
}

fn update_max_lateness_us(candidate: u32) {
    let mut current = TIMER_CALLBACK_MAX_LATENESS_US.load(Ordering::Relaxed);
    while candidate > current {
        match TIMER_CALLBACK_MAX_LATENESS_US.compare_exchange_weak(
            current,
            candidate,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }
}

fn record_recent_exec(
    ordinal: u32,
    callback_ptr: usize,
    arg_ptr: usize,
    exec_at_us: u32,
    due_at_us: u32,
    timeout_us: u32,
    lateness_us: u32,
) {
    let idx = (ordinal as usize) % TIMER_EXEC_RING_CAP;
    TIMER_CALLBACK_RECENT_ORDINALS[idx].store(ordinal, Ordering::Relaxed);
    TIMER_CALLBACK_RECENT_PTRS[idx].store(callback_ptr, Ordering::Relaxed);
    TIMER_CALLBACK_RECENT_ARG_PTRS[idx].store(arg_ptr, Ordering::Relaxed);
    TIMER_CALLBACK_RECENT_EXEC_AT_US[idx].store(exec_at_us, Ordering::Relaxed);
    TIMER_CALLBACK_RECENT_DUE_AT_US[idx].store(due_at_us, Ordering::Relaxed);
    TIMER_CALLBACK_RECENT_TIMEOUT_US[idx].store(timeout_us, Ordering::Relaxed);
    TIMER_CALLBACK_RECENT_LATENESS_US[idx].store(lateness_us, Ordering::Relaxed);
}

fn record_timer_callback_sideeffect(
    kind: u32,
    target_timer_ptr: usize,
    target_callback_ptr: usize,
    target_arg_ptr: usize,
    timeout_us: u64,
    repeat: bool,
) {
    let current_callback_ptr = TIMER_CALLBACK_CURRENT_PTR.load(Ordering::Relaxed);
    if current_callback_ptr == 0 {
        return;
    }
    let current_arg_ptr = TIMER_CALLBACK_CURRENT_ARG_PTR.load(Ordering::Relaxed);
    match kind {
        1 => {
            TIMER_CALLBACK_SIDEEFFECT_ARM_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        2 => {
            TIMER_CALLBACK_SIDEEFFECT_DISARM_COUNT.fetch_add(1, Ordering::Relaxed);
        }
        _ => {}
    }
    TIMER_CALLBACK_SIDEEFFECT_LAST_KIND.store(kind, Ordering::Relaxed);
    TIMER_CALLBACK_SIDEEFFECT_LAST_CURRENT_PTR.store(current_callback_ptr, Ordering::Relaxed);
    TIMER_CALLBACK_SIDEEFFECT_LAST_CURRENT_ARG_PTR.store(current_arg_ptr, Ordering::Relaxed);
    TIMER_CALLBACK_SIDEEFFECT_LAST_TARGET_TIMER_PTR.store(target_timer_ptr, Ordering::Relaxed);
    TIMER_CALLBACK_SIDEEFFECT_LAST_TARGET_CALLBACK_PTR
        .store(target_callback_ptr, Ordering::Relaxed);
    TIMER_CALLBACK_SIDEEFFECT_LAST_TARGET_ARG_PTR.store(target_arg_ptr, Ordering::Relaxed);
    TIMER_CALLBACK_SIDEEFFECT_LAST_TIMEOUT_US.store(timeout_us, Ordering::Relaxed);
    TIMER_CALLBACK_SIDEEFFECT_LAST_REPEAT.store(repeat, Ordering::Relaxed);
}

struct TimerQueueInner {
    // A linked list of active timers
    head: Option<NonNull<Timer>>,
    next_wakeup: u64,
    task: Option<TimerTaskHandle>,
}

unsafe impl Send for TimerQueueInner {}

impl TimerQueueInner {
    const fn new() -> Self {
        Self {
            head: None,
            next_wakeup: 0,
            task: None,
        }
    }

    fn enqueue(&mut self, timer: &Timer) {
        let head = self.head;
        let props = timer.properties(self);
        let due = props.next_due;

        if !props.enqueued {
            props.enqueued = true;

            props.next = head;
            self.head = Some(NonNull::from(timer));
        }

        if let Some(task) = self.task {
            if due < self.next_wakeup {
                self.next_wakeup = due;
                TIMER_TASK_RESUME_COUNT.fetch_add(1, Ordering::Relaxed);
                wake_timer_task_handle(task);
            }
        } else {
            // create the timer task
            let task = create_timer_task();
            note_timer_task_created(task, 3, task.mode_code());
            TIMER_TASK_PTR.store(task.ptr(), Ordering::Relaxed);
            self.task = Some(task);
            self.next_wakeup = due;
        }
    }

    fn dequeue(&mut self, timer: &Timer) -> bool {
        let mut current = self.head;
        let mut prev: Option<NonNull<Timer>> = None;

        // Scan through the queue until we find the timer
        while let Some(current_timer) = current {
            if core::ptr::eq(current_timer.as_ptr(), timer) {
                // If we find the timer, remove it from the queue by bypassing it in the linked
                // list. The previous element, if any, will point at the next element.

                let timer_props = timer.properties(self);
                let next = timer_props.next.take();
                timer_props.enqueued = false;

                if let Some(mut p) = prev {
                    unsafe { p.as_mut().properties(self).next = next };
                } else {
                    self.head = next;
                }
                return true;
            }

            prev = current;
            current = unsafe { current_timer.as_ref().properties(self).next };
        }

        false
    }
}

fn create_timer_task() -> TimerTaskHandle {
    if backend_legacy_port_runtime_enabled() {
        crate::esp_radio::legacy_preempt_builtin::enable();
        let task_handle =
            crate::esp_radio::legacy_preempt_builtin::task_create(
                legacy_timer_task,
                core::ptr::null_mut(),
                8192,
            );
        return TimerTaskHandle::Legacy(task_handle);
    }
    if legacy_builtin_scheduler_runtime_mode_enabled() {
        crate::esp_radio::legacy_builtin_scheduler::allocate_main_task();
        let task_handle = crate::esp_radio::legacy_builtin_scheduler::task_create(
            "timer",
            timer_task,
            core::ptr::null_mut(),
            8192,
        );
        TimerTaskHandle::Legacy(task_handle)
    } else {
        let task_ptr =
            SCHEDULER.create_task("timer", timer_task, core::ptr::null_mut(), 8192, 2, None);
        legacy_scheduler::note_created_task("timer", task_ptr);
        TimerTaskHandle::Modern(task_ptr)
    }
}

fn create_legacy_timer_task() -> TimerTaskHandle {
    if backend_legacy_port_runtime_enabled() {
        crate::esp_radio::legacy_preempt_builtin::enable();
        let task_handle = crate::esp_radio::legacy_preempt_builtin::task_create(
            legacy_timer_task,
            core::ptr::null_mut(),
            8192,
        );
        return TimerTaskHandle::Legacy(task_handle);
    }
    if legacy_builtin_scheduler_runtime_mode_enabled() {
        crate::esp_radio::legacy_builtin_scheduler::allocate_main_task();
        let task_handle = crate::esp_radio::legacy_builtin_scheduler::task_create(
            "timer",
            legacy_timer_task,
            core::ptr::null_mut(),
            8192,
        );
        TimerTaskHandle::Legacy(task_handle)
    } else {
        let task_ptr = SCHEDULER.create_task(
            "timer",
            legacy_timer_task,
            core::ptr::null_mut(),
            8192,
            2,
            None,
        );
        legacy_scheduler::note_created_task("timer", task_ptr);
        TimerTaskHandle::Modern(task_ptr)
    }
}

fn wake_timer_task_handle(task: TimerTaskHandle) {
    match task {
        TimerTaskHandle::Modern(task) => task.resume(),
        TimerTaskHandle::Legacy(_) => crate::task::yield_task(),
    }
}

struct TimerQueue {
    inner: NonReentrantMutex<TimerQueueInner>,
}

unsafe impl Send for TimerQueue {}

impl TimerQueue {
    const fn new() -> Self {
        Self {
            inner: NonReentrantMutex::new(TimerQueueInner::new()),
        }
    }

    /// Trigger due timers.
    ///
    /// The timer queue needs to be re-processed when a new timer is armed, because the new timer
    /// may need to be triggered before the next scheduled wakeup.
    fn process(&self) {
        debug!("Processing timers");
        let mut timers = self.inner.with(|q| {
            q.next_wakeup = u64::MAX;
            q.head.take()
        });

        while let Some(current) = timers {
            debug!("Checking timer: {:x}", current.addr());
            let current_timer = unsafe { current.as_ref() };

            let run_callback = self.inner.with(|q| {
                let props = current_timer.properties(q);

                // Remove current timer from the list.
                timers = props.next.take();

                if !props.is_active || props.drop {
                    TIMER_TASK_PROCESS_SKIP_INACTIVE_COUNT.fetch_add(1, Ordering::Relaxed);
                    TIMER_TASK_PROCESS_LAST_SKIP_CALLBACK_PTR
                        .store(current_timer.callback_ptr, Ordering::Relaxed);
                    TIMER_TASK_PROCESS_LAST_SKIP_ARG_PTR
                        .store(current_timer.callback_arg_ptr, Ordering::Relaxed);
                    TIMER_TASK_PROCESS_LAST_SKIP_NOW_US.store(crate::now() as u32, Ordering::Relaxed);
                    TIMER_TASK_PROCESS_LAST_SKIP_DUE_US
                        .store(props.next_due as u32, Ordering::Relaxed);
                    debug!(
                        "Timer {:x} is inactive or dropped",
                        current_timer as *const _ as usize
                    );
                    return None;
                }

                let now = crate::now();
                if props.next_due > now {
                    TIMER_TASK_PROCESS_SKIP_NOT_DUE_COUNT.fetch_add(1, Ordering::Relaxed);
                    TIMER_TASK_PROCESS_LAST_SKIP_CALLBACK_PTR
                        .store(current_timer.callback_ptr, Ordering::Relaxed);
                    TIMER_TASK_PROCESS_LAST_SKIP_ARG_PTR
                        .store(current_timer.callback_arg_ptr, Ordering::Relaxed);
                    TIMER_TASK_PROCESS_LAST_SKIP_NOW_US.store(now as u32, Ordering::Relaxed);
                    TIMER_TASK_PROCESS_LAST_SKIP_DUE_US
                        .store(props.next_due as u32, Ordering::Relaxed);
                    // Not our time yet.
                    debug!(
                        "Timer {:x} is not due yet",
                        current_timer as *const _ as usize
                    );
                    return None;
                }

                let due_at_us = props.next_due as u32;
                let timeout_us = props.period as u32;

                if props.periodic {
                    props.next_due += props.period;
                }
                props.is_active = props.periodic;
                Some((due_at_us, timeout_us))
            });

            if let Some((due_at_us, timeout_us)) = run_callback {
                debug!("Triggering timer: {:x}", current_timer as *const _ as usize);
                let exec_at_us = crate::now() as u32;
                let lateness_us = exec_at_us.saturating_sub(due_at_us);
                let ordinal = TIMER_CALLBACK_EXEC_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
                TIMER_CALLBACK_CURRENT_PTR
                    .store(current_timer.callback_ptr, Ordering::Relaxed);
                TIMER_CALLBACK_CURRENT_ARG_PTR
                    .store(current_timer.callback_arg_ptr, Ordering::Relaxed);
                TIMER_CALLBACK_LAST_PTR.store(current_timer.callback_ptr, Ordering::Relaxed);
                TIMER_CALLBACK_LAST_ARG_PTR.store(current_timer.callback_arg_ptr, Ordering::Relaxed);
                TIMER_CALLBACK_LAST_EXEC_AT_US.store(exec_at_us, Ordering::Relaxed);
                TIMER_CALLBACK_LAST_DUE_AT_US.store(due_at_us, Ordering::Relaxed);
                TIMER_CALLBACK_LAST_TIMEOUT_US.store(timeout_us, Ordering::Relaxed);
                TIMER_CALLBACK_LAST_LATENESS_US.store(lateness_us, Ordering::Relaxed);
                update_max_lateness_us(lateness_us);
                record_recent_exec(
                    ordinal,
                    current_timer.callback_ptr,
                    current_timer.callback_arg_ptr,
                    exec_at_us,
                    due_at_us,
                    timeout_us,
                    lateness_us,
                );
                (current_timer.callback.borrow_mut())();
                TIMER_CALLBACK_CURRENT_PTR.store(0, Ordering::Relaxed);
                TIMER_CALLBACK_CURRENT_ARG_PTR.store(0, Ordering::Relaxed);
            }

            self.inner.with(|q| {
                let props = current_timer.properties(q);
                // Set this AFTER the callback so that the callback doesn't leave us in an unknown
                // "queued?" state.
                props.enqueued = false;

                if props.drop {
                    debug!("Dropping timer {:x} (delayed)", current.addr());
                    let boxed = unsafe { Box::from_raw(current.as_ptr()) };
                    core::mem::drop(boxed);
                } else if props.is_active {
                    let next_due = props.next_due;
                    if next_due < q.next_wakeup {
                        q.next_wakeup = next_due;
                    }

                    debug!("Re-queueing timer {:x}", current_timer as *const _ as usize);
                    q.enqueue(current_timer);
                } else {
                    debug!("Timer {:x} inactive", current_timer as *const _ as usize);
                }
            });
        }

        self.inner.with(|q| {
            let next_wakeup = q.next_wakeup;
            debug!("next_wakeup: {}", next_wakeup);
            if use_legacy_timer_loop_diag_enabled() {
                crate::task::yield_task();
            } else {
                let current_task_ptr = crate::task::current_task().as_ptr() as usize;
                let timer_task_ptr = TIMER_TASK_PTR.load(Ordering::Relaxed);
                let wake_at = Instant::EPOCH + Duration::from_micros(next_wakeup);
                TIMER_TASK_SLEEP_COUNT.fetch_add(1, Ordering::Relaxed);
                TIMER_TASK_SLEEP_LAST_TASK_PTR.store(current_task_ptr, Ordering::Relaxed);
                TIMER_TASK_SLEEP_LAST_WAKE_AT_US.store(next_wakeup, Ordering::Relaxed);
                if current_task_ptr != 0 && timer_task_ptr != 0 && current_task_ptr != timer_task_ptr
                {
                    TIMER_TASK_SLEEP_TASK_MISMATCH_COUNT.fetch_add(1, Ordering::Relaxed);
                }
                let slept = SCHEDULER.sleep_until(wake_at);
                TIMER_TASK_SLEEP_LAST_RESULT.store(slept, Ordering::Relaxed);
                if slept {
                    TIMER_TASK_SLEEP_TRUE_COUNT.fetch_add(1, Ordering::Relaxed);
                } else {
                    TIMER_TASK_SLEEP_FALSE_COUNT.fetch_add(1, Ordering::Relaxed);
                }
            }
        });
    }

    fn process_legacy_style(&self) {
        let maybe_due = self.inner.with(|q| {
            let now = crate::now();
            let mut current = q.head;

            while let Some(timer_ptr) = current {
                let timer = unsafe { timer_ptr.as_ref() };
                let props = timer.properties(q);
                let next = props.next;

                if props.is_active && now.wrapping_sub(props.started) >= props.period {
                    let due_at_us = props.next_due as u32;
                    let timeout_us = props.period as u32;

                    if props.periodic {
                        props.started = now;
                        props.next_due += props.period;
                    }
                    props.is_active = props.periodic;

                    return Some((
                        timer_ptr,
                        due_at_us,
                        timeout_us,
                        timer.callback_ptr,
                        timer.callback_arg_ptr,
                    ));
                }

                current = next;
            }

            None
        });

        if let Some((timer_ptr, due_at_us, timeout_us, callback_ptr, callback_arg_ptr)) = maybe_due {
            let exec_at_us = crate::now() as u32;
            let lateness_us = exec_at_us.saturating_sub(due_at_us);
            let ordinal = TIMER_CALLBACK_EXEC_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            TIMER_CALLBACK_CURRENT_PTR.store(callback_ptr, Ordering::Relaxed);
            TIMER_CALLBACK_CURRENT_ARG_PTR.store(callback_arg_ptr, Ordering::Relaxed);
            TIMER_CALLBACK_LAST_PTR.store(callback_ptr, Ordering::Relaxed);
            TIMER_CALLBACK_LAST_ARG_PTR.store(callback_arg_ptr, Ordering::Relaxed);
            TIMER_CALLBACK_LAST_EXEC_AT_US.store(exec_at_us, Ordering::Relaxed);
            TIMER_CALLBACK_LAST_DUE_AT_US.store(due_at_us, Ordering::Relaxed);
            TIMER_CALLBACK_LAST_TIMEOUT_US.store(timeout_us, Ordering::Relaxed);
            TIMER_CALLBACK_LAST_LATENESS_US.store(lateness_us, Ordering::Relaxed);
            update_max_lateness_us(lateness_us);
            record_recent_exec(
                ordinal,
                callback_ptr,
                callback_arg_ptr,
                exec_at_us,
                due_at_us,
                timeout_us,
                lateness_us,
            );

            let timer = unsafe { timer_ptr.as_ref() };
            (timer.callback.borrow_mut())();

            TIMER_CALLBACK_CURRENT_PTR.store(0, Ordering::Relaxed);
            TIMER_CALLBACK_CURRENT_ARG_PTR.store(0, Ordering::Relaxed);

            self.inner.with(|q| {
                let timer = unsafe { timer_ptr.as_ref() };
                let props = timer.properties(q);
                if props.drop {
                    q.dequeue(timer);
                    let boxed = unsafe { Box::from_raw(timer_ptr.as_ptr()) };
                    core::mem::drop(boxed);
                }
            });
        } else {
            crate::task::yield_task();
        }
    }
}

struct TimerProperties {
    is_active: bool,
    started: u64,
    next_due: u64,
    period: u64,
    periodic: bool,
    drop: bool,

    enqueued: bool,
    next: Option<NonNull<Timer>>,
}

struct TimerQueueCell<T>(UnsafeCell<T>);

impl<T> TimerQueueCell<T> {
    const fn new(inner: T) -> Self {
        Self(UnsafeCell::new(inner))
    }

    fn get_mut<'a>(&'a self, _q: &'a mut TimerQueueInner) -> &'a mut T {
        unsafe { &mut *self.0.get() }
    }
}

pub struct Timer {
    callback: RefCell<Box<dyn FnMut() + Send>>,
    callback_ptr: usize,
    callback_arg_ptr: usize,
    // Timer properties, not available in `callback` due to how the timer is constructed.
    timer_properties: TimerQueueCell<TimerProperties>,
}

impl Timer {
    pub fn new(callback: Box<dyn FnMut() + Send>, callback_ptr: usize, callback_arg_ptr: usize) -> Self {
        Timer {
            callback: RefCell::new(callback),
            callback_ptr,
            callback_arg_ptr,
            timer_properties: TimerQueueCell::new(TimerProperties {
                is_active: false,
                started: 0,
                next_due: 0,
                period: 0,
                periodic: false,
                drop: false,

                enqueued: false,
                next: None,
            }),
        }
    }

    unsafe fn from_ptr<'a>(ptr: TimerPtr) -> &'a Self {
        unsafe { ptr.cast::<Self>().as_mut() }
    }

    fn arm(&self, q: &mut TimerQueueInner, timeout: u64, periodic: bool) {
        let now = crate::now();
        let next_due = now + timeout;

        let props = self.properties(q);
        props.is_active = true;
        props.started = now;
        props.next_due = next_due;
        props.period = timeout;
        props.periodic = periodic;

        q.enqueue(self);
    }

    fn is_active(&self, q: &mut TimerQueueInner) -> bool {
        self.properties(q).is_active
    }

    fn disarm(&self, q: &mut TimerQueueInner) {
        self.properties(q).is_active = false;

        // We don't dequeue the timer - processing the queue will just skip it. If we re-arm,
        // the timer may already be in the queue.
    }

    fn properties<'a>(&'a self, q: &'a mut TimerQueueInner) -> &'a mut TimerProperties {
        self.timer_properties.get_mut(q)
    }
}

impl TimerImplementation for Timer {
    fn create(func: unsafe extern "C" fn(*mut c_void), data: *mut c_void) -> TimerPtr {
        // TODO: get rid of the inner box (or its heap allocation) somehow
        struct CCallback {
            func: unsafe extern "C" fn(*mut c_void),
            data: *mut c_void,
        }
        unsafe impl Send for CCallback {}

        impl CCallback {
            unsafe fn call(&mut self) {
                unsafe { (self.func)(self.data) }
            }
        }

        let mut callback = CCallback { func, data };

        let timer = Box::new(Timer::new(
            Box::new(move || unsafe { callback.call() }),
            func as usize,
            data as usize,
        ));
        let ptr = NonNull::from(Box::leak(timer)).cast();
        debug!("Created timer: {:x}", ptr.addr());
        ptr
    }

    unsafe fn delete(timer: TimerPtr) {
        debug!("Deleting timer: {:x}", timer.addr());
        TIMER_QUEUE.inner.with(|q| {
            let timer = unsafe { Box::from_raw(timer.cast::<Timer>().as_ptr()) };

            // There are two cases:
            // - We can remove the timer from the queue - we can drop it.
            // - We can't remove the timer from the queue. There are the following cases:
            //   - The timer isn't in the queue. We can drop it.
            //   - The timer is in the queue and the queue is being processed. We need to mark it to
            //     be dropped by the timer queue.
            if q.dequeue(&timer) {
                core::mem::drop(timer);
            } else {
                timer.properties(q).drop = true;
                core::mem::forget(timer);
            }
        })
    }

    unsafe fn arm(timer: TimerPtr, timeout: u64, periodic: bool) {
        debug!(
            "Arming {:?} for {} us, periodic = {:?}",
            timer, timeout, periodic
        );
        let timer = unsafe { Timer::from_ptr(timer) };
        record_timer_callback_sideeffect(
            1,
            timer as *const Timer as usize,
            timer.callback_ptr,
            timer.callback_arg_ptr,
            timeout,
            periodic,
        );
        record_timer_arm(
            timer as *const Timer as usize,
            timer.callback_ptr,
            timer.callback_arg_ptr,
            current_arm_caller_ptr(),
            timeout,
            periodic,
        );
        TIMER_QUEUE.inner.with(|q| timer.arm(q, timeout, periodic))
    }

    unsafe fn is_active(timer: TimerPtr) -> bool {
        debug!("Checking if timer {:?} is active", timer);
        let timer = unsafe { Timer::from_ptr(timer) };
        TIMER_QUEUE.inner.with(|q| timer.is_active(q))
    }

    unsafe fn disarm(timer: TimerPtr) {
        debug!("Disarming {:?}", timer);
        let timer = unsafe { Timer::from_ptr(timer) };
        record_timer_callback_sideeffect(
            2,
            timer as *const Timer as usize,
            timer.callback_ptr,
            timer.callback_arg_ptr,
            0,
            false,
        );
        TIMER_QUEUE.inner.with(|q| timer.disarm(q))
    }
}

register_timer_implementation!(Timer);

/// Entry point for the timer task responsible for handling scheduled timer
/// events.
///
/// The timer task is created when the first timer is armed.
pub(crate) extern "C" fn timer_task(_: *mut c_void) {
    TIMER_TASK_ENTRY_COUNT.fetch_add(1, Ordering::Relaxed);
    loop {
        TIMER_TASK_LOOP_COUNT.fetch_add(1, Ordering::Relaxed);
        if use_legacy_timer_compat_driver_diag_enabled()
            && unsafe { __esp_radio_diag_legacy_timer_compat_enabled() }
        {
            TIMER_TASK_LEGACY_COMPAT_BRANCH_COUNT.fetch_add(1, Ordering::Relaxed);
            if !unsafe { __esp_radio_diag_process_legacy_timer_compat_due() } {
                if use_exact_legacy_timer_compat_loop_diag_enabled() {
                    crate::task::yield_task();
                } else {
                    let delay_us = unsafe { __esp_radio_diag_legacy_timer_compat_next_due_us() };
                    if delay_us == u32::MAX {
                        SCHEDULER.sleep_until(Instant::EPOCH + Duration::MAX);
                    } else if delay_us == 0 {
                        crate::task::yield_task();
                    } else {
                        SCHEDULER
                            .sleep_until(Instant::now() + Duration::from_micros(delay_us as u64));
                    }
                }
            }
        } else if use_legacy_timer_task_driver_diag_enabled() {
            TIMER_TASK_LEGACY_DRIVER_BRANCH_COUNT.fetch_add(1, Ordering::Relaxed);
            TIMER_QUEUE.process_legacy_style();
        } else {
            TIMER_TASK_DEFAULT_BRANCH_COUNT.fetch_add(1, Ordering::Relaxed);
            TIMER_QUEUE.process();
        }
    }
}

pub(crate) extern "C" fn legacy_timer_task(_: *mut c_void) {
    TIMER_TASK_ENTRY_COUNT.fetch_add(1, Ordering::Relaxed);
    loop {
        TIMER_TASK_LOOP_COUNT.fetch_add(1, Ordering::Relaxed);
        TIMER_TASK_LEGACY_COMPAT_BRANCH_COUNT.fetch_add(1, Ordering::Relaxed);
        if !unsafe { __esp_radio_diag_process_legacy_timer_compat_due() } {
            crate::task::yield_task();
        }
    }
}
#[derive(Clone, Copy)]
enum TimerTaskHandle {
    Modern(TaskPtr),
    Legacy(*mut c_void),
}

impl TimerTaskHandle {
    fn ptr(self) -> usize {
        match self {
            Self::Modern(task) => task.as_ptr() as usize,
            Self::Legacy(task) => task as usize,
        }
    }

    fn mode_code(self) -> u32 {
        match self {
            Self::Modern(_) => 1,
            Self::Legacy(_) => 2,
        }
    }
}
