use alloc::boxed::Box;

use esp_sync::NonReentrantMutex;
use portable_atomic::Ordering;

use crate::binary::{
    c_types,
    include::ets_timer,
};
use crate::compat::timer_compat_legacy_diag::{
    diag as legacy_diag, reset_diag as legacy_reset_diag, LegacyTimerDiag, LEGACY_ARM_COUNT,
    LEGACY_EXEC_COUNT, LEGACY_LAST_ARG_PTR, LEGACY_LAST_ARM_REPEAT, LEGACY_LAST_ARM_US,
    LEGACY_LAST_CALLBACK_PTR, LEGACY_LAST_NEXT_DUE_US, LEGACY_LAST_NOW_US,
    LEGACY_LAST_STARTED_US, LEGACY_LAST_TIMEOUT_US, LEGACY_PROCESS_DUE_CALL_COUNT,
    LEGACY_PROCESS_DUE_HIT_COUNT, LEGACY_SETFN_COUNT,
};
use crate::compat::timer_compat_legacy_policy::{
    compat_enabled as legacy_timer_compat_enabled, should_suppress_callback_arm,
    should_suppress_callback_setfn,
};

unsafe extern "C" {
    fn __esp_rtos_diag_precreate_esp_radio_timer_task();
    fn __esp_rtos_diag_resume_esp_radio_timer_task();
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
            if timer.active && now.wrapping_sub(timer.started) >= timer.timeout {
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

    let now = crate::preempt::now();
    LEGACY_PROCESS_DUE_CALL_COUNT.fetch_add(1, Ordering::Relaxed);
    LEGACY_LAST_NOW_US.store(now as u32, Ordering::Relaxed);
    let to_run = TIMERS.with(|timers| {
        let due = unsafe { timers.find_next_due(now) }?;
        LEGACY_LAST_STARTED_US.store(due.started as u32, Ordering::Relaxed);
        LEGACY_LAST_TIMEOUT_US.store(due.timeout as u32, Ordering::Relaxed);
        due.active = due.periodic;
        if due.periodic {
            due.started = now;
        }
        Some(due.callback)
    });

    if let Some(callback) = to_run {
        LEGACY_EXEC_COUNT.fetch_add(1, Ordering::Relaxed);
        LEGACY_PROCESS_DUE_HIT_COUNT.fetch_add(1, Ordering::Relaxed);
        callback.call();
        true
    } else {
        false
    }
}

pub(crate) fn next_due_delay_us() -> Option<u32> {
    if !legacy_timer_compat_enabled() {
        return None;
    }

    let now = crate::preempt::now();
    LEGACY_LAST_NOW_US.store(now as u32, Ordering::Relaxed);
    TIMERS.with(|timers| {
        let mut current = timers.head.as_ref();
        let mut best: Option<u64> = None;
        while let Some(timer) = current {
            if timer.active {
                let elapsed = now.wrapping_sub(timer.started);
                let remaining = timer.timeout.saturating_sub(elapsed);
                best = Some(best.map_or(remaining, |prev| prev.min(remaining)));
            }
            current = timer.next.as_ref();
        }
        let next_due_us = best.map(|delay| delay.min(u32::MAX as u64) as u32).unwrap_or(u32::MAX);
        LEGACY_LAST_NEXT_DUE_US.store(next_due_us, Ordering::Relaxed);
        if let Some(timer) = timers.head.as_ref() {
            LEGACY_LAST_STARTED_US.store(timer.started as u32, Ordering::Relaxed);
            LEGACY_LAST_TIMEOUT_US.store(timer.timeout as u32, Ordering::Relaxed);
        }
        (next_due_us != u32::MAX).then_some(next_due_us)
    })
}

pub(crate) fn compat_timer_arm(ets_timer: *mut ets_timer, tmout_ms: u32, repeat: bool) {
    compat_timer_arm_us(ets_timer, tmout_ms.saturating_mul(1000), repeat);
}

pub(crate) fn compat_timer_arm_us(ets_timer: *mut ets_timer, us: u32, repeat: bool) {
    unsafe { __esp_rtos_diag_precreate_esp_radio_timer_task() };
    unsafe { __esp_rtos_diag_resume_esp_radio_timer_task() };
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
            timer.started = crate::preempt::now();
            timer.timeout = us as u64;
            timer.active = true;
            timer.periodic = repeat;
            LEGACY_ARM_COUNT.fetch_add(1, Ordering::Relaxed);
            LEGACY_LAST_CALLBACK_PTR.store(timer.callback.f as usize, Ordering::Relaxed);
            LEGACY_LAST_ARG_PTR.store(timer.callback.args as usize, Ordering::Relaxed);
            LEGACY_LAST_ARM_US.store(us, Ordering::Relaxed);
            LEGACY_LAST_ARM_REPEAT.store(repeat, Ordering::Relaxed);
            LEGACY_LAST_STARTED_US.store(timer.started as u32, Ordering::Relaxed);
            LEGACY_LAST_TIMEOUT_US.store(timer.timeout as u32, Ordering::Relaxed);
        }
    });
}

pub(crate) fn compat_timer_disarm(ets_timer: *mut ets_timer) {
    TIMERS.with(|timers| {
        if let Some(timer) = timers.find(ets_timer) {
            timer.active = false;
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
    LEGACY_SETFN_COUNT.fetch_add(1, Ordering::Relaxed);
    LEGACY_LAST_CALLBACK_PTR.store(pfunction as usize, Ordering::Relaxed);
    LEGACY_LAST_ARG_PTR.store(parg as usize, Ordering::Relaxed);
    let callback = if should_suppress_callback_setfn(pfunction as usize, parg as usize) {
        suppressed_timer_callback
    } else {
        pfunction
    };
    let set = TIMERS.with(|timers| unsafe {
        if let Some(timer) = timers.find(ets_timer) {
            timer.callback = TimerCallback::new(callback, parg);
            timer.active = false;
            (*ets_timer).expire = 0;
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
            true
        }
    });

    if !set {
        warn!("Failed to set legacy timer function {:x}", ets_timer as usize);
    }
}

unsafe extern "C" fn suppressed_timer_callback(_arg: *mut c_types::c_void) {}
