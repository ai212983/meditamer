use core::{ffi::c_void, ptr::NonNull};
use portable_atomic::{AtomicU32, AtomicUsize, Ordering};

unsafe extern "C" {
    fn __esp_rtos_legacy_preempt_builtin_enable();
    fn __esp_rtos_legacy_preempt_builtin_yield_task();
    fn __esp_rtos_legacy_preempt_builtin_current_task() -> *mut c_void;
    fn __esp_rtos_legacy_preempt_builtin_current_task_thread_semaphore() -> *mut c_void;
    fn __esp_rtos_legacy_preempt_builtin_task_create(
        task: extern "C" fn(*mut c_void),
        param: *mut c_void,
        task_stack_size: usize,
    ) -> *mut c_void;
    fn __esp_rtos_legacy_preempt_builtin_schedule_task_deletion(task: *mut c_void);
    fn __esp_rtos_legacy_preempt_builtin_max_task_priority() -> u32;
}

#[derive(Clone, Copy)]
pub(crate) struct PreemptLegacyBackendDiag {
    pub enable_count: u32,
    pub yield_count: u32,
    pub current_task_count: u32,
    pub current_task_last_ptr: usize,
    pub current_task_thread_sem_count: u32,
    pub current_task_thread_sem_last_ptr: usize,
    pub task_create_count: u32,
    pub task_create_last_task_ptr: usize,
    pub task_create_last_stack_size: usize,
    pub schedule_delete_count: u32,
}

static PREEMPT_LEGACY_ENABLE_COUNT: AtomicU32 = AtomicU32::new(0);
static PREEMPT_LEGACY_YIELD_COUNT: AtomicU32 = AtomicU32::new(0);
static PREEMPT_LEGACY_CURRENT_TASK_COUNT: AtomicU32 = AtomicU32::new(0);
static PREEMPT_LEGACY_CURRENT_TASK_LAST_PTR: AtomicUsize = AtomicUsize::new(0);
static PREEMPT_LEGACY_CURRENT_TASK_THREAD_SEM_COUNT: AtomicU32 = AtomicU32::new(0);
static PREEMPT_LEGACY_CURRENT_TASK_THREAD_SEM_LAST_PTR: AtomicUsize = AtomicUsize::new(0);
static PREEMPT_LEGACY_TASK_CREATE_COUNT: AtomicU32 = AtomicU32::new(0);
static PREEMPT_LEGACY_TASK_CREATE_LAST_PTR: AtomicUsize = AtomicUsize::new(0);
static PREEMPT_LEGACY_TASK_CREATE_LAST_STACK_SIZE: AtomicUsize = AtomicUsize::new(0);
static PREEMPT_LEGACY_SCHEDULE_DELETE_COUNT: AtomicU32 = AtomicU32::new(0);

pub(crate) fn enable() {
    PREEMPT_LEGACY_ENABLE_COUNT.fetch_add(1, Ordering::Relaxed);
    unsafe { __esp_rtos_legacy_preempt_builtin_enable() };
}

pub(crate) fn yield_task() {
    PREEMPT_LEGACY_YIELD_COUNT.fetch_add(1, Ordering::Relaxed);
    unsafe { __esp_rtos_legacy_preempt_builtin_yield_task() };
}

pub(crate) fn current_task() -> *mut c_void {
    let ptr = unsafe { __esp_rtos_legacy_preempt_builtin_current_task() };
    PREEMPT_LEGACY_CURRENT_TASK_COUNT.fetch_add(1, Ordering::Relaxed);
    PREEMPT_LEGACY_CURRENT_TASK_LAST_PTR.store(ptr as usize, Ordering::Relaxed);
    ptr
}

pub(crate) fn current_task_thread_semaphore() -> NonNull<c_void> {
    let ptr = unsafe { __esp_rtos_legacy_preempt_builtin_current_task_thread_semaphore() };
    PREEMPT_LEGACY_CURRENT_TASK_THREAD_SEM_COUNT.fetch_add(1, Ordering::Relaxed);
    PREEMPT_LEGACY_CURRENT_TASK_THREAD_SEM_LAST_PTR.store(ptr as usize, Ordering::Relaxed);
    NonNull::new(ptr)
        .expect("legacy current_task_thread_semaphore returned null")
}

pub(crate) fn task_create(
    task: extern "C" fn(*mut c_void),
    param: *mut c_void,
    task_stack_size: usize,
) -> *mut c_void {
    let ptr = unsafe { __esp_rtos_legacy_preempt_builtin_task_create(task, param, task_stack_size) };
    PREEMPT_LEGACY_TASK_CREATE_COUNT.fetch_add(1, Ordering::Relaxed);
    PREEMPT_LEGACY_TASK_CREATE_LAST_PTR.store(ptr as usize, Ordering::Relaxed);
    PREEMPT_LEGACY_TASK_CREATE_LAST_STACK_SIZE.store(task_stack_size, Ordering::Relaxed);
    ptr
}

pub(crate) fn schedule_task_deletion(task: *mut c_void) {
    PREEMPT_LEGACY_SCHEDULE_DELETE_COUNT.fetch_add(1, Ordering::Relaxed);
    unsafe { __esp_rtos_legacy_preempt_builtin_schedule_task_deletion(task) };
}

pub(crate) fn max_task_priority() -> u32 {
    unsafe { __esp_rtos_legacy_preempt_builtin_max_task_priority() }
}

pub(crate) fn preempt_legacy_backend_diag() -> PreemptLegacyBackendDiag {
    PreemptLegacyBackendDiag {
        enable_count: PREEMPT_LEGACY_ENABLE_COUNT.load(Ordering::Relaxed),
        yield_count: PREEMPT_LEGACY_YIELD_COUNT.load(Ordering::Relaxed),
        current_task_count: PREEMPT_LEGACY_CURRENT_TASK_COUNT.load(Ordering::Relaxed),
        current_task_last_ptr: PREEMPT_LEGACY_CURRENT_TASK_LAST_PTR.load(Ordering::Relaxed),
        current_task_thread_sem_count: PREEMPT_LEGACY_CURRENT_TASK_THREAD_SEM_COUNT
            .load(Ordering::Relaxed),
        current_task_thread_sem_last_ptr: PREEMPT_LEGACY_CURRENT_TASK_THREAD_SEM_LAST_PTR
            .load(Ordering::Relaxed),
        task_create_count: PREEMPT_LEGACY_TASK_CREATE_COUNT.load(Ordering::Relaxed),
        task_create_last_task_ptr: PREEMPT_LEGACY_TASK_CREATE_LAST_PTR.load(Ordering::Relaxed),
        task_create_last_stack_size: PREEMPT_LEGACY_TASK_CREATE_LAST_STACK_SIZE
            .load(Ordering::Relaxed),
        schedule_delete_count: PREEMPT_LEGACY_SCHEDULE_DELETE_COUNT.load(Ordering::Relaxed),
    }
}
