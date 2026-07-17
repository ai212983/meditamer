use esp_hal::{
    interrupt::{InterruptHandler, Priority},
    system::Cpu,
    time::{Duration, Instant, Rate},
};
use portable_atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};

#[cfg(feature = "embassy")]
use crate::TIMER_QUEUE;
#[cfg(feature = "rtos-trace")]
use crate::TraceEvents;
use crate::{
    SCHEDULER,
    TICK_RATE,
    TimeBase,
    run_queue::RunSchedulerOn,
    task::{self, TaskExt, TaskPtr, TaskQueue, TaskState, TaskTimerQueueElement},
};

#[cfg(feature = "embassy")]
pub(crate) mod embassy;

const TIMESLICE_DURATION: Duration = Rate::from_hz(TICK_RATE).as_duration();

static TIMER_WAKE_SCHEDULE_CALL_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_WAKE_SCHEDULE_ACCEPT_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_WAKE_SCHEDULE_PAST_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_WAKE_SCHEDULE_INFINITE_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_WAKE_SCHEDULE_LAST_TASK_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_WAKE_SCHEDULE_LAST_WAKE_AT_US: AtomicU64 = AtomicU64::new(0);
static TIMER_WAKE_TICK_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_WAKE_HANDLE_ALARM_CALL_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_WAKE_HANDLE_ALARM_SKIP_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_WAKE_HANDLE_ALARM_PROCESS_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_WAKE_READY_COUNT: AtomicU32 = AtomicU32::new(0);
static TIMER_WAKE_LAST_READY_TASK_PTR: AtomicUsize = AtomicUsize::new(0);
static TIMER_WAKE_LAST_NOW_US: AtomicU64 = AtomicU64::new(0);
static TIMER_WAKE_LAST_CURRENT_ALARM_US: AtomicU64 = AtomicU64::new(0);
static TIMER_WAKE_LAST_QUEUE_NEXT_WAKEUP_US: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
pub struct SchedulerTimerWakeDiag {
    pub schedule_call_count: u32,
    pub schedule_accept_count: u32,
    pub schedule_past_count: u32,
    pub schedule_infinite_count: u32,
    pub schedule_last_task_ptr: usize,
    pub schedule_last_wake_at_us: u64,
    pub tick_count: u32,
    pub handle_alarm_call_count: u32,
    pub handle_alarm_skip_count: u32,
    pub handle_alarm_process_count: u32,
    pub ready_count: u32,
    pub last_ready_task_ptr: usize,
    pub last_now_us: u64,
    pub last_current_alarm_us: u64,
    pub last_queue_next_wakeup_us: u64,
}

pub fn reset_scheduler_timer_wake_diag() {
    TIMER_WAKE_SCHEDULE_CALL_COUNT.store(0, Ordering::Relaxed);
    TIMER_WAKE_SCHEDULE_ACCEPT_COUNT.store(0, Ordering::Relaxed);
    TIMER_WAKE_SCHEDULE_PAST_COUNT.store(0, Ordering::Relaxed);
    TIMER_WAKE_SCHEDULE_INFINITE_COUNT.store(0, Ordering::Relaxed);
    TIMER_WAKE_SCHEDULE_LAST_TASK_PTR.store(0, Ordering::Relaxed);
    TIMER_WAKE_SCHEDULE_LAST_WAKE_AT_US.store(0, Ordering::Relaxed);
    TIMER_WAKE_TICK_COUNT.store(0, Ordering::Relaxed);
    TIMER_WAKE_HANDLE_ALARM_CALL_COUNT.store(0, Ordering::Relaxed);
    TIMER_WAKE_HANDLE_ALARM_SKIP_COUNT.store(0, Ordering::Relaxed);
    TIMER_WAKE_HANDLE_ALARM_PROCESS_COUNT.store(0, Ordering::Relaxed);
    TIMER_WAKE_READY_COUNT.store(0, Ordering::Relaxed);
    TIMER_WAKE_LAST_READY_TASK_PTR.store(0, Ordering::Relaxed);
    TIMER_WAKE_LAST_NOW_US.store(0, Ordering::Relaxed);
    TIMER_WAKE_LAST_CURRENT_ALARM_US.store(0, Ordering::Relaxed);
    TIMER_WAKE_LAST_QUEUE_NEXT_WAKEUP_US.store(0, Ordering::Relaxed);
}

pub fn scheduler_timer_wake_diag() -> SchedulerTimerWakeDiag {
    SchedulerTimerWakeDiag {
        schedule_call_count: TIMER_WAKE_SCHEDULE_CALL_COUNT.load(Ordering::Relaxed),
        schedule_accept_count: TIMER_WAKE_SCHEDULE_ACCEPT_COUNT.load(Ordering::Relaxed),
        schedule_past_count: TIMER_WAKE_SCHEDULE_PAST_COUNT.load(Ordering::Relaxed),
        schedule_infinite_count: TIMER_WAKE_SCHEDULE_INFINITE_COUNT.load(Ordering::Relaxed),
        schedule_last_task_ptr: TIMER_WAKE_SCHEDULE_LAST_TASK_PTR.load(Ordering::Relaxed),
        schedule_last_wake_at_us: TIMER_WAKE_SCHEDULE_LAST_WAKE_AT_US.load(Ordering::Relaxed),
        tick_count: TIMER_WAKE_TICK_COUNT.load(Ordering::Relaxed),
        handle_alarm_call_count: TIMER_WAKE_HANDLE_ALARM_CALL_COUNT.load(Ordering::Relaxed),
        handle_alarm_skip_count: TIMER_WAKE_HANDLE_ALARM_SKIP_COUNT.load(Ordering::Relaxed),
        handle_alarm_process_count: TIMER_WAKE_HANDLE_ALARM_PROCESS_COUNT.load(Ordering::Relaxed),
        ready_count: TIMER_WAKE_READY_COUNT.load(Ordering::Relaxed),
        last_ready_task_ptr: TIMER_WAKE_LAST_READY_TASK_PTR.load(Ordering::Relaxed),
        last_now_us: TIMER_WAKE_LAST_NOW_US.load(Ordering::Relaxed),
        last_current_alarm_us: TIMER_WAKE_LAST_CURRENT_ALARM_US.load(Ordering::Relaxed),
        last_queue_next_wakeup_us: TIMER_WAKE_LAST_QUEUE_NEXT_WAKEUP_US.load(Ordering::Relaxed),
    }
}

#[cfg(feature = "esp-radio")]
fn legacy_preempt_builtin_timer_diag_enabled() -> bool {
    crate::esp_radio::legacy_preempt_builtin_timer_diag_enabled()
}

#[cfg(not(feature = "esp-radio"))]
fn legacy_preempt_builtin_timer_diag_enabled() -> bool {
    false
}

pub(crate) struct TimerQueue {
    queue: TaskQueue<TaskTimerQueueElement>,
    next_wakeup: u64,
    time_slice_target: [u64; Cpu::COUNT],
}

impl Default for TimerQueue {
    fn default() -> Self {
        // Can't derive Default, the default implementation must start with no wakeup timestamp
        Self::new()
    }
}

impl TimerQueue {
    pub const fn new() -> Self {
        Self {
            queue: TaskQueue::new(),
            next_wakeup: u64::MAX,
            time_slice_target: [u64::MAX; Cpu::COUNT],
        }
    }

    fn retain(&mut self, now: u64, mut on_task_ready: impl FnMut(TaskPtr)) {
        if now < self.next_wakeup {
            trace!("Skipping timer queue");
            return;
        }

        let mut timer_queue = core::mem::take(self);
        self.time_slice_target = timer_queue.time_slice_target;

        while let Some(mut task_ptr) = timer_queue.pop() {
            let task = unsafe { task_ptr.as_mut() };

            let wakeup_at = task.wakeup_at;
            let ready = wakeup_at <= now;

            if ready {
                on_task_ready(task_ptr);
            } else {
                self.push(task_ptr, wakeup_at);
            }
        }
    }

    pub fn push(&mut self, task: TaskPtr, wakeup_at: u64) {
        self.queue.push(task);

        self.next_wakeup = self.next_wakeup.min(wakeup_at);
    }

    pub fn pop(&mut self) -> Option<TaskPtr> {
        // We can allow waking up sooner than necessary, so this function doesn't need to
        // re-calculate the next wakeup time.
        self.queue.pop()
    }

    pub fn remove(&mut self, task: TaskPtr) {
        // We can allow waking up sooner than necessary, so this function doesn't need to
        // re-calculate the next wakeup time.
        self.queue.remove(task);
    }

    fn next_wakeup(&self) -> u64 {
        let mut wakeup_at = self.next_wakeup;

        for time_slice_target in self.time_slice_target.iter().copied() {
            wakeup_at = wakeup_at.min(time_slice_target);
        }

        #[cfg(feature = "embassy")]
        let wakeup_at = wakeup_at.min(TIMER_QUEUE.next_wakeup());

        wakeup_at
    }
}

pub(crate) struct TimeDriver {
    timer: TimeBase,
    pub(crate) timer_queue: TimerQueue,
    current_alarm: u64,
}

impl TimeDriver {
    pub(crate) fn new(mut timer: TimeBase) -> Self {
        // The timer needs to tick at Priority 1 to prevent accidentally interrupting
        // priority limited locks.
        let timer_priority = Priority::Priority1;

        let cb: extern "C" fn() = unsafe { core::mem::transmute(timer_tick_handler as *const ()) };

        cfg_if::cfg_if! {
            if #[cfg(riscv)] {
                // Register the interrupt handler without nesting to satisfy the requirements of the
                // task switching code
                let handler = InterruptHandler::new_not_nested(cb, timer_priority);
            } else {
                let handler = InterruptHandler::new(cb, timer_priority);
            }
        };

        timer.set_interrupt_handler(handler);
        timer.listen();

        let mut driver = Self {
            timer,
            timer_queue: TimerQueue::new(),
            current_alarm: u64::MAX,
        };

        if legacy_preempt_builtin_timer_diag_enabled() {
            driver.arm_legacy_periodic_tick(crate::now());
        }

        driver
    }

    pub(crate) fn handle_alarm(&mut self, now: u64, on_task_ready: impl FnMut(TaskPtr)) {
        TIMER_WAKE_HANDLE_ALARM_CALL_COUNT.fetch_add(1, Ordering::Relaxed);
        TIMER_WAKE_LAST_NOW_US.store(now, Ordering::Relaxed);
        TIMER_WAKE_LAST_CURRENT_ALARM_US.store(self.current_alarm, Ordering::Relaxed);
        TIMER_WAKE_LAST_QUEUE_NEXT_WAKEUP_US.store(self.timer_queue.next_wakeup(), Ordering::Relaxed);
        if now < self.current_alarm {
            TIMER_WAKE_HANDLE_ALARM_SKIP_COUNT.fetch_add(1, Ordering::Relaxed);
            trace!(
                "Not processing RTOS timer queue. Now: {}, expected next wakeup: {}",
                now, self.current_alarm
            );
            return;
        }
        TIMER_WAKE_HANDLE_ALARM_PROCESS_COUNT.fetch_add(1, Ordering::Relaxed);
        trace!("Processing RTOS timer queue at {}", now);
        self.current_alarm = u64::MAX;
        self.timer_queue.retain(now, on_task_ready);
    }

    pub(crate) fn set_time_slice(&mut self, cpu: Cpu, now: u64, enable: bool) {
        self.timer_queue.time_slice_target[cpu as usize] = if enable {
            trace!("Enable time slicing");
            now + TIMESLICE_DURATION.as_micros()
        } else {
            trace!("Disable time slicing");
            u64::MAX
        };
    }

    pub(crate) fn arm_next_wakeup(&mut self, now: u64) {
        let next_wakeup = self.timer_queue.next_wakeup();

        // Only skip arming timer if the timestamp is the same. If the next wakeup changed to a
        // later timestamp, the tick handler may not trigger a scheduler run. This means that if we
        // did not arm here, the timer would not be re-armed.
        if next_wakeup == self.current_alarm {
            return;
        }

        self.current_alarm = next_wakeup;

        let sleep_duration = next_wakeup.saturating_sub(now);

        // assume 52-bit underlying timer. it's not a big deal to sleep for a shorter time
        let mut timeout = sleep_duration & ((1 << 52) - 1);

        trace!("Arming timer for {} (target = {})", timeout, next_wakeup);
        loop {
            match self.timer.schedule(Duration::from_micros(timeout)) {
                Ok(_) => break,
                Err(esp_hal::timer::Error::InvalidTimeout) if timeout != 0 => {
                    timeout /= 2;
                    continue;
                }
                Err(e) => panic!("Failed to schedule timer: {:?}", e),
            }
        }
    }

    pub(crate) fn arm_legacy_periodic_tick(&mut self, now: u64) {
        self.current_alarm = now + TIMESLICE_DURATION.as_micros();
        unwrap!(self.timer.schedule(TIMESLICE_DURATION));
    }

    pub(crate) fn schedule_wakeup(&mut self, mut current_task: TaskPtr, at: Instant) -> bool {
        TIMER_WAKE_SCHEDULE_CALL_COUNT.fetch_add(1, Ordering::Relaxed);
        TIMER_WAKE_SCHEDULE_LAST_TASK_PTR.store(current_task.as_ptr() as usize, Ordering::Relaxed);
        TIMER_WAKE_SCHEDULE_LAST_WAKE_AT_US
            .store(at.duration_since_epoch().as_micros(), Ordering::Relaxed);
        let state = current_task.state();
        #[cfg(feature = "esp-radio")]
        let allow_legacy_non_ready = crate::esp_radio::backend_legacy_port_runtime_enabled()
            || crate::esp_radio::legacy_runtime_mode_enabled()
            || crate::esp_radio::legacy_builtin_scheduler_runtime_mode_enabled();
        #[cfg(not(feature = "esp-radio"))]
        let allow_legacy_non_ready = false;
        if !allow_legacy_non_ready {
            debug_assert_eq!(state, TaskState::Ready, "task: {:?}", current_task);
        }

        // Target time is infinite, suspend task without waking up via timer.
        if at == Instant::EPOCH + Duration::MAX {
            TIMER_WAKE_SCHEDULE_INFINITE_COUNT.fetch_add(1, Ordering::Relaxed);
            current_task.set_state(TaskState::Sleeping);
            debug!("Suspending task: {:?}", current_task);
            return true;
        }

        // Target time is in the past, don't sleep.
        if at <= Instant::now() {
            TIMER_WAKE_SCHEDULE_PAST_COUNT.fetch_add(1, Ordering::Relaxed);
            debug!("Target time is in the past");
            return false;
        }

        TIMER_WAKE_SCHEDULE_ACCEPT_COUNT.fetch_add(1, Ordering::Relaxed);
        current_task.set_state(TaskState::Sleeping);

        let timestamp = at.duration_since_epoch().as_micros();
        debug!(
            "Scheduling wakeup for task {:?} at timestamp {}",
            current_task, timestamp
        );
        self.timer_queue.push(current_task, timestamp);

        unsafe { current_task.as_mut().wakeup_at = timestamp };

        true
    }
}

#[esp_hal::ram]
extern "C" fn timer_tick_handler() {
    TIMER_WAKE_TICK_COUNT.fetch_add(1, Ordering::Relaxed);
    #[cfg(feature = "rtos-trace")]
    rtos_trace::trace::marker_begin(TraceEvents::TimerTickHandler as u32);

    trace!("Timer tick");

    SCHEDULER.with_shared(|scheduler| {
        let now = crate::now();

        #[cfg(feature = "embassy")]
        {
            #[cfg(feature = "rtos-trace")]
            rtos_trace::trace::marker_begin(TraceEvents::ProcessEmbassyTimerQueue as u32);

            TIMER_QUEUE.handle_alarm(now);

            #[cfg(feature = "rtos-trace")]
            rtos_trace::trace::marker_end(TraceEvents::ProcessEmbassyTimerQueue as u32);
        }

        let mut scheduler = unwrap!(scheduler.try_borrow_mut());
        let scheduler = &mut *scheduler;

        let time_driver = unwrap!(scheduler.time_driver.as_mut());

        time_driver.timer.clear_interrupt();

        #[cfg(feature = "rtos-trace")]
        rtos_trace::trace::marker_begin(TraceEvents::ProcessTimerQueue as u32);

        // Process timer queue. This will wake up ready tasks, and set a new alarm.
        time_driver.handle_alarm(now, |ready_task| {
            TIMER_WAKE_READY_COUNT.fetch_add(1, Ordering::Relaxed);
            TIMER_WAKE_LAST_READY_TASK_PTR.store(ready_task.as_ptr() as usize, Ordering::Relaxed);
            debug_assert_eq!(
                ready_task.state(),
                crate::task::TaskState::Sleeping,
                "task: {:?}",
                ready_task
            );

            debug!("Task {:?} is ready", ready_task);

            match scheduler
                .run_queue
                .mark_task_ready(&scheduler.per_cpu, ready_task)
            {
                RunSchedulerOn::DontRun => {}
                RunSchedulerOn::CurrentCore => task::yield_task(),
                #[cfg(multi_core)]
                RunSchedulerOn::OtherCore => task::schedule_other_core(),
            }
        });

        #[cfg(feature = "rtos-trace")]
        rtos_trace::trace::marker_end(TraceEvents::ProcessTimerQueue as u32);

        if now >= time_driver.timer_queue.time_slice_target[0] {
            crate::task::yield_task();
        }

        #[cfg(multi_core)]
        if now >= time_driver.timer_queue.time_slice_target[1] {
            crate::task::schedule_other_core();
        }

        if legacy_preempt_builtin_timer_diag_enabled() {
            time_driver.arm_legacy_periodic_tick(now);
        } else {
            // Re-arm the timer. This should be relatively cheap, and ensures that the timer will keep
            // ticking even if the interrupt doesn't trigger a context switch.
            // FIXME: this SHOULD be relatively cheap, but arming the timer involves u64 division.
            time_driver.current_alarm = u64::MAX;
            time_driver.arm_next_wakeup(now);
        }
    });

    #[cfg(feature = "rtos-trace")]
    rtos_trace::trace::marker_end(TraceEvents::TimerTickHandler as u32);
}
