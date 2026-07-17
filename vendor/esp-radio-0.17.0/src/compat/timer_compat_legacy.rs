use alloc::boxed::Box;

use esp_sync::NonReentrantMutex;
use portable_atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::binary::{
    c_types,
    include::ets_timer,
};
use crate::compat::legacy_runtime_policy;
use crate::compat::timer_compat_legacy_diag::{
    diag as legacy_diag, reset_diag as legacy_reset_diag, LegacyTimerDiag, LEGACY_ARM_COUNT,
    LEGACY_EXEC_COUNT, LEGACY_LAST_ARG_PTR, LEGACY_LAST_ARM_REPEAT, LEGACY_LAST_ARM_US,
    LEGACY_LAST_CALLBACK_PTR, LEGACY_LAST_NEXT_DUE_US, LEGACY_LAST_NOW_US,
    LEGACY_LAST_STARTED_US, LEGACY_LAST_TIMEOUT_US, LEGACY_PROCESS_DUE_CALL_COUNT,
    LEGACY_PROCESS_DUE_HIT_COUNT, LEGACY_RECENT_SETFN_ARG_PTRS,
    LEGACY_RECENT_SETFN_CALLER_PTRS, LEGACY_RECENT_SETFN_CALLBACK_PTRS,
    LEGACY_RECENT_SETFN_ETS_TIMER_PTRS, LEGACY_RECENT_SETFN_ORDINALS,
    LEGACY_RECENT_EXEC_ARG_PTRS, LEGACY_RECENT_EXEC_CALLBACK_PTRS,
    LEGACY_RECENT_EXEC_OP_CHANS, LEGACY_RECENT_EXEC_ORDINALS,
    LEGACY_RECENT_EXEC_SCAN_WORD00, LEGACY_RECENT_EXEC_SCAN_WORD114, LEGACY_SETFN_COUNT,
    LEGACY_TIMER_RING_CAP, LEGACY_RECENT_DUE_ORDINALS, LEGACY_RECENT_DUE_FOUND,
    LEGACY_RECENT_DUE_EXECUTED, LEGACY_RECENT_DUE_CALLBACK_PTRS, LEGACY_RECENT_DUE_ARG_PTRS,
    LEGACY_RECENT_DUE_OP_CHANS, LEGACY_RECENT_DUE_SCAN_WORD00, LEGACY_RECENT_DUE_SCAN_WORD114,
};
use crate::compat::timer_compat_legacy_policy::{
    compat_enabled as legacy_timer_compat_enabled, should_suppress_callback_arm,
    should_suppress_callback_setfn,
};

unsafe extern "C" {
    fn __esp_rtos_diag_precreate_esp_radio_timer_task();
    fn __esp_rtos_diag_resume_esp_radio_timer_task();
    fn ieee80211_timer_process(arg: *mut c_types::c_void);
    #[link_name = "ieee80211_timer_process"]
    fn ieee80211_timer_process_scan_step(kind: u32, reason: u32, arg: *mut c_types::c_void);
    fn nan_dp_schedule_ndc_start(arg: *mut c_types::c_void);
    static mut g_chm: u8;
    static mut g_scan: u8;
}

#[cfg(xtensa)]
fn current_setfn_caller_ptr() -> usize {
    let caller_ptr: usize;
    unsafe {
        core::arch::asm!("mov {0}, a0", out(reg) caller_ptr);
    }
    caller_ptr
}

unsafe fn read_u8(ptr: usize, offset: usize) -> u8 {
    ((ptr + offset) as *const u8).read_volatile()
}

unsafe fn read_u32(ptr: usize, offset: usize) -> u32 {
    ((ptr + offset) as *const u32).read_volatile()
}

#[cfg(not(xtensa))]
fn current_setfn_caller_ptr() -> usize {
    0
}

const IEEE80211_TIMER_PROCESS_OFFSET: usize = 0xa0;
const NAN_DP_SCHEDULE_NDC_START_OFFSET: usize = 0x68;

fn nan_timer_slot_retarget_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_NAN_TIMER_SLOT_RETARGET_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_NAN_TIMER_SLOT_RETARGET_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn nan_timer_slot_retarget_trampoline_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_NAN_TIMER_SLOT_RETARGET_TRAMPOLINE_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_NAN_TIMER_SLOT_RETARGET_TRAMPOLINE_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

unsafe extern "C" fn legacy_ieee80211_timer_process_scan_step_trampoline(
    arg: *mut c_types::c_void,
) {
    unsafe {
        ieee80211_timer_process_scan_step(7, 8, arg);
    }
}

fn maybe_retarget_nan_slot_callback(
    pfunction: unsafe extern "C" fn(*mut c_types::c_void),
    parg: *mut c_types::c_void,
) -> unsafe extern "C" fn(*mut c_types::c_void) {
    if !nan_timer_slot_retarget_enabled() {
        return pfunction;
    }

    let arg = parg as usize;
    if arg > 1 {
        return pfunction;
    }

    let source_ptr = nan_dp_schedule_ndc_start as usize + NAN_DP_SCHEDULE_NDC_START_OFFSET;
    if pfunction as usize != source_ptr {
        return pfunction;
    }

    unsafe {
        if nan_timer_slot_retarget_trampoline_enabled() {
            legacy_ieee80211_timer_process_scan_step_trampoline
        } else {
            core::mem::transmute::<usize, unsafe extern "C" fn(*mut c_types::c_void)>(
                ieee80211_timer_process as usize + IEEE80211_TIMER_PROCESS_OFFSET,
            )
        }
    }
}

fn legacy_retarget_trace_enabled() -> bool {
    nan_timer_slot_retarget_enabled()
}

fn legacy_due_trace_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_BOOT_SCAN_ONLY_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_BOOT_SCAN_ONLY_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn legacy_skip_pair_arg1_exec_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_LEGACY_TIMER_SKIP_PAIR_ARG1_EXEC_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_LEGACY_TIMER_SKIP_PAIR_ARG1_EXEC_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn legacy_pair_retarget_target() -> unsafe extern "C" fn(*mut c_types::c_void) {
    unsafe {
        if nan_timer_slot_retarget_trampoline_enabled() {
            legacy_ieee80211_timer_process_scan_step_trampoline
        } else {
            core::mem::transmute::<usize, unsafe extern "C" fn(*mut c_types::c_void)>(
                ieee80211_timer_process as usize + IEEE80211_TIMER_PROCESS_OFFSET,
            )
        }
    }
}

fn legacy_skip_recovery_arg0_exec_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_LEGACY_TIMER_SKIP_RECOVERY_ARG0_EXEC_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_LEGACY_TIMER_SKIP_RECOVERY_ARG0_EXEC_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn try_retarget_arg01_callback_pair(
    timers: &mut TimerQueue,
    source_callback_ptr: usize,
    target_callback: unsafe extern "C" fn(*mut c_types::c_void),
) -> bool {
    let mut saw_arg0 = false;
    let mut saw_arg1 = false;
    let mut current = timers.head.as_mut();
    while let Some(timer) = current {
        if timer.callback.f as usize == source_callback_ptr {
            match timer.callback.args as usize {
                0 => saw_arg0 = true,
                1 => saw_arg1 = true,
                _ => {}
            }
        }
        current = timer.next.as_mut();
    }

    if !(saw_arg0 && saw_arg1) {
        return false;
    }

    let mut changed = false;
    let mut current = timers.head.as_mut();
    while let Some(timer) = current {
        if timer.callback.f as usize == source_callback_ptr {
            match timer.callback.args as usize {
                0 | 1 => {
                    timer.callback.f = target_callback;
                    changed = true;
                }
                _ => {}
            }
        }
        current = timer.next.as_mut();
    }
    changed
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TimerCallback {
    f: unsafe extern "C" fn(*mut c_types::c_void),
    args: *mut c_types::c_void,
}

impl TimerCallback {
    fn new(f: unsafe extern "C" fn(*mut c_types::c_void), args: *mut c_types::c_void) -> Self {
        Self { f, args }
    }

    pub(crate) fn call(self) {
        unsafe { (self.f)(self.args) };
    }
}

#[repr(C)]
#[derive(Debug, Clone)]
struct Timer {
    ets_timer: *mut ets_timer,
    started: u64,
    timeout: u64,
    active: bool,
    periodic: bool,
    callback: TimerCallback,
    next: Option<Box<Timer>>,
}

struct TimerQueue {
    head: Option<Box<Timer>>,
}

impl TimerQueue {
    const fn new() -> Self {
        Self { head: None }
    }

    fn find(&mut self, ets_timer: *mut ets_timer) -> Option<&mut Box<Timer>> {
        let mut current = self.head.as_mut();
        while let Some(timer) = current {
            if core::ptr::eq(timer.ets_timer, ets_timer) {
                return Some(timer);
            }
            current = timer.next.as_mut();
        }
        None
    }

    unsafe fn find_next_due(&mut self, now: u64) -> Option<&mut Box<Timer>> {
        let mut current = self.head.as_mut();
        while let Some(timer) = current {
            if timer.active && crate::time::time_diff(timer.started, now) >= timer.timeout {
                return Some(timer);
            }
            current = timer.next.as_mut();
        }
        None
    }

    fn remove(&mut self, ets_timer: *mut ets_timer) {
        if let Some(head) = self.head.as_mut()
            && core::ptr::eq(head.ets_timer, ets_timer)
        {
            self.head = head.next.take();
            return;
        }

        if let Some(target) = self.find(ets_timer) {
            let tail = target.next.take();
            let mut current = self.head.as_mut();
            let mut before = None;
            while let Some(node) = current {
                if core::ptr::eq(node.next.as_mut().unwrap().ets_timer, ets_timer) {
                    before = Some(node);
                    break;
                }
                current = node.next.as_mut();
            }

            if let Some(before) = before {
                let to_remove = before.next.take().unwrap();
                let to_remove = Box::into_raw(to_remove);
                unsafe { crate::compat::malloc::free(to_remove.cast()) };
                before.next = tail;
            }
        }
    }

    fn push(&mut self, to_add: Box<Timer>) {
        if self.head.is_none() {
            self.head = Some(to_add);
            return;
        }

        let mut current = self.head.as_mut();
        while let Some(timer) = current {
            if timer.next.is_none() {
                timer.next = Some(to_add);
                break;
            }
            current = timer.next.as_mut();
        }
    }
}

unsafe impl Send for TimerQueue {}

static TIMERS: NonReentrantMutex<TimerQueue> = NonReentrantMutex::new(TimerQueue::new());
static LEGACY_CURRENT_EXEC_ACTIVE: AtomicBool = AtomicBool::new(false);
static LEGACY_CURRENT_EXEC_CALLBACK_PTR: AtomicUsize = AtomicUsize::new(0);
static LEGACY_CURRENT_EXEC_ARG_PTR: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn compat_enabled() -> bool {
    legacy_timer_compat_enabled()
}

pub(crate) fn reset_diag() {
    legacy_reset_diag();
}

pub(crate) fn diag() -> LegacyTimerDiag {
    legacy_diag()
}

pub(crate) fn process_due_timer() -> bool {
    if !legacy_timer_compat_enabled() {
        return false;
    }

    let now = crate::time::systimer_count();
    let ordinal = LEGACY_PROCESS_DUE_CALL_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    LEGACY_LAST_NOW_US.store(now as u32, Ordering::Relaxed);
    let due_ring_idx = (ordinal as usize) % LEGACY_TIMER_RING_CAP;
    let (pre_op_chan, pre_scan_word00, pre_scan_word114) = unsafe {
        let chm_ptr = read_u32(core::ptr::addr_of!(g_chm) as usize, 0) as usize;
        let scan_ptr = read_u32(core::ptr::addr_of!(g_scan) as usize, 0) as usize;
        (
            read_u8(chm_ptr, 0x04) as u32,
            read_u32(scan_ptr, 0x00),
            read_u32(scan_ptr, 0x114),
        )
    };
    LEGACY_RECENT_DUE_ORDINALS[due_ring_idx].store(ordinal, Ordering::Relaxed);
    LEGACY_RECENT_DUE_FOUND[due_ring_idx].store(0, Ordering::Relaxed);
    LEGACY_RECENT_DUE_EXECUTED[due_ring_idx].store(0, Ordering::Relaxed);
    LEGACY_RECENT_DUE_CALLBACK_PTRS[due_ring_idx].store(0, Ordering::Relaxed);
    LEGACY_RECENT_DUE_ARG_PTRS[due_ring_idx].store(0, Ordering::Relaxed);
    LEGACY_RECENT_DUE_OP_CHANS[due_ring_idx].store(pre_op_chan, Ordering::Relaxed);
    LEGACY_RECENT_DUE_SCAN_WORD00[due_ring_idx].store(pre_scan_word00, Ordering::Relaxed);
    LEGACY_RECENT_DUE_SCAN_WORD114[due_ring_idx].store(pre_scan_word114, Ordering::Relaxed);
    let to_run = TIMERS.with(|timers| {
        let due = unsafe { timers.find_next_due(now) }?;
        LEGACY_LAST_STARTED_US.store(due.started as u32, Ordering::Relaxed);
        LEGACY_LAST_TIMEOUT_US.store(crate::time::ticks_to_micros(due.timeout).min(u32::MAX as u64) as u32, Ordering::Relaxed);
        LEGACY_RECENT_DUE_FOUND[due_ring_idx].store(1, Ordering::Relaxed);
        LEGACY_RECENT_DUE_CALLBACK_PTRS[due_ring_idx].store(due.callback.f as usize, Ordering::Relaxed);
        LEGACY_RECENT_DUE_ARG_PTRS[due_ring_idx].store(due.callback.args as usize, Ordering::Relaxed);
        due.active = due.periodic;
        if due.periodic {
            due.started = now;
        }
        Some(due.callback)
    });

    if let Some(callback) = to_run {
        let ordinal = LEGACY_EXEC_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        let ring_idx = (ordinal as usize) % LEGACY_TIMER_RING_CAP;
        let (op_chan, scan_word00, scan_word114) = unsafe {
            let chm_ptr = read_u32(core::ptr::addr_of!(g_chm) as usize, 0) as usize;
            let scan_ptr = read_u32(core::ptr::addr_of!(g_scan) as usize, 0) as usize;
            (
                read_u8(chm_ptr, 0x04) as u32,
                read_u32(scan_ptr, 0x00),
                read_u32(scan_ptr, 0x114),
            )
        };
        LEGACY_RECENT_EXEC_ORDINALS[ring_idx].store(ordinal, Ordering::Relaxed);
        LEGACY_RECENT_EXEC_CALLBACK_PTRS[ring_idx].store(callback.f as usize, Ordering::Relaxed);
        LEGACY_RECENT_EXEC_ARG_PTRS[ring_idx].store(callback.args as usize, Ordering::Relaxed);
        LEGACY_RECENT_EXEC_OP_CHANS[ring_idx].store(op_chan, Ordering::Relaxed);
        LEGACY_RECENT_EXEC_SCAN_WORD00[ring_idx].store(scan_word00, Ordering::Relaxed);
        LEGACY_RECENT_EXEC_SCAN_WORD114[ring_idx].store(scan_word114, Ordering::Relaxed);
        let skip_pair_arg1 = legacy_skip_pair_arg1_exec_enabled()
            && callback.args as usize == 1
            && callback.f as usize == legacy_pair_retarget_target() as usize;
        let skip_recovery_arg0 = legacy_skip_recovery_arg0_exec_enabled()
            && callback.args as usize == 0
            && pre_scan_word00 == 0x0000_010f
            && pre_scan_word114 == 0x0000_0000;
        let skip_exec = skip_pair_arg1 || skip_recovery_arg0;
        LEGACY_RECENT_DUE_EXECUTED[due_ring_idx].store((!skip_exec) as u32, Ordering::Relaxed);
        if legacy_due_trace_enabled() && ordinal <= 40 {
            esp_println::println!(
                "upload_http: boot_scan_only_diag legacy_due_trace ordinal={} found=1 executed={} callback_ptr=0x{:x} arg_ptr=0x{:x} pre_op_chan=0x{:02x} pre_scan_word00=0x{:08x} pre_scan_word114=0x{:08x}",
                ordinal,
                (!skip_exec) as u8,
                callback.f as usize,
                callback.args as usize,
                pre_op_chan,
                pre_scan_word00,
                pre_scan_word114,
            );
        }
        if skip_exec {
            if legacy_due_trace_enabled() && ordinal <= 40 {
                esp_println::println!(
                    "upload_http: boot_scan_only_diag legacy_due_trace ordinal={} found=1 executed=0 callback_ptr=0x{:x} arg_ptr=0x{:x} pre_op_chan=0x{:02x} pre_scan_word00=0x{:08x} pre_scan_word114=0x{:08x} reason={}",
                    ordinal,
                    callback.f as usize,
                    callback.args as usize,
                    pre_op_chan,
                    pre_scan_word00,
                    pre_scan_word114,
                    if skip_pair_arg1 {
                        "pair_arg1"
                    } else {
                        "recovery_arg0"
                    },
                );
            }
            return false;
        }
        LEGACY_PROCESS_DUE_HIT_COUNT.fetch_add(1, Ordering::Relaxed);
        LEGACY_CURRENT_EXEC_CALLBACK_PTR.store(callback.f as usize, Ordering::Relaxed);
        LEGACY_CURRENT_EXEC_ARG_PTR.store(callback.args as usize, Ordering::Relaxed);
        LEGACY_CURRENT_EXEC_ACTIVE.store(true, Ordering::Relaxed);
        callback.call();
        LEGACY_CURRENT_EXEC_ACTIVE.store(false, Ordering::Relaxed);
        LEGACY_CURRENT_EXEC_CALLBACK_PTR.store(0, Ordering::Relaxed);
        LEGACY_CURRENT_EXEC_ARG_PTR.store(0, Ordering::Relaxed);
        true
    } else {
        if legacy_due_trace_enabled() && ordinal <= 40 {
            esp_println::println!(
                "upload_http: boot_scan_only_diag legacy_due_trace ordinal={} found=0 executed=0 callback_ptr=0x0 arg_ptr=0x0 pre_op_chan=0x{:02x} pre_scan_word00=0x{:08x} pre_scan_word114=0x{:08x}",
                ordinal,
                pre_op_chan,
                pre_scan_word00,
                pre_scan_word114,
            );
        }
        false
    }
}

pub(crate) fn next_due_delay_us() -> Option<u32> {
    if !legacy_timer_compat_enabled() {
        return None;
    }

    let now = crate::time::systimer_count();
    LEGACY_LAST_NOW_US.store(now as u32, Ordering::Relaxed);
    TIMERS.with(|timers| {
        let mut current = timers.head.as_ref();
        let mut best: Option<u64> = None;
        while let Some(timer) = current {
            if timer.active {
                let elapsed = crate::time::time_diff(timer.started, now);
                let remaining = crate::time::ticks_to_micros(timer.timeout.saturating_sub(elapsed));
                best = Some(best.map_or(remaining, |prev| prev.min(remaining)));
            }
            current = timer.next.as_ref();
        }
        let next_due_us = best.map(|delay| delay.min(u32::MAX as u64) as u32).unwrap_or(u32::MAX);
        LEGACY_LAST_NEXT_DUE_US.store(next_due_us, Ordering::Relaxed);
        if let Some(timer) = timers.head.as_ref() {
            LEGACY_LAST_STARTED_US.store(timer.started as u32, Ordering::Relaxed);
            LEGACY_LAST_TIMEOUT_US.store(crate::time::ticks_to_micros(timer.timeout).min(u32::MAX as u64) as u32, Ordering::Relaxed);
        }
        (next_due_us != u32::MAX).then_some(next_due_us)
    })
}

pub(crate) fn compat_timer_arm(ets_timer: *mut ets_timer, tmout_ms: u32, repeat: bool) {
    compat_timer_arm_us(ets_timer, tmout_ms.saturating_mul(1000), repeat);
}

pub(crate) fn compat_timer_arm_us(ets_timer: *mut ets_timer, us: u32, repeat: bool) {
    if !legacy_runtime_policy::backend_legacy_port_enabled() {
        unsafe { __esp_rtos_diag_precreate_esp_radio_timer_task() };
        unsafe { __esp_rtos_diag_resume_esp_radio_timer_task() };
    }
    TIMERS.with(|timers| {
        if let Some(timer) = timers.find(ets_timer) {
            if should_suppress_callback_arm(timer.callback.f as usize, timer.callback.args as usize) {
                if matches!(
                    option_env!("MEDITAMER_WIFI_NEW_TRACE_DIAG"),
                    Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
                ) {
                    esp_println::println!(
                        "esp_radio: legacy_timer_compat suppress_arm callback=0x{:x} arg=0x{:x} us={}",
                        timer.callback.f as usize,
                        timer.callback.args as usize,
                        us
                    );
                }
                timer.active = false;
                timer.periodic = false;
                return;
            }
            timer.started = crate::time::systimer_count();
            timer.timeout = crate::time::micros_to_ticks(us as u64);
            timer.active = true;
            timer.periodic = repeat;
            LEGACY_ARM_COUNT.fetch_add(1, Ordering::Relaxed);
            LEGACY_LAST_CALLBACK_PTR.store(timer.callback.f as usize, Ordering::Relaxed);
            LEGACY_LAST_ARG_PTR.store(timer.callback.args as usize, Ordering::Relaxed);
            LEGACY_LAST_ARM_US.store(us, Ordering::Relaxed);
            LEGACY_LAST_ARM_REPEAT.store(repeat, Ordering::Relaxed);
            LEGACY_LAST_STARTED_US.store(timer.started as u32, Ordering::Relaxed);
            LEGACY_LAST_TIMEOUT_US.store(crate::time::ticks_to_micros(timer.timeout).min(u32::MAX as u64) as u32, Ordering::Relaxed);
            if legacy_due_trace_enabled() && LEGACY_CURRENT_EXEC_ACTIVE.load(Ordering::Relaxed) {
                esp_println::println!(
                    "upload_http: boot_scan_only_diag legacy_callback_sideeffect kind=arm current_callback_ptr=0x{:x} current_arg_ptr=0x{:x} target_timer_ptr=0x{:x} target_callback_ptr=0x{:x} target_arg_ptr=0x{:x} us={} repeat={}",
                    LEGACY_CURRENT_EXEC_CALLBACK_PTR.load(Ordering::Relaxed),
                    LEGACY_CURRENT_EXEC_ARG_PTR.load(Ordering::Relaxed),
                    ets_timer as usize,
                    timer.callback.f as usize,
                    timer.callback.args as usize,
                    us,
                    repeat as u8,
                );
            }
        }
    });
}

pub(crate) fn compat_timer_disarm(ets_timer: *mut ets_timer) {
    TIMERS.with(|timers| {
        if let Some(timer) = timers.find(ets_timer) {
            timer.active = false;
            if legacy_due_trace_enabled() && LEGACY_CURRENT_EXEC_ACTIVE.load(Ordering::Relaxed) {
                esp_println::println!(
                    "upload_http: boot_scan_only_diag legacy_callback_sideeffect kind=disarm current_callback_ptr=0x{:x} current_arg_ptr=0x{:x} target_timer_ptr=0x{:x} target_callback_ptr=0x{:x} target_arg_ptr=0x{:x}",
                    LEGACY_CURRENT_EXEC_CALLBACK_PTR.load(Ordering::Relaxed),
                    LEGACY_CURRENT_EXEC_ARG_PTR.load(Ordering::Relaxed),
                    ets_timer as usize,
                    timer.callback.f as usize,
                    timer.callback.args as usize,
                );
            }
        }
    });
}

pub(crate) fn compat_timer_is_active(ets_timer: *mut ets_timer) -> bool {
    TIMERS.with(|timers| timers.find(ets_timer).is_some_and(|timer| timer.active))
}

pub(crate) fn compat_timer_done(ets_timer: *mut ets_timer) {
    TIMERS.with(|timers| {
        if timers.find(ets_timer).is_some() {
            unsafe {
                (*ets_timer).priv_ = core::ptr::null_mut();
                (*ets_timer).expire = 0;
            }
            timers.remove(ets_timer);
        }
    });
}

pub(crate) fn compat_timer_setfn(
    ets_timer: *mut ets_timer,
    pfunction: unsafe extern "C" fn(*mut c_types::c_void),
    parg: *mut c_types::c_void,
) {
    let retargeted = maybe_retarget_nan_slot_callback(pfunction, parg);
    if legacy_retarget_trace_enabled() && (parg as usize) <= 1 {
        let source_ptr = nan_dp_schedule_ndc_start as usize + NAN_DP_SCHEDULE_NDC_START_OFFSET;
        let target_ptr = legacy_pair_retarget_target() as usize;
        esp_println::println!(
            "esp_radio: legacy_timer_retarget ets_timer=0x{:x} arg=0x{:x} incoming=0x{:x} source=0x{:x} target=0x{:x} effective=0x{:x}",
            ets_timer as usize,
            parg as usize,
            pfunction as usize,
            source_ptr,
            target_ptr,
            retargeted as usize
        );
    }
    let ordinal = LEGACY_SETFN_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
    let caller_ptr = current_setfn_caller_ptr();
    let ring_idx = (ordinal as usize) % LEGACY_TIMER_RING_CAP;
    LEGACY_RECENT_SETFN_ORDINALS[ring_idx].store(ordinal, Ordering::Relaxed);
    LEGACY_RECENT_SETFN_ETS_TIMER_PTRS[ring_idx].store(ets_timer as usize, Ordering::Relaxed);
    LEGACY_RECENT_SETFN_CALLBACK_PTRS[ring_idx].store(retargeted as usize, Ordering::Relaxed);
    LEGACY_RECENT_SETFN_ARG_PTRS[ring_idx].store(parg as usize, Ordering::Relaxed);
    LEGACY_RECENT_SETFN_CALLER_PTRS[ring_idx].store(caller_ptr, Ordering::Relaxed);
    LEGACY_LAST_CALLBACK_PTR.store(retargeted as usize, Ordering::Relaxed);
    LEGACY_LAST_ARG_PTR.store(parg as usize, Ordering::Relaxed);
    let callback = if should_suppress_callback_setfn(retargeted as usize, parg as usize) {
        suppressed_timer_callback
    } else {
        retargeted
    };
    let pair_target = legacy_pair_retarget_target();
    let set = TIMERS.with(|timers| unsafe {
        if let Some(timer) = timers.find(ets_timer) {
            timer.callback = TimerCallback::new(callback, parg);
            timer.active = false;
            (*ets_timer).expire = 0;
            if nan_timer_slot_retarget_enabled()
                && try_retarget_arg01_callback_pair(timers, callback as usize, pair_target)
            {
                esp_println::println!(
                    "esp_radio: legacy_timer_pair_retarget source=0x{:x} target=0x{:x}",
                    callback as usize,
                    pair_target as usize
                );
            }
            true
        } else {
            (*ets_timer).next = core::ptr::null_mut();
            (*ets_timer).period = 0;
            (*ets_timer).func = None;
            (*ets_timer).priv_ = core::ptr::null_mut();

            let timer = crate::compat::malloc::calloc(1, core::mem::size_of::<Timer>()) as *mut Timer;
            (*timer).next = None;
            (*timer).ets_timer = ets_timer;
            (*timer).started = 0;
            (*timer).timeout = 0;
            (*timer).active = false;
            (*timer).periodic = false;
            (*timer).callback = TimerCallback::new(callback, parg);

            timers.push(Box::from_raw(timer));
            if nan_timer_slot_retarget_enabled()
                && try_retarget_arg01_callback_pair(timers, callback as usize, pair_target)
            {
                esp_println::println!(
                    "esp_radio: legacy_timer_pair_retarget source=0x{:x} target=0x{:x}",
                    callback as usize,
                    pair_target as usize
                );
            }
            true
        }
    });

    if !set {
        warn!("Failed to set legacy timer function {:x}", ets_timer as usize);
    } else if legacy_due_trace_enabled() && LEGACY_CURRENT_EXEC_ACTIVE.load(Ordering::Relaxed) {
        esp_println::println!(
            "upload_http: boot_scan_only_diag legacy_callback_sideeffect kind=setfn current_callback_ptr=0x{:x} current_arg_ptr=0x{:x} target_timer_ptr=0x{:x} target_callback_ptr=0x{:x} target_arg_ptr=0x{:x}",
            LEGACY_CURRENT_EXEC_CALLBACK_PTR.load(Ordering::Relaxed),
            LEGACY_CURRENT_EXEC_ARG_PTR.load(Ordering::Relaxed),
            ets_timer as usize,
            callback as usize,
            parg as usize,
        );
    }
}

unsafe extern "C" fn suppressed_timer_callback(_arg: *mut c_types::c_void) {}
