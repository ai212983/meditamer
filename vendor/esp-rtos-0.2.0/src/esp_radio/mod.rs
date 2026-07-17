//! esp-radio support

use core::{ffi::c_void, ptr::NonNull};

use esp_hal::{
    system::Cpu,
    time::{Duration, Instant},
};
use esp_radio_rtos_driver::semaphore::SemaphorePtr;

use crate::{
    SCHEDULER,
    run_queue::MaxPriority,
    scheduler::Scheduler,
    semaphore::Semaphore,
    task::{self, Task},
};

mod queue;
mod bootstrap;
mod legacy_preempt_builtin;
mod legacy_preempt_driver;
mod legacy_tasks;
mod legacy_builtin_scheduler;
mod legacy_preempt;
mod legacy_scheduler;
mod policy;
mod semaphore_impl;
pub(crate) mod sem_trace;
mod task_bootstrap;
mod task_create_diag;
mod timer_queue;

pub use bootstrap::{LegacyWifiBootstrapShimStatus, bootstrap_legacy_wifi_contract_shim};
pub use legacy_tasks::{LegacyWifiTasksInitStatus, init_legacy_wifi_tasks};
pub use legacy_preempt::{LegacyPreemptCompatStatus, legacy_preempt_bootstrap_compat};
pub use sem_trace::SemTraceSnapshot;
pub(crate) use task_bootstrap::{maybe_handoff_to_wifi_task, note_task_selected, wifi_task_selected_count};
pub(crate) use timer_queue::{
    ensure_timer_task, reset_timer_callback_exec_diag, timer_callback_exec_diag,
    timer_arm_count, timer_arm_recent_arg_ptr, timer_arm_recent_callback_ptr,
    timer_arm_recent_caller_ptr, timer_arm_recent_ordinal, timer_arm_recent_periodic,
    timer_arm_recent_timeout_us, timer_arm_recent_timer_ptr,
    timer_live_callback_arg_ptr, timer_live_callback_ptr, timer_live_is_active,
    timer_live_next_due_us, timer_live_period_us, timer_live_periodic, timer_live_started_us,
    timer_task_create_count, timer_task_create_from_enqueue_count,
    timer_task_create_from_ensure_count, timer_task_create_from_wake_count,
    timer_task_create_last_mode, timer_task_create_last_ptr, timer_task_create_last_source,
    timer_task_process_last_skip_arg_ptr, timer_task_process_last_skip_callback_ptr,
    timer_task_process_last_skip_due_us, timer_task_process_last_skip_now_us,
    timer_task_process_skip_inactive_count, timer_task_process_skip_not_due_count,
    timer_task_default_branch_count, timer_task_entry_count,
    timer_task_legacy_compat_branch_count, timer_task_legacy_driver_branch_count,
    timer_task_loop_count, timer_task_mark_ready_count, timer_task_pop_count,
    timer_task_ptr, timer_task_resume_count, timer_task_selected_count,
    timer_task_sleep_count, timer_task_sleep_false_count, timer_task_sleep_last_result,
    timer_task_sleep_last_task_ptr, timer_task_sleep_last_wake_at_us,
    timer_task_sleep_task_mismatch_count, timer_task_sleep_true_count,
    note_timer_task_mark_ready, note_timer_task_popped, note_timer_task_selected,
    wake_timer_task,
};
pub(crate) use queue::{
    queue_create_count, queue_create_last_capacity, queue_create_last_item_size,
};
pub(crate) use policy::{
    backend_legacy_port_runtime_enabled, force_wifi_task_max_prio_diag_enabled,
    legacy_builtin_scheduler_runtime_mode_enabled, legacy_preempt_builtin_timer_diag_enabled,
    legacy_wifi_task_priority_model_diag_enabled,
};
pub(crate) use task_create_diag::{
    note_task_create, task_create_count, task_create_last_effective_priority,
    task_create_last_requested_priority,
};
pub(crate) use legacy_scheduler::pop_next_ready_override as pop_next_legacy_esp_radio_task;
pub(crate) use legacy_scheduler::ready_count as legacy_ready_task_count;
pub(crate) use legacy_scheduler::note_task_selected as note_legacy_task_selected;
pub(crate) use legacy_scheduler::runtime_mode_enabled as legacy_runtime_mode_enabled;
pub(crate) use legacy_builtin_scheduler::switch_task as legacy_builtin_scheduler_switch_task;
pub(crate) use legacy_preempt_builtin::task_switch as legacy_preempt_builtin_switch_task;
pub(crate) use legacy_preempt_builtin::initialized as legacy_preempt_builtin_initialized;
pub(crate) use legacy_builtin_scheduler::{
    LegacyBuiltinSchedulerSnapshot,
    reset_snapshot as reset_legacy_builtin_scheduler_snapshot,
    snapshot as legacy_builtin_scheduler_snapshot,
    task_ptr_at as legacy_builtin_scheduler_task_ptr_at,
    task_role_ptr_for_task_ptr as legacy_builtin_scheduler_task_role_ptr_for_task_ptr,
    task_role_at as legacy_builtin_scheduler_task_role_at,
};
pub fn reset_sem_trace_diag() {
    sem_trace::reset();
}

pub fn sem_trace_diag() -> SemTraceSnapshot {
    sem_trace::snapshot()
}

pub(crate) fn legacy_preempt_builtin_enable() {
    legacy_preempt_builtin::enable();
    if backend_legacy_port_runtime_enabled() {
        legacy_builtin_scheduler::allocate_main_task();
    }
}

pub(crate) fn legacy_preempt_builtin_setup_timer(timer: crate::TimeBase) {
    legacy_preempt_builtin::setup_timer(timer);
}

pub(crate) fn legacy_preempt_builtin_yield_task() {
    if backend_legacy_port_runtime_enabled() {
        task::yield_task();
        return;
    }
    legacy_preempt_builtin::yield_task();
}

pub(crate) fn legacy_preempt_builtin_current_task() -> *mut c_void {
    if backend_legacy_port_runtime_enabled() {
        legacy_builtin_scheduler::allocate_main_task();
        return legacy_builtin_scheduler::current_task()
            .map(|task| task.as_ptr().cast())
            .unwrap_or(core::ptr::null_mut());
    }
    legacy_preempt_builtin::current_task()
}

pub(crate) fn legacy_preempt_builtin_current_task_thread_semaphore() -> *mut c_void {
    if backend_legacy_port_runtime_enabled() {
        legacy_builtin_scheduler::allocate_main_task();
        return legacy_builtin_scheduler::current_task_thread_semaphore_ptr();
    }
    legacy_preempt_builtin::current_task_thread_semaphore()
}

pub(crate) fn legacy_preempt_builtin_task_create(
    task: extern "C" fn(*mut c_void),
    param: *mut c_void,
    task_stack_size: usize,
) -> *mut c_void {
    if backend_legacy_port_runtime_enabled() {
        legacy_builtin_scheduler::allocate_main_task();
        return legacy_builtin_scheduler::task_create("legacy", task, param, task_stack_size);
    }
    legacy_preempt_builtin::task_create(task, param, task_stack_size)
}

pub(crate) fn legacy_preempt_builtin_schedule_task_deletion(task: *mut c_void) {
    if backend_legacy_port_runtime_enabled() {
        legacy_builtin_scheduler::schedule_task_deletion(task);
        return;
    }
    legacy_preempt_builtin::schedule_task_deletion(task)
}

pub(crate) fn legacy_preempt_builtin_max_task_priority() -> u32 {
    if backend_legacy_port_runtime_enabled() {
        return legacy_builtin_scheduler::max_task_priority();
    }
    legacy_preempt_builtin::max_task_priority()
}

impl esp_radio_rtos_driver::Scheduler for Scheduler {
    fn initialized(&self) -> bool {
        if backend_legacy_port_runtime_enabled() {
            return legacy_preempt_driver::initialized();
        }
        self.with(|scheduler| {
            if scheduler.time_driver.is_none() {
                warn!("Trying to initialize esp-radio before starting esp-rtos");
                return false;
            }

            let current_cpu = Cpu::current() as usize;
            if !scheduler.per_cpu[current_cpu].initialized {
                warn!(
                    "Trying to initialize esp-radio on {:?} but esp-rtos is not running on this core",
                    current_cpu
                );
                return false;
            }

            true
        })
    }

    fn yield_task(&self) {
        if backend_legacy_port_runtime_enabled() {
            legacy_preempt_driver::yield_task();
            return;
        }
        legacy_scheduler::yield_override();
        task::yield_task();
    }

    fn yield_task_from_isr(&self) {
        if backend_legacy_port_runtime_enabled() {
            legacy_preempt_driver::yield_task_from_isr();
            return;
        }
        legacy_scheduler::yield_override();
        task::yield_task();
    }

    fn max_task_priority(&self) -> u32 {
        if backend_legacy_port_runtime_enabled() {
            return legacy_preempt_driver::max_task_priority();
        }
        MaxPriority::MAX_PRIORITY as u32
    }

    fn task_create(
        &self,
        name: &str,
        task: extern "C" fn(*mut c_void),
        param: *mut c_void,
        priority: u32,
        pin_to_core: Option<u32>,
        task_stack_size: usize,
    ) -> *mut c_void {
        legacy_scheduler::initialize_main_if_enabled();
        let effective_priority = if name == "wifi" && legacy_wifi_task_priority_model_diag_enabled()
        {
            0
        } else if force_wifi_task_max_prio_diag_enabled() && name == "wifi" {
            self.max_task_priority()
        } else {
            priority.min(self.max_task_priority())
        };
        note_task_create(priority, effective_priority);
        if backend_legacy_port_runtime_enabled() {
            return legacy_preempt_driver::task_create(task, param, task_stack_size);
        }
        if legacy_builtin_scheduler_runtime_mode_enabled() {
            legacy_builtin_scheduler::allocate_main_task();
            let task_handle =
                legacy_builtin_scheduler::task_create(name, task, param, task_stack_size);
            maybe_handoff_to_wifi_task(name);
            return task_handle;
        }
        let task_ptr = self.create_task(
            name,
            task,
            param,
            task_stack_size,
            effective_priority,
            pin_to_core.and_then(|core| match core {
                0 => Some(Cpu::ProCpu),
                #[cfg(multi_core)]
                1 => Some(Cpu::AppCpu),
                _ => {
                    warn!("Invalid core number: {}", core);
                    None
                }
            }),
        );
        legacy_scheduler::note_created_task(name, task_ptr);
        maybe_handoff_to_wifi_task(name);
        task_ptr.as_ptr().cast()
    }

    fn current_task(&self) -> *mut c_void {
        if backend_legacy_port_runtime_enabled() {
            return legacy_preempt_driver::current_task();
        }
        if legacy_builtin_scheduler_runtime_mode_enabled() {
            legacy_builtin_scheduler::allocate_main_task();
            return legacy_builtin_scheduler::current_task()
                .map(|task| task.as_ptr().cast())
                .unwrap_or(core::ptr::null_mut());
        }
        legacy_scheduler::current_task_override()
            .unwrap_or_else(|| self.current_task())
            .as_ptr()
            .cast()
    }

    fn schedule_task_deletion(&self, task_handle: *mut c_void) {
        if backend_legacy_port_runtime_enabled() {
            legacy_preempt_driver::schedule_task_deletion(task_handle);
            return;
        }
        if legacy_builtin_scheduler_runtime_mode_enabled() {
            legacy_builtin_scheduler::schedule_task_deletion(task_handle);
            return;
        }
        legacy_scheduler::note_deleted_task(NonNull::new(task_handle.cast::<Task>()));
        task::schedule_task_deletion(task_handle as *mut Task)
    }

    fn current_task_thread_semaphore(&self) -> SemaphorePtr {
        if backend_legacy_port_runtime_enabled() {
            return legacy_preempt_driver::current_task_thread_semaphore();
        }
        if legacy_builtin_scheduler_runtime_mode_enabled() {
            legacy_builtin_scheduler::allocate_main_task();
            return NonNull::new(legacy_builtin_scheduler::current_task_thread_semaphore_ptr())
                .unwrap()
                .cast();
        }
        let task_ptr =
            legacy_scheduler::current_task_override().unwrap_or_else(|| self.current_task());
        unsafe {
            let task = task_ptr.as_ptr();
            NonNull::from(
                (*task)
                    .thread_semaphore
                    .get_or_insert_with(|| Semaphore::new_counting(0, 1)),
            )
            .cast()
        }
    }

    fn usleep(&self, us: u32) {
        if backend_legacy_port_runtime_enabled() {
            legacy_preempt_driver::usleep(us);
            return;
        }
        SCHEDULER.sleep_until(Instant::now() + Duration::from_micros(us as u64));
    }

    fn now(&self) -> u64 {
        if backend_legacy_port_runtime_enabled() {
            return legacy_preempt_driver::now_us();
        }
        // We're using a SingleShotTimer as the time driver, which lets us use the system timer's
        // timestamps.
        crate::now()
    }
}

pub(crate) fn legacy_task_model_entry_count() -> usize {
    if legacy_builtin_scheduler_runtime_mode_enabled() {
        return 0;
    }
    legacy_scheduler::entry_count()
}

pub(crate) fn legacy_task_model_current_index() -> usize {
    if legacy_builtin_scheduler_runtime_mode_enabled() {
        return 0;
    }
    legacy_scheduler::current_index()
}

pub(crate) fn reset_legacy_task_model() {
    if legacy_builtin_scheduler_runtime_mode_enabled() {
        return;
    }
    legacy_scheduler::reset()
}

pub(crate) fn legacy_task_model_task_ptr_at(index: usize) -> usize {
    if legacy_builtin_scheduler_runtime_mode_enabled() {
        return 0;
    }
    legacy_scheduler::task_ptr_at(index)
}

pub(crate) fn legacy_task_model_task_state_at(index: usize) -> usize {
    if legacy_builtin_scheduler_runtime_mode_enabled() {
        return 0;
    }
    legacy_scheduler::task_state_at(index)
}

pub(crate) fn legacy_task_model_last_pop_candidate_ptr() -> usize {
    if legacy_builtin_scheduler_runtime_mode_enabled() {
        return 0;
    }
    legacy_scheduler::last_pop_candidate_ptr()
}

pub(crate) fn legacy_task_model_last_pop_candidate_state() -> usize {
    if legacy_builtin_scheduler_runtime_mode_enabled() {
        return 0;
    }
    legacy_scheduler::last_pop_candidate_state()
}

pub(crate) fn legacy_task_model_last_pop_selected_ptr() -> usize {
    if legacy_builtin_scheduler_runtime_mode_enabled() {
        return 0;
    }
    legacy_scheduler::last_pop_selected_ptr()
}
