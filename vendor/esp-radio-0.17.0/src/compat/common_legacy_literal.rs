#![allow(unused)]

use core::{ffi::c_void, mem::size_of};

use esp_wifi_sys::c_types::{c_char, c_int};
use portable_atomic::{AtomicU32, AtomicUsize, Ordering};

use crate::{
    compat::{
        common::str_from_c,
        common_legacy_queue::{self, ConcurrentQueue},
        malloc::{free, malloc},
        preempt_legacy_backend as legacy_preempt,
        OSI_FUNCS_TIME_BLOCKING,
    },
    memory_fence::memory_fence,
    time,
};

#[repr(C)]
struct Mutex {
    locking_pid: usize,
    count: u32,
    recursive: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct CommonLegacyLiteralDiag {
    pub task_create_count: u32,
    pub task_create_last_task_ptr: usize,
    pub task_create_last_stack_depth: u32,
    pub thread_sem_get_count: u32,
    pub thread_sem_get_last_ptr: usize,
    pub queue_create_count: u32,
    pub queue_create_last_len: i32,
    pub queue_create_last_item_size: i32,
}

static COMMON_LEGACY_TASK_CREATE_COUNT: AtomicU32 = AtomicU32::new(0);
static COMMON_LEGACY_TASK_CREATE_LAST_TASK_PTR: AtomicUsize = AtomicUsize::new(0);
static COMMON_LEGACY_TASK_CREATE_LAST_STACK_DEPTH: AtomicU32 = AtomicU32::new(0);
static COMMON_LEGACY_THREAD_SEM_GET_COUNT: AtomicU32 = AtomicU32::new(0);
static COMMON_LEGACY_THREAD_SEM_GET_LAST_PTR: AtomicUsize = AtomicUsize::new(0);
static COMMON_LEGACY_QUEUE_CREATE_COUNT: AtomicU32 = AtomicU32::new(0);
static COMMON_LEGACY_QUEUE_CREATE_LAST_LEN: AtomicU32 = AtomicU32::new(0);
static COMMON_LEGACY_QUEUE_CREATE_LAST_ITEM_SIZE: AtomicU32 = AtomicU32::new(0);

fn mutex_ptr(mutex: *mut c_void) -> *mut Mutex {
    mutex.cast()
}

pub(crate) fn sem_create(_max: u32, init: u32) -> *mut c_void {
    unsafe {
        let ptr = malloc(size_of::<u32>()) as *mut u32;
        ptr.write_volatile(init);
        ptr.cast()
    }
}

pub(crate) fn sem_delete(semphr: *mut c_void) {
    unsafe { free(semphr.cast()) };
}

pub(crate) fn sem_take(semphr: *mut c_void, tick: u32) -> i32 {
    let tick = if tick == OSI_FUNCS_TIME_BLOCKING && crate::is_interrupts_disabled() {
        1
    } else {
        tick
    };

    let forever = tick == OSI_FUNCS_TIME_BLOCKING;
    let timeout = tick as u64;
    let start = time::systimer_count();
    let sem = semphr.cast::<u32>();

    loop {
        let res = critical_section::with(|_| unsafe {
            memory_fence();
            let cnt = *sem;
            if cnt > 0 {
                *sem = cnt - 1;
                1
            } else {
                0
            }
        });

        if res == 1 {
            return 1;
        }
        if !forever && time::elapsed_time_since(start) > timeout {
            return 0;
        }
        legacy_preempt::yield_task();
    }
}

pub(crate) fn sem_take_from_isr(semphr: *mut c_void, higher_priority_task_waken: *mut bool) -> i32 {
    let sem = semphr.cast::<u32>();
    critical_section::with(|_| unsafe {
        let cnt = *sem;
        if cnt > 0 {
            *sem = cnt - 1;
            if let Some(waken) = higher_priority_task_waken.as_mut() {
                *waken = true;
            }
            1
        } else {
            0
        }
    })
}

pub(crate) fn sem_give(semphr: *mut c_void) -> i32 {
    let sem = semphr.cast::<u32>();
    critical_section::with(|_| unsafe {
        let cnt = *sem;
        *sem = cnt + 1;
        1
    })
}

pub(crate) fn sem_give_from_isr(semphr: *mut c_void, higher_priority_task_waken: *mut bool) -> i32 {
    let sem = semphr.cast::<u32>();
    critical_section::with(|_| unsafe {
        let cnt = *sem;
        *sem = cnt + 1;
        if let Some(waken) = higher_priority_task_waken.as_mut() {
            *waken = true;
        }
        1
    })
}

pub(crate) fn thread_sem_get() -> *mut c_void {
    let ptr = legacy_preempt::current_task_thread_semaphore()
        .as_ptr()
        .cast::<c_void>();
    COMMON_LEGACY_THREAD_SEM_GET_COUNT.fetch_add(1, Ordering::Relaxed);
    COMMON_LEGACY_THREAD_SEM_GET_LAST_PTR.store(ptr as usize, Ordering::Relaxed);
    ptr
}

pub(crate) fn create_recursive_mutex() -> *mut c_void {
    let mutex = Mutex {
        locking_pid: usize::MAX,
        count: 0,
        recursive: true,
    };
    unsafe {
        let ptr = malloc(size_of::<Mutex>()) as *mut Mutex;
        ptr.write(mutex);
        ptr.cast()
    }
}

pub(crate) fn create_mutex() -> *mut c_void {
    let mutex = Mutex {
        locking_pid: usize::MAX,
        count: 0,
        recursive: false,
    };
    unsafe {
        let ptr = malloc(size_of::<Mutex>()) as *mut Mutex;
        ptr.write(mutex);
        ptr.cast()
    }
}

pub(crate) fn mutex_delete(mutex: *mut c_void) {
    unsafe { free(mutex.cast()) };
}

pub(crate) fn lock_mutex(mutex: *mut c_void) -> i32 {
    let ptr = mutex_ptr(mutex);
    let current_task = legacy_preempt::current_task() as usize;

    loop {
        let locked = critical_section::with(|_| unsafe {
            if (*ptr).count == 0 {
                (*ptr).locking_pid = current_task;
                (*ptr).count = 1;
                true
            } else if (*ptr).recursive && (*ptr).locking_pid == current_task {
                (*ptr).count = (*ptr).count.saturating_add(1);
                true
            } else {
                false
            }
        });

        if locked {
            return 1;
        }

        legacy_preempt::yield_task();
    }
}

pub(crate) fn unlock_mutex(mutex: *mut c_void) -> i32 {
    let ptr = mutex_ptr(mutex);
    critical_section::with(|_| unsafe {
        if (*ptr).count > 0 {
            (*ptr).count -= 1;
            if (*ptr).count == 0 {
                (*ptr).locking_pid = usize::MAX;
            }
            1
        } else {
            0
        }
    })
}

pub(crate) fn create_queue(queue_len: c_int, item_size: c_int) -> *mut ConcurrentQueue {
    COMMON_LEGACY_QUEUE_CREATE_COUNT.fetch_add(1, Ordering::Relaxed);
    COMMON_LEGACY_QUEUE_CREATE_LAST_LEN.store(queue_len as u32, Ordering::Relaxed);
    COMMON_LEGACY_QUEUE_CREATE_LAST_ITEM_SIZE.store(item_size as u32, Ordering::Relaxed);
    common_legacy_queue::create_queue(queue_len, item_size)
}

pub(crate) fn delete_queue(queue: *mut ConcurrentQueue) {
    common_legacy_queue::delete_queue(queue);
}

pub(crate) fn send_queued(queue: *mut ConcurrentQueue, item: *mut c_void, block_time_tick: u32) -> i32 {
    common_legacy_queue::send_queued(queue, item, block_time_tick)
}

pub(crate) fn send_queued_front(
    queue: *mut ConcurrentQueue,
    item: *mut c_void,
    block_time_tick: u32,
) -> i32 {
    common_legacy_queue::send_queued_front(queue, item, block_time_tick)
}

pub(crate) fn receive_queued(
    queue: *mut ConcurrentQueue,
    item: *mut c_void,
    block_time_tick: u32,
) -> i32 {
    common_legacy_queue::receive_queued(queue, item, block_time_tick)
}

pub(crate) fn number_of_messages_in_queue(queue: *const ConcurrentQueue) -> u32 {
    common_legacy_queue::number_of_messages_in_queue(queue)
}

pub(crate) fn try_send_queued_from_isr(
    queue: *mut ConcurrentQueue,
    item: *mut c_void,
    higher_priority_task_waken: *mut bool,
) -> i32 {
    common_legacy_queue::try_send_queued_from_isr(queue, item, higher_priority_task_waken)
}

pub(crate) fn try_receive_queued_from_isr(
    queue: *mut ConcurrentQueue,
    item: *mut c_void,
    higher_priority_task_waken: *mut bool,
) -> i32 {
    common_legacy_queue::try_receive_queued_from_isr(queue, item, higher_priority_task_waken)
}

pub(crate) fn task_create(
    task_func: *mut c_void,
    _name: *const c_char,
    stack_depth: u32,
    param: *mut c_void,
    task_handle: *mut c_void,
) -> i32 {
    let task = legacy_preempt::task_create(
        unsafe { core::mem::transmute(task_func) },
        param,
        stack_depth as usize,
    );
    COMMON_LEGACY_TASK_CREATE_COUNT.fetch_add(1, Ordering::Relaxed);
    COMMON_LEGACY_TASK_CREATE_LAST_TASK_PTR.store(task as usize, Ordering::Relaxed);
    COMMON_LEGACY_TASK_CREATE_LAST_STACK_DEPTH.store(stack_depth, Ordering::Relaxed);
    unsafe { *(task_handle as *mut usize) = task as usize };
    1
}

pub(crate) fn common_legacy_literal_diag() -> CommonLegacyLiteralDiag {
    CommonLegacyLiteralDiag {
        task_create_count: COMMON_LEGACY_TASK_CREATE_COUNT.load(Ordering::Relaxed),
        task_create_last_task_ptr: COMMON_LEGACY_TASK_CREATE_LAST_TASK_PTR.load(Ordering::Relaxed),
        task_create_last_stack_depth: COMMON_LEGACY_TASK_CREATE_LAST_STACK_DEPTH
            .load(Ordering::Relaxed),
        thread_sem_get_count: COMMON_LEGACY_THREAD_SEM_GET_COUNT.load(Ordering::Relaxed),
        thread_sem_get_last_ptr: COMMON_LEGACY_THREAD_SEM_GET_LAST_PTR.load(Ordering::Relaxed),
        queue_create_count: COMMON_LEGACY_QUEUE_CREATE_COUNT.load(Ordering::Relaxed),
        queue_create_last_len: COMMON_LEGACY_QUEUE_CREATE_LAST_LEN.load(Ordering::Relaxed) as i32,
        queue_create_last_item_size: COMMON_LEGACY_QUEUE_CREATE_LAST_ITEM_SIZE
            .load(Ordering::Relaxed) as i32,
    }
}

pub(crate) fn task_delete(task_handle: *mut c_void) {
    legacy_preempt::schedule_task_deletion(task_handle);
}

pub(crate) fn task_delay(tick: u32) {
    let forever = tick == OSI_FUNCS_TIME_BLOCKING;
    let timeout = tick as u64;
    let start = time::systimer_count();
    loop {
        if !forever && time::elapsed_time_since(start) > timeout {
            return;
        }
        legacy_preempt::yield_task();
        if !forever {
            return;
        }
    }
}

pub(crate) fn task_get_current_task() -> *mut c_void {
    legacy_preempt::current_task()
}

pub(crate) fn task_get_max_priority() -> i32 {
    legacy_preempt::max_task_priority() as i32
}

pub(crate) fn task_yield_from_isr() {
    legacy_preempt::yield_task();
}

pub unsafe extern "C" fn semphr_create(max: u32, init: u32) -> *mut c_void {
    sem_create(max, init)
}

pub unsafe extern "C" fn semphr_delete(semphr: *mut c_void) {
    sem_delete(semphr)
}

pub unsafe extern "C" fn semphr_take(semphr: *mut c_void, tick: u32) -> i32 {
    sem_take(semphr, tick)
}

pub unsafe extern "C" fn semphr_take_from_isr_c_void(
    semphr: *mut c_void,
    higher_priority_task_waken: *mut c_void,
) -> i32 {
    sem_take_from_isr(semphr, higher_priority_task_waken.cast())
}

pub unsafe extern "C" fn semphr_give(semphr: *mut c_void) -> i32 {
    sem_give(semphr)
}

pub unsafe extern "C" fn semphr_give_from_isr_c_void(
    semphr: *mut c_void,
    higher_priority_task_waken: *mut c_void,
) -> i32 {
    sem_give_from_isr(semphr, higher_priority_task_waken.cast())
}

pub unsafe extern "C" fn wifi_thread_semphr_get() -> *mut c_void {
    thread_sem_get()
}

pub unsafe extern "C" fn mutex_create() -> *mut c_void {
    create_mutex()
}

pub unsafe extern "C" fn recursive_mutex_create() -> *mut c_void {
    create_recursive_mutex()
}

pub unsafe extern "C" fn queue_create_c_void(queue_len: c_int, item_size: c_int) -> *mut c_void {
    create_queue(queue_len, item_size).cast()
}

pub unsafe extern "C" fn queue_delete_c_void(queue: *mut c_void) {
    delete_queue(queue.cast())
}

pub unsafe extern "C" fn queue_send(
    queue: *mut c_void,
    item: *mut c_void,
    block_time_tick: u32,
) -> i32 {
    send_queued(queue.cast(), item, block_time_tick)
}

pub unsafe extern "C" fn queue_send_from_isr(
    queue: *mut c_void,
    item: *mut c_void,
    higher_priority_task_waken: *mut c_void,
) -> i32 {
    try_send_queued_from_isr(queue.cast(), item, higher_priority_task_waken.cast())
}

pub unsafe extern "C" fn queue_send_to_back(
    queue: *mut c_void,
    item: *mut c_void,
    block_time_tick: u32,
) -> i32 {
    send_queued(queue.cast(), item, block_time_tick)
}

pub unsafe extern "C" fn queue_send_to_front(
    queue: *mut c_void,
    item: *mut c_void,
    block_time_tick: u32,
) -> i32 {
    send_queued_front(queue.cast(), item, block_time_tick)
}

pub unsafe extern "C" fn queue_recv(
    queue: *mut c_void,
    item: *mut c_void,
    block_time_tick: u32,
) -> i32 {
    receive_queued(queue.cast(), item, block_time_tick)
}

pub unsafe extern "C" fn queue_msg_waiting(queue: *mut c_void) -> u32 {
    number_of_messages_in_queue(queue.cast())
}

pub unsafe extern "C" fn task_create_pinned_to_core(
    task_func: *mut c_void,
    name: *const c_char,
    stack_depth: u32,
    param: *mut c_void,
    _prio: u32,
    task_handle: *mut c_void,
    _core_id: u32,
) -> i32 {
    task_create(task_func, name, stack_depth, param, task_handle)
}

pub unsafe extern "C" fn task_create_unpinned(
    task_func: *mut c_void,
    name: *const c_char,
    stack_depth: u32,
    param: *mut c_void,
    _prio: u32,
    task_handle: *mut c_void,
) -> i32 {
    task_create(task_func, name, stack_depth, param, task_handle)
}
