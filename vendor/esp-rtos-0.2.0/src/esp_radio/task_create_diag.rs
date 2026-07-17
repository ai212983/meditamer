use portable_atomic::{AtomicU32, Ordering};

static TASK_CREATE_COUNT: AtomicU32 = AtomicU32::new(0);
static TASK_CREATE_LAST_REQUESTED_PRIORITY: AtomicU32 = AtomicU32::new(0);
static TASK_CREATE_LAST_EFFECTIVE_PRIORITY: AtomicU32 = AtomicU32::new(0);

pub(crate) fn note_task_create(requested_priority: u32, effective_priority: u32) {
    TASK_CREATE_COUNT.fetch_add(1, Ordering::Relaxed);
    TASK_CREATE_LAST_REQUESTED_PRIORITY.store(requested_priority, Ordering::Relaxed);
    TASK_CREATE_LAST_EFFECTIVE_PRIORITY.store(effective_priority, Ordering::Relaxed);
}

pub(crate) fn task_create_count() -> u32 {
    TASK_CREATE_COUNT.load(Ordering::Relaxed)
}

pub(crate) fn task_create_last_requested_priority() -> u32 {
    TASK_CREATE_LAST_REQUESTED_PRIORITY.load(Ordering::Relaxed)
}

pub(crate) fn task_create_last_effective_priority() -> u32 {
    TASK_CREATE_LAST_EFFECTIVE_PRIORITY.load(Ordering::Relaxed)
}
