use esp_radio_rtos_driver::timer::TimerHandle;
use portable_atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use crate::{
    binary::{c_types::c_void, include::ets_timer},
    preempt::timer::TimerPtr,
};

const TIMER_COMPAT_RING_CAP: usize = 6;

const fn suppress_nan_dp_timer_arm_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_SUPPRESS_NAN_DP_TIMER_ARM_DIAG"),
        Some(_)
    ) || matches!(option_env!("WIFI_SUPPRESS_NAN_DP_TIMER_ARM_DIAG"), Some(_))
}

const fn suppress_nan_dp_timer_arg1_arm_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_SUPPRESS_NAN_DP_TIMER_ARG1_ARM_DIAG"),
        Some(_)
    ) || matches!(option_env!("WIFI_SUPPRESS_NAN_DP_TIMER_ARG1_ARM_DIAG"), Some(_))
}

const fn suppress_nan_dp_timer_arg0_arm_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_SUPPRESS_NAN_DP_TIMER_ARG0_ARM_DIAG"),
        Some(_)
    ) || matches!(option_env!("WIFI_SUPPRESS_NAN_DP_TIMER_ARG0_ARM_DIAG"), Some(_))
}

const fn suppress_nan_dp_timer_arg1_setfn_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_SUPPRESS_NAN_DP_TIMER_ARG1_SETFN_DIAG"),
        Some(_)
    ) || matches!(option_env!("WIFI_SUPPRESS_NAN_DP_TIMER_ARG1_SETFN_DIAG"), Some(_))
}

unsafe extern "C" {
    fn nan_dp_schedule_ndc_start();
    fn chm_mhz2num();
}

#[derive(Clone, Copy)]
pub struct TimerCompatDiag {
    pub setfn_count: u32,
    pub arm_count: u32,
    pub wrapper_arm_count: u32,
    pub last_ets_timer_ptr: usize,
    pub last_timer_handle_ptr: usize,
    pub last_callback_ptr: usize,
    pub last_arg_ptr: usize,
    pub last_arm_us: u32,
    pub last_arm_repeat: bool,
    pub recent_setfn_ordinals: [u32; TIMER_COMPAT_RING_CAP],
    pub recent_setfn_ets_timer_ptrs: [usize; TIMER_COMPAT_RING_CAP],
    pub recent_setfn_timer_handle_ptrs: [usize; TIMER_COMPAT_RING_CAP],
    pub recent_setfn_callback_ptrs: [usize; TIMER_COMPAT_RING_CAP],
    pub recent_setfn_arg_ptrs: [usize; TIMER_COMPAT_RING_CAP],
    pub recent_arm_ordinals: [u32; TIMER_COMPAT_RING_CAP],
    pub recent_arm_ets_timer_ptrs: [usize; TIMER_COMPAT_RING_CAP],
    pub recent_arm_timer_handle_ptrs: [usize; TIMER_COMPAT_RING_CAP],
    pub recent_arm_callback_ptrs: [usize; TIMER_COMPAT_RING_CAP],
    pub recent_arm_arg_ptrs: [usize; TIMER_COMPAT_RING_CAP],
    pub recent_arm_caller_ptrs: [usize; TIMER_COMPAT_RING_CAP],
    pub recent_wrapper_arm_ordinals: [u32; TIMER_COMPAT_RING_CAP],
    pub recent_wrapper_arm_caller_ptrs: [usize; TIMER_COMPAT_RING_CAP],
    pub recent_wrapper_arm_timer_ptrs: [usize; TIMER_COMPAT_RING_CAP],
    pub recent_wrapper_arm_us: [u32; TIMER_COMPAT_RING_CAP],
    pub recent_wrapper_arm_repeat: [bool; TIMER_COMPAT_RING_CAP],
    pub recent_arm_us: [u32; TIMER_COMPAT_RING_CAP],
    pub recent_arm_repeat: [bool; TIMER_COMPAT_RING_CAP],
    pub suppressed_setfn_count: u32,
    pub last_suppressed_setfn_callback_ptr: usize,
    pub last_suppressed_setfn_arg_ptr: usize,
    pub suppressed_arm_count: u32,
    pub last_suppressed_callback_ptr: usize,
    pub last_suppressed_arg_ptr: usize,
    pub last_suppressed_us: u32,
}

static TIMER_COMPAT_SETFN_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_COMPAT_ARM_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_COMPAT_WRAPPER_ARM_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_COMPAT_LAST_ETS_TIMER_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_COMPAT_LAST_TIMER_HANDLE_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_COMPAT_LAST_CALLBACK_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_COMPAT_LAST_ARG_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_COMPAT_LAST_ARM_US: AtomicU32 = AtomicU32::new(0);
static TIMER_COMPAT_LAST_ARM_REPEAT: AtomicBool = AtomicBool::new(false);
static TIMER_COMPAT_RECENT_SETFN_ORDINALS: [AtomicU32; TIMER_COMPAT_RING_CAP] =
    [const { AtomicU32::new(0) }; TIMER_COMPAT_RING_CAP];
static TIMER_COMPAT_RECENT_SETFN_ETS_TIMER_PTRS: [AtomicUsize; TIMER_COMPAT_RING_CAP] =
    [const { AtomicUsize::new(0) }; TIMER_COMPAT_RING_CAP];
static TIMER_COMPAT_RECENT_SETFN_TIMER_HANDLE_PTRS: [AtomicUsize; TIMER_COMPAT_RING_CAP] =
    [const { AtomicUsize::new(0) }; TIMER_COMPAT_RING_CAP];
static TIMER_COMPAT_RECENT_SETFN_CALLBACK_PTRS: [AtomicUsize; TIMER_COMPAT_RING_CAP] =
    [const { AtomicUsize::new(0) }; TIMER_COMPAT_RING_CAP];
static TIMER_COMPAT_RECENT_SETFN_ARG_PTRS: [AtomicUsize; TIMER_COMPAT_RING_CAP] =
    [const { AtomicUsize::new(0) }; TIMER_COMPAT_RING_CAP];
static TIMER_COMPAT_RECENT_ARM_ORDINALS: [AtomicU32; TIMER_COMPAT_RING_CAP] =
    [const { AtomicU32::new(0) }; TIMER_COMPAT_RING_CAP];
static TIMER_COMPAT_RECENT_ARM_ETS_TIMER_PTRS: [AtomicUsize; TIMER_COMPAT_RING_CAP] =
    [const { AtomicUsize::new(0) }; TIMER_COMPAT_RING_CAP];
static TIMER_COMPAT_RECENT_ARM_TIMER_HANDLE_PTRS: [AtomicUsize; TIMER_COMPAT_RING_CAP] =
    [const { AtomicUsize::new(0) }; TIMER_COMPAT_RING_CAP];
static TIMER_COMPAT_RECENT_ARM_CALLBACK_PTRS: [AtomicUsize; TIMER_COMPAT_RING_CAP] =
    [const { AtomicUsize::new(0) }; TIMER_COMPAT_RING_CAP];
static TIMER_COMPAT_RECENT_ARM_ARG_PTRS: [AtomicUsize; TIMER_COMPAT_RING_CAP] =
    [const { AtomicUsize::new(0) }; TIMER_COMPAT_RING_CAP];
static TIMER_COMPAT_RECENT_ARM_CALLER_PTRS: [AtomicUsize; TIMER_COMPAT_RING_CAP] =
    [const { AtomicUsize::new(0) }; TIMER_COMPAT_RING_CAP];
static TIMER_COMPAT_RECENT_WRAPPER_ARM_ORDINALS: [AtomicU32; TIMER_COMPAT_RING_CAP] =
    [const { AtomicU32::new(0) }; TIMER_COMPAT_RING_CAP];
static TIMER_COMPAT_RECENT_WRAPPER_ARM_CALLER_PTRS: [AtomicUsize; TIMER_COMPAT_RING_CAP] =
    [const { AtomicUsize::new(0) }; TIMER_COMPAT_RING_CAP];
static TIMER_COMPAT_RECENT_WRAPPER_ARM_TIMER_PTRS: [AtomicUsize; TIMER_COMPAT_RING_CAP] =
    [const { AtomicUsize::new(0) }; TIMER_COMPAT_RING_CAP];
static TIMER_COMPAT_RECENT_WRAPPER_ARM_US: [AtomicU32; TIMER_COMPAT_RING_CAP] =
    [const { AtomicU32::new(0) }; TIMER_COMPAT_RING_CAP];
static TIMER_COMPAT_RECENT_WRAPPER_ARM_REPEAT: [AtomicBool; TIMER_COMPAT_RING_CAP] =
    [const { AtomicBool::new(false) }; TIMER_COMPAT_RING_CAP];
static TIMER_COMPAT_RECENT_ARM_US: [AtomicU32; TIMER_COMPAT_RING_CAP] =
    [const { AtomicU32::new(0) }; TIMER_COMPAT_RING_CAP];
static TIMER_COMPAT_RECENT_ARM_REPEAT: [AtomicBool; TIMER_COMPAT_RING_CAP] =
    [const { AtomicBool::new(false) }; TIMER_COMPAT_RING_CAP];
static TIMER_COMPAT_SUPPRESSED_SETFN_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_COMPAT_LAST_SUPPRESSED_SETFN_CALLBACK_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_COMPAT_LAST_SUPPRESSED_SETFN_ARG_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_COMPAT_SUPPRESSED_ARM_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_COMPAT_LAST_SUPPRESSED_CALLBACK_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_COMPAT_LAST_SUPPRESSED_ARG_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_COMPAT_LAST_SUPPRESSED_US: AtomicU32 = AtomicU32::new(0);

fn callback_is_nan_dp_timer_family(callback_ptr: usize) -> bool {
    if callback_ptr == 0 {
        return false;
    }
    let start = nan_dp_schedule_ndc_start as usize;
    let end = chm_mhz2num as usize;
    callback_ptr >= start && callback_ptr < end
}

fn should_suppress_nan_dp_timer_arm(callback_ptr: usize, arg_ptr: usize) -> bool {
    if !callback_is_nan_dp_timer_family(callback_ptr) {
        return false;
    }
    if suppress_nan_dp_timer_arm_enabled() {
        return true;
    }
    if suppress_nan_dp_timer_arg1_arm_enabled() && arg_ptr == 1 {
        return true;
    }
    suppress_nan_dp_timer_arg0_arm_enabled() && arg_ptr == 0
}

fn should_suppress_nan_dp_timer_setfn(callback_ptr: usize, arg_ptr: usize) -> bool {
    suppress_nan_dp_timer_arg1_setfn_enabled()
        && callback_is_nan_dp_timer_family(callback_ptr)
        && arg_ptr == 1
}

unsafe extern "C" fn suppressed_timer_callback(_arg: *mut c_void) {}

fn record_recent_setfn(
    ordinal: u32,
    ets_timer_ptr: usize,
    timer_handle_ptr: usize,
    callback_ptr: usize,
    arg_ptr: usize,
) {
    let idx = (ordinal as usize) % TIMER_COMPAT_RING_CAP;
    TIMER_COMPAT_RECENT_SETFN_ORDINALS[idx].store(ordinal, Ordering::Relaxed);
    TIMER_COMPAT_RECENT_SETFN_ETS_TIMER_PTRS[idx].store(ets_timer_ptr, Ordering::Relaxed);
    TIMER_COMPAT_RECENT_SETFN_TIMER_HANDLE_PTRS[idx].store(timer_handle_ptr, Ordering::Relaxed);
    TIMER_COMPAT_RECENT_SETFN_CALLBACK_PTRS[idx].store(callback_ptr, Ordering::Relaxed);
    TIMER_COMPAT_RECENT_SETFN_ARG_PTRS[idx].store(arg_ptr, Ordering::Relaxed);
}

fn record_recent_arm(
    ordinal: u32,
    ets_timer_ptr: usize,
    timer_handle_ptr: usize,
    callback_ptr: usize,
    arg_ptr: usize,
    caller_ptr: usize,
    us: u32,
    repeat: bool,
) {
    let idx = (ordinal as usize) % TIMER_COMPAT_RING_CAP;
    TIMER_COMPAT_RECENT_ARM_ORDINALS[idx].store(ordinal, Ordering::Relaxed);
    TIMER_COMPAT_RECENT_ARM_ETS_TIMER_PTRS[idx].store(ets_timer_ptr, Ordering::Relaxed);
    TIMER_COMPAT_RECENT_ARM_TIMER_HANDLE_PTRS[idx].store(timer_handle_ptr, Ordering::Relaxed);
    TIMER_COMPAT_RECENT_ARM_CALLBACK_PTRS[idx].store(callback_ptr, Ordering::Relaxed);
    TIMER_COMPAT_RECENT_ARM_ARG_PTRS[idx].store(arg_ptr, Ordering::Relaxed);
    TIMER_COMPAT_RECENT_ARM_CALLER_PTRS[idx].store(caller_ptr, Ordering::Relaxed);
    TIMER_COMPAT_RECENT_ARM_US[idx].store(us, Ordering::Relaxed);
    TIMER_COMPAT_RECENT_ARM_REPEAT[idx].store(repeat, Ordering::Relaxed);
}

pub fn record_wrapper_arm_call(timer_ptr: usize, caller_ptr: usize, us: u32, repeat: bool) {
    let ordinal = TIMER_COMPAT_WRAPPER_ARM_COUNT.fetch_add(1, Ordering::Relaxed);
    let idx = (ordinal as usize) % TIMER_COMPAT_RING_CAP;
    TIMER_COMPAT_RECENT_WRAPPER_ARM_ORDINALS[idx].store(ordinal, Ordering::Relaxed);
    TIMER_COMPAT_RECENT_WRAPPER_ARM_TIMER_PTRS[idx].store(timer_ptr, Ordering::Relaxed);
    TIMER_COMPAT_RECENT_WRAPPER_ARM_CALLER_PTRS[idx].store(caller_ptr, Ordering::Relaxed);
    TIMER_COMPAT_RECENT_WRAPPER_ARM_US[idx].store(us, Ordering::Relaxed);
    TIMER_COMPAT_RECENT_WRAPPER_ARM_REPEAT[idx].store(repeat, Ordering::Relaxed);
}

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

fn lookup_recent_setfn_by_handle(timer_handle_ptr: usize) -> (usize, usize) {
    if timer_handle_ptr == 0 {
        return (0, 0);
    }
    for idx in 0..TIMER_COMPAT_RING_CAP {
        if TIMER_COMPAT_RECENT_SETFN_TIMER_HANDLE_PTRS[idx].load(Ordering::Relaxed) == timer_handle_ptr {
            return (
                TIMER_COMPAT_RECENT_SETFN_CALLBACK_PTRS[idx].load(Ordering::Relaxed),
                TIMER_COMPAT_RECENT_SETFN_ARG_PTRS[idx].load(Ordering::Relaxed),
            );
        }
    }
    (0, 0)
}

pub fn reset_timer_compat_diag() {
    TIMER_COMPAT_SETFN_COUNT.store(0, Ordering::Relaxed);
    TIMER_COMPAT_ARM_COUNT.store(0, Ordering::Relaxed);
    TIMER_COMPAT_WRAPPER_ARM_COUNT.store(0, Ordering::Relaxed);
    TIMER_COMPAT_LAST_ETS_TIMER_PTR.store(0, Ordering::Relaxed);
    TIMER_COMPAT_LAST_TIMER_HANDLE_PTR.store(0, Ordering::Relaxed);
    TIMER_COMPAT_LAST_CALLBACK_PTR.store(0, Ordering::Relaxed);
    TIMER_COMPAT_LAST_ARG_PTR.store(0, Ordering::Relaxed);
    TIMER_COMPAT_LAST_ARM_US.store(0, Ordering::Relaxed);
    TIMER_COMPAT_LAST_ARM_REPEAT.store(false, Ordering::Relaxed);
    for idx in 0..TIMER_COMPAT_RING_CAP {
        TIMER_COMPAT_RECENT_SETFN_ORDINALS[idx].store(0, Ordering::Relaxed);
        TIMER_COMPAT_RECENT_SETFN_ETS_TIMER_PTRS[idx].store(0, Ordering::Relaxed);
        TIMER_COMPAT_RECENT_SETFN_TIMER_HANDLE_PTRS[idx].store(0, Ordering::Relaxed);
        TIMER_COMPAT_RECENT_SETFN_CALLBACK_PTRS[idx].store(0, Ordering::Relaxed);
        TIMER_COMPAT_RECENT_SETFN_ARG_PTRS[idx].store(0, Ordering::Relaxed);
        TIMER_COMPAT_RECENT_ARM_ORDINALS[idx].store(0, Ordering::Relaxed);
        TIMER_COMPAT_RECENT_ARM_ETS_TIMER_PTRS[idx].store(0, Ordering::Relaxed);
        TIMER_COMPAT_RECENT_ARM_TIMER_HANDLE_PTRS[idx].store(0, Ordering::Relaxed);
        TIMER_COMPAT_RECENT_ARM_CALLBACK_PTRS[idx].store(0, Ordering::Relaxed);
        TIMER_COMPAT_RECENT_ARM_ARG_PTRS[idx].store(0, Ordering::Relaxed);
        TIMER_COMPAT_RECENT_ARM_CALLER_PTRS[idx].store(0, Ordering::Relaxed);
        TIMER_COMPAT_RECENT_WRAPPER_ARM_ORDINALS[idx].store(0, Ordering::Relaxed);
        TIMER_COMPAT_RECENT_WRAPPER_ARM_TIMER_PTRS[idx].store(0, Ordering::Relaxed);
        TIMER_COMPAT_RECENT_WRAPPER_ARM_CALLER_PTRS[idx].store(0, Ordering::Relaxed);
        TIMER_COMPAT_RECENT_WRAPPER_ARM_US[idx].store(0, Ordering::Relaxed);
        TIMER_COMPAT_RECENT_WRAPPER_ARM_REPEAT[idx].store(false, Ordering::Relaxed);
        TIMER_COMPAT_RECENT_ARM_US[idx].store(0, Ordering::Relaxed);
        TIMER_COMPAT_RECENT_ARM_REPEAT[idx].store(false, Ordering::Relaxed);
    }
    TIMER_COMPAT_SUPPRESSED_SETFN_COUNT.store(0, Ordering::Relaxed);
    TIMER_COMPAT_LAST_SUPPRESSED_SETFN_CALLBACK_PTR.store(0, Ordering::Relaxed);
    TIMER_COMPAT_LAST_SUPPRESSED_SETFN_ARG_PTR.store(0, Ordering::Relaxed);
    TIMER_COMPAT_SUPPRESSED_ARM_COUNT.store(0, Ordering::Relaxed);
    TIMER_COMPAT_LAST_SUPPRESSED_CALLBACK_PTR.store(0, Ordering::Relaxed);
    TIMER_COMPAT_LAST_SUPPRESSED_ARG_PTR.store(0, Ordering::Relaxed);
    TIMER_COMPAT_LAST_SUPPRESSED_US.store(0, Ordering::Relaxed);
}

pub fn timer_compat_diag() -> TimerCompatDiag {
    TimerCompatDiag {
        setfn_count: TIMER_COMPAT_SETFN_COUNT.load(Ordering::Relaxed),
        arm_count: TIMER_COMPAT_ARM_COUNT.load(Ordering::Relaxed),
        wrapper_arm_count: TIMER_COMPAT_WRAPPER_ARM_COUNT.load(Ordering::Relaxed),
        last_ets_timer_ptr: TIMER_COMPAT_LAST_ETS_TIMER_PTR.load(Ordering::Relaxed),
        last_timer_handle_ptr: TIMER_COMPAT_LAST_TIMER_HANDLE_PTR.load(Ordering::Relaxed),
        last_callback_ptr: TIMER_COMPAT_LAST_CALLBACK_PTR.load(Ordering::Relaxed),
        last_arg_ptr: TIMER_COMPAT_LAST_ARG_PTR.load(Ordering::Relaxed),
        last_arm_us: TIMER_COMPAT_LAST_ARM_US.load(Ordering::Relaxed),
        last_arm_repeat: TIMER_COMPAT_LAST_ARM_REPEAT.load(Ordering::Relaxed),
        recent_setfn_ordinals: core::array::from_fn(|idx| {
            TIMER_COMPAT_RECENT_SETFN_ORDINALS[idx].load(Ordering::Relaxed)
        }),
        recent_setfn_ets_timer_ptrs: core::array::from_fn(|idx| {
            TIMER_COMPAT_RECENT_SETFN_ETS_TIMER_PTRS[idx].load(Ordering::Relaxed)
        }),
        recent_setfn_timer_handle_ptrs: core::array::from_fn(|idx| {
            TIMER_COMPAT_RECENT_SETFN_TIMER_HANDLE_PTRS[idx].load(Ordering::Relaxed)
        }),
        recent_setfn_callback_ptrs: core::array::from_fn(|idx| {
            TIMER_COMPAT_RECENT_SETFN_CALLBACK_PTRS[idx].load(Ordering::Relaxed)
        }),
        recent_setfn_arg_ptrs: core::array::from_fn(|idx| {
            TIMER_COMPAT_RECENT_SETFN_ARG_PTRS[idx].load(Ordering::Relaxed)
        }),
        recent_arm_ordinals: core::array::from_fn(|idx| {
            TIMER_COMPAT_RECENT_ARM_ORDINALS[idx].load(Ordering::Relaxed)
        }),
        recent_arm_ets_timer_ptrs: core::array::from_fn(|idx| {
            TIMER_COMPAT_RECENT_ARM_ETS_TIMER_PTRS[idx].load(Ordering::Relaxed)
        }),
        recent_arm_timer_handle_ptrs: core::array::from_fn(|idx| {
            TIMER_COMPAT_RECENT_ARM_TIMER_HANDLE_PTRS[idx].load(Ordering::Relaxed)
        }),
        recent_arm_callback_ptrs: core::array::from_fn(|idx| {
            TIMER_COMPAT_RECENT_ARM_CALLBACK_PTRS[idx].load(Ordering::Relaxed)
        }),
        recent_arm_arg_ptrs: core::array::from_fn(|idx| {
            TIMER_COMPAT_RECENT_ARM_ARG_PTRS[idx].load(Ordering::Relaxed)
        }),
        recent_arm_caller_ptrs: core::array::from_fn(|idx| {
            TIMER_COMPAT_RECENT_ARM_CALLER_PTRS[idx].load(Ordering::Relaxed)
        }),
        recent_wrapper_arm_ordinals: core::array::from_fn(|idx| {
            TIMER_COMPAT_RECENT_WRAPPER_ARM_ORDINALS[idx].load(Ordering::Relaxed)
        }),
        recent_wrapper_arm_caller_ptrs: core::array::from_fn(|idx| {
            TIMER_COMPAT_RECENT_WRAPPER_ARM_CALLER_PTRS[idx].load(Ordering::Relaxed)
        }),
        recent_wrapper_arm_timer_ptrs: core::array::from_fn(|idx| {
            TIMER_COMPAT_RECENT_WRAPPER_ARM_TIMER_PTRS[idx].load(Ordering::Relaxed)
        }),
        recent_wrapper_arm_us: core::array::from_fn(|idx| {
            TIMER_COMPAT_RECENT_WRAPPER_ARM_US[idx].load(Ordering::Relaxed)
        }),
        recent_wrapper_arm_repeat: core::array::from_fn(|idx| {
            TIMER_COMPAT_RECENT_WRAPPER_ARM_REPEAT[idx].load(Ordering::Relaxed)
        }),
        recent_arm_us: core::array::from_fn(|idx| {
            TIMER_COMPAT_RECENT_ARM_US[idx].load(Ordering::Relaxed)
        }),
        recent_arm_repeat: core::array::from_fn(|idx| {
            TIMER_COMPAT_RECENT_ARM_REPEAT[idx].load(Ordering::Relaxed)
        }),
        suppressed_setfn_count: TIMER_COMPAT_SUPPRESSED_SETFN_COUNT.load(Ordering::Relaxed),
        last_suppressed_setfn_callback_ptr: TIMER_COMPAT_LAST_SUPPRESSED_SETFN_CALLBACK_PTR
            .load(Ordering::Relaxed),
        last_suppressed_setfn_arg_ptr: TIMER_COMPAT_LAST_SUPPRESSED_SETFN_ARG_PTR
            .load(Ordering::Relaxed),
        suppressed_arm_count: TIMER_COMPAT_SUPPRESSED_ARM_COUNT.load(Ordering::Relaxed),
        last_suppressed_callback_ptr: TIMER_COMPAT_LAST_SUPPRESSED_CALLBACK_PTR
            .load(Ordering::Relaxed),
        last_suppressed_arg_ptr: TIMER_COMPAT_LAST_SUPPRESSED_ARG_PTR.load(Ordering::Relaxed),
        last_suppressed_us: TIMER_COMPAT_LAST_SUPPRESSED_US.load(Ordering::Relaxed),
    }
}

pub(crate) fn compat_timer_arm(ets_timer: *mut ets_timer, tmout_ms: u32, repeat: bool) {
    compat_timer_arm_us(ets_timer, tmout_ms.saturating_mul(1000), repeat);
}

pub(crate) fn compat_timer_arm_us(ets_timer: *mut ets_timer, us: u32, repeat: bool) {
    trace!(
        "timer_arm_us {:x} current: {} micros: {} repeat: {}",
        ets_timer as usize,
        crate::preempt::now(),
        us,
        repeat
    );

    let ets_timer = unwrap!(unsafe { ets_timer.as_mut() }, "ets_timer is null");
    let ordinal = TIMER_COMPAT_ARM_COUNT.fetch_add(1, Ordering::Relaxed);
    TIMER_COMPAT_LAST_ETS_TIMER_PTR.store(ets_timer as *mut ets_timer as usize, Ordering::Relaxed);
    TIMER_COMPAT_LAST_ARM_US.store(us, Ordering::Relaxed);
    TIMER_COMPAT_LAST_ARM_REPEAT.store(repeat, Ordering::Relaxed);

    let timer = unwrap!(TimerPtr::new(ets_timer.priv_.cast()), "timer is null");
    TIMER_COMPAT_LAST_TIMER_HANDLE_PTR.store(timer.as_ptr() as usize, Ordering::Relaxed);
    let (callback_ptr, arg_ptr) = lookup_recent_setfn_by_handle(timer.as_ptr() as usize);
    let caller_ptr = current_arm_caller_ptr();
    record_recent_arm(
        ordinal,
        ets_timer as *mut ets_timer as usize,
        timer.as_ptr() as usize,
        callback_ptr,
        arg_ptr,
        caller_ptr,
        us,
        repeat,
    );
    if should_suppress_nan_dp_timer_arm(callback_ptr, arg_ptr) {
        TIMER_COMPAT_SUPPRESSED_ARM_COUNT.fetch_add(1, Ordering::Relaxed);
        TIMER_COMPAT_LAST_SUPPRESSED_CALLBACK_PTR.store(callback_ptr, Ordering::Relaxed);
        TIMER_COMPAT_LAST_SUPPRESSED_ARG_PTR.store(arg_ptr, Ordering::Relaxed);
        TIMER_COMPAT_LAST_SUPPRESSED_US.store(us, Ordering::Relaxed);
        return;
    }
    let timer = unsafe { TimerHandle::ref_from_ptr(&timer) };

    timer.arm(us as u64, repeat);
}

pub(crate) fn compat_timer_disarm(ets_timer: *mut ets_timer) {
    trace!("timer disarm");
    let ets_timer = unwrap!(unsafe { ets_timer.as_mut() }, "ets_timer is null");

    if let Some(timer) = TimerPtr::new(ets_timer.priv_.cast()) {
        let timer = unsafe { TimerHandle::ref_from_ptr(&timer) };

        timer.disarm();
    }
}

pub(crate) fn compat_timer_is_active(ets_timer: *mut ets_timer) -> bool {
    trace!("timer is_active");
    let ets_timer = unwrap!(unsafe { ets_timer.as_mut() }, "ets_timer is null");

    if let Some(timer) = TimerPtr::new(ets_timer.priv_.cast()) {
        let timer = unsafe { TimerHandle::ref_from_ptr(&timer) };

        timer.is_active()
    } else {
        false
    }
}

fn delete_timer(ets_timer: &mut ets_timer) {
    if let Some(timer) = TimerPtr::new(ets_timer.priv_.cast()) {
        let timer = unsafe { TimerHandle::from_ptr(timer) };

        core::mem::drop(timer);
        ets_timer.priv_ = core::ptr::null_mut();
    }
}

pub(crate) fn compat_timer_done(ets_timer: *mut ets_timer) {
    trace!("timer done");

    let ets_timer = unwrap!(unsafe { ets_timer.as_mut() }, "ets_timer is null");

    delete_timer(ets_timer);
}

pub(crate) fn compat_timer_setfn(
    ets_timer: *mut ets_timer,
    pfunction: unsafe extern "C" fn(*mut c_void),
    parg: *mut c_void,
) {
    trace!(
        "timer_setfn {:x} {:?} {:?}",
        ets_timer as usize, pfunction, parg
    );

    let ets_timer = unwrap!(unsafe { ets_timer.as_mut() }, "ets_timer is null");
    let ordinal = TIMER_COMPAT_SETFN_COUNT.fetch_add(1, Ordering::Relaxed);
    TIMER_COMPAT_LAST_ETS_TIMER_PTR.store(ets_timer as *mut ets_timer as usize, Ordering::Relaxed);
    TIMER_COMPAT_LAST_CALLBACK_PTR.store(pfunction as usize, Ordering::Relaxed);
    TIMER_COMPAT_LAST_ARG_PTR.store(parg as usize, Ordering::Relaxed);

    // This function is expected to create timers. For the simplicity of the preempt API, we
    // will not update existing timers, but create new ones.
    delete_timer(ets_timer);

    let effective_function = if should_suppress_nan_dp_timer_setfn(pfunction as usize, parg as usize) {
        TIMER_COMPAT_SUPPRESSED_SETFN_COUNT.fetch_add(1, Ordering::Relaxed);
        TIMER_COMPAT_LAST_SUPPRESSED_SETFN_CALLBACK_PTR.store(pfunction as usize, Ordering::Relaxed);
        TIMER_COMPAT_LAST_SUPPRESSED_SETFN_ARG_PTR.store(parg as usize, Ordering::Relaxed);
        suppressed_timer_callback
    } else {
        pfunction
    };

    let timer = unsafe { TimerHandle::new(effective_function, parg) }
        .leak()
        .cast()
        .as_ptr();
    TIMER_COMPAT_LAST_TIMER_HANDLE_PTR.store(timer as usize, Ordering::Relaxed);
    record_recent_setfn(
        ordinal,
        ets_timer as *mut ets_timer as usize,
        timer as usize,
        pfunction as usize,
        parg as usize,
    );

    ets_timer.next = core::ptr::null_mut();
    ets_timer.period = 0;
    ets_timer.func = None;
    ets_timer.priv_ = timer;
}
