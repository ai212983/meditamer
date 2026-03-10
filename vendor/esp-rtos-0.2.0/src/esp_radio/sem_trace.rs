use portable_atomic::{AtomicU32, AtomicUsize, Ordering};

#[derive(Clone, Copy, Debug, Default)]
pub struct SemTraceSnapshot {
    pub take_wait_count: u32,
    pub take_done_count: u32,
    pub give_count: u32,
    pub wait_queue_sleep_count: u32,
    pub wait_queue_notify_count: u32,
    pub last_event: u32,
    pub last_task_ptr: usize,
    pub last_object_ptr: usize,
    pub last_value: u32,
}

const EVENT_TAKE_WAIT: u32 = 1;
const EVENT_TAKE_DONE: u32 = 2;
const EVENT_GIVE: u32 = 3;
const EVENT_WAIT_QUEUE_SLEEP: u32 = 4;
const EVENT_WAIT_QUEUE_NOTIFY: u32 = 5;

static TAKE_WAIT_COUNT: AtomicU32 = AtomicU32::new(0);
static TAKE_DONE_COUNT: AtomicU32 = AtomicU32::new(0);
static GIVE_COUNT: AtomicU32 = AtomicU32::new(0);
static WAIT_QUEUE_SLEEP_COUNT: AtomicU32 = AtomicU32::new(0);
static WAIT_QUEUE_NOTIFY_COUNT: AtomicU32 = AtomicU32::new(0);
static LAST_EVENT: AtomicU32 = AtomicU32::new(0);
static LAST_TASK_PTR: AtomicUsize = AtomicUsize::new(0);
static LAST_OBJECT_PTR: AtomicUsize = AtomicUsize::new(0);
static LAST_VALUE: AtomicU32 = AtomicU32::new(0);

fn record(event: u32, object_ptr: usize, value: u32) {
    // Keep sem trace passive: scheduler-owned paths like WaitQueue::notify()
    // cannot safely re-enter Scheduler::with() just to resolve current_task.
    LAST_TASK_PTR.store(0, Ordering::Relaxed);
    LAST_OBJECT_PTR.store(object_ptr, Ordering::Relaxed);
    LAST_VALUE.store(value, Ordering::Relaxed);
    LAST_EVENT.store(event, Ordering::Relaxed);
}

pub(crate) fn reset() {
    TAKE_WAIT_COUNT.store(0, Ordering::Relaxed);
    TAKE_DONE_COUNT.store(0, Ordering::Relaxed);
    GIVE_COUNT.store(0, Ordering::Relaxed);
    WAIT_QUEUE_SLEEP_COUNT.store(0, Ordering::Relaxed);
    WAIT_QUEUE_NOTIFY_COUNT.store(0, Ordering::Relaxed);
    LAST_EVENT.store(0, Ordering::Relaxed);
    LAST_TASK_PTR.store(0, Ordering::Relaxed);
    LAST_OBJECT_PTR.store(0, Ordering::Relaxed);
    LAST_VALUE.store(0, Ordering::Relaxed);
}

pub(crate) fn snapshot() -> SemTraceSnapshot {
    SemTraceSnapshot {
        take_wait_count: TAKE_WAIT_COUNT.load(Ordering::Relaxed),
        take_done_count: TAKE_DONE_COUNT.load(Ordering::Relaxed),
        give_count: GIVE_COUNT.load(Ordering::Relaxed),
        wait_queue_sleep_count: WAIT_QUEUE_SLEEP_COUNT.load(Ordering::Relaxed),
        wait_queue_notify_count: WAIT_QUEUE_NOTIFY_COUNT.load(Ordering::Relaxed),
        last_event: LAST_EVENT.load(Ordering::Relaxed),
        last_task_ptr: LAST_TASK_PTR.load(Ordering::Relaxed),
        last_object_ptr: LAST_OBJECT_PTR.load(Ordering::Relaxed),
        last_value: LAST_VALUE.load(Ordering::Relaxed),
    }
}

pub(crate) fn trace_take_wait(sem_ptr: usize, timeout_us: Option<u32>) {
    TAKE_WAIT_COUNT.fetch_add(1, Ordering::Relaxed);
    record(EVENT_TAKE_WAIT, sem_ptr, timeout_us.unwrap_or(u32::MAX));
}

pub(crate) fn trace_take_done(sem_ptr: usize, ok: bool) {
    TAKE_DONE_COUNT.fetch_add(1, Ordering::Relaxed);
    record(EVENT_TAKE_DONE, sem_ptr, ok as u32);
}

pub(crate) fn trace_give(sem_ptr: usize, ok: bool) {
    GIVE_COUNT.fetch_add(1, Ordering::Relaxed);
    record(EVENT_GIVE, sem_ptr, ok as u32);
}

pub(crate) fn trace_wait_queue_sleep(queue_ptr: usize) {
    WAIT_QUEUE_SLEEP_COUNT.fetch_add(1, Ordering::Relaxed);
    record(EVENT_WAIT_QUEUE_SLEEP, queue_ptr, 0);
}

pub(crate) fn trace_wait_queue_notify(queue_ptr: usize, resumed: usize) {
    WAIT_QUEUE_NOTIFY_COUNT.fetch_add(1, Ordering::Relaxed);
    record(EVENT_WAIT_QUEUE_NOTIFY, queue_ptr, resumed as u32);
}
