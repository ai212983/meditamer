use core::ffi::c_void;

mod mutex;
mod queue;
mod semaphore;
mod task;
mod event_group;

pub(crate) use queue::ConcurrentQueue;

pub(crate) fn thread_sem_get() -> *mut c_void {
    crate::compat::preempt_legacy_backend::current_task_thread_semaphore()
        .as_ptr()
        .cast::<c_void>()
}

pub(crate) use mutex::{mutex_create, mutex_delete, mutex_lock, mutex_unlock};
pub(crate) use queue::{
    queue_create, queue_delete, queue_msg_waiting, queue_recv, queue_recv_from_isr,
    queue_send_from_isr, queue_send_to_back, queue_send_to_front,
};
pub(crate) use semaphore::{
    semphr_create, semphr_delete, semphr_give, semphr_give_from_isr, semphr_take,
    semphr_take_from_isr,
};
pub(crate) use event_group::{
    event_group_clear_bits, event_group_create, event_group_delete, event_group_set_bits,
    event_group_wait_bits,
};
pub(crate) use task::{
    esp_timer_get_time, task_create, task_current, task_delete, task_delay, task_max_priority,
    task_ms_to_tick, task_yield_from_isr,
};
