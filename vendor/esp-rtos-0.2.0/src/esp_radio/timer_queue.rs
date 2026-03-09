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
use portable_atomic::{AtomicU32, AtomicUsize, Ordering};
use esp_sync::NonReentrantMutex;

use crate::{
    SCHEDULER,
    esp_radio::legacy_scheduler,
    task::{TaskExt, TaskPtr},
};

static TIMER_QUEUE: TimerQueue = TimerQueue::new();
static TIMER_CALLBACK_EXEC_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_TASK_ENTRY_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_CALLBACK_CURRENT_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_CALLBACK_CURRENT_ARG_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_CALLBACK_LAST_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_CALLBACK_LAST_ARG_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_CALLBACK_LAST_EXEC_AT_US: AtomicU32 = AtomicU32::new(0);
static TIMER_CALLBACK_LAST_DUE_AT_US: AtomicU32 = AtomicU32::new(0);
static TIMER_CALLBACK_LAST_TIMEOUT_US: AtomicU32 = AtomicU32::new(0);
static TIMER_CALLBACK_LAST_LATENESS_US: AtomicU32 = AtomicU32::new(0);
static TIMER_CALLBACK_MAX_LATENESS_US: AtomicU32 = AtomicU32::new(0);

fn use_legacy_timer_loop_diag_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RTOS_USE_LEGACY_TIMER_LOOP_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("ESP_RTOS_USE_LEGACY_TIMER_LOOP_DIAG"),
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
}

pub fn reset_timer_callback_exec_diag() {
    TIMER_CALLBACK_EXEC_COUNT.store(0, Ordering::Relaxed);
    TIMER_TASK_ENTRY_COUNT.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_CURRENT_PTR.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_CURRENT_ARG_PTR.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_LAST_PTR.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_LAST_ARG_PTR.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_LAST_EXEC_AT_US.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_LAST_DUE_AT_US.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_LAST_TIMEOUT_US.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_LAST_LATENESS_US.store(0, Ordering::Relaxed);
    TIMER_CALLBACK_MAX_LATENESS_US.store(0, Ordering::Relaxed);
}

pub fn timer_task_entry_count() -> u32 {
    TIMER_TASK_ENTRY_COUNT.load(Ordering::Relaxed)
}

pub fn timer_callback_exec_diag() -> TimerCallbackExecDiag {
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
    }
}

pub fn ensure_timer_task() {
    TIMER_QUEUE.inner.with(|q| {
        if q.task.is_none() {
            let task_ptr =
                SCHEDULER.create_task("timer", timer_task, core::ptr::null_mut(), 8192, 2, None);
            legacy_scheduler::note_created_task("timer", task_ptr);
            q.task = Some(task_ptr);
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

struct TimerQueueInner {
    // A linked list of active timers
    head: Option<NonNull<Timer>>,
    next_wakeup: u64,
    task: Option<TaskPtr>,
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
                task.resume();
            }
        } else {
            // create the timer task
            let task_ptr =
                SCHEDULER.create_task("timer", timer_task, core::ptr::null_mut(), 8192, 2, None);
            legacy_scheduler::note_created_task("timer", task_ptr);
            self.task = Some(task_ptr);
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
                    debug!(
                        "Timer {:x} is inactive or dropped",
                        current_timer as *const _ as usize
                    );
                    return None;
                }

                if props.next_due > crate::now() {
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
                TIMER_CALLBACK_EXEC_COUNT.fetch_add(1, Ordering::Relaxed);
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
                SCHEDULER.sleep_until(Instant::EPOCH + Duration::from_micros(next_wakeup));
            }
        });
    }
}

struct TimerProperties {
    is_active: bool,
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
        let next_due = crate::now() + timeout;

        let props = self.properties(q);
        props.is_active = true;
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
        TIMER_QUEUE.process();
    }
}
