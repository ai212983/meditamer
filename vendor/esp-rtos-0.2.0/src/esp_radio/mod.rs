//! esp-radio support

use core::{ffi::c_void, ptr::NonNull};

use allocator_api2::boxed::Box;
use esp_hal::{
    system::Cpu,
    time::{Duration, Instant},
};
use portable_atomic::{AtomicU32, Ordering};
use esp_radio_rtos_driver::{
    register_semaphore_implementation,
    semaphore::{SemaphoreImplementation, SemaphoreKind, SemaphorePtr},
};

use crate::{
    SCHEDULER,
    run_queue::MaxPriority,
    scheduler::Scheduler,
    semaphore::Semaphore,
    task::{self, Task},
};

mod queue;
mod bootstrap;
mod legacy_builtin_scheduler;
mod legacy_preempt;
mod legacy_scheduler;
pub(crate) mod sem_trace;
mod task_bootstrap;
mod timer_queue;

pub use bootstrap::{LegacyWifiBootstrapShimStatus, bootstrap_legacy_wifi_contract_shim};
pub use legacy_preempt::{LegacyPreemptCompatStatus, legacy_preempt_bootstrap_compat};
pub use sem_trace::SemTraceSnapshot;
pub(crate) use task_bootstrap::{maybe_handoff_to_wifi_task, note_task_selected, wifi_task_selected_count};
pub(crate) use timer_queue::{
    ensure_timer_task, reset_timer_callback_exec_diag, timer_callback_exec_diag,
    timer_task_default_branch_count, timer_task_entry_count,
    timer_task_legacy_compat_branch_count, timer_task_legacy_driver_branch_count,
    timer_task_loop_count, timer_task_mark_ready_count, timer_task_pop_count,
    timer_task_ptr, timer_task_resume_count, timer_task_selected_count,
    note_timer_task_mark_ready, note_timer_task_popped, note_timer_task_selected,
    wake_timer_task,
};
pub(crate) use queue::{
    queue_create_count, queue_create_last_capacity, queue_create_last_item_size,
};
pub(crate) use legacy_scheduler::pop_next_ready_override as pop_next_legacy_esp_radio_task;
pub(crate) use legacy_scheduler::ready_count as legacy_ready_task_count;
pub(crate) use legacy_scheduler::note_task_selected as note_legacy_task_selected;
pub(crate) use legacy_scheduler::runtime_mode_enabled as legacy_runtime_mode_enabled;
pub(crate) use legacy_builtin_scheduler::switch_task as legacy_builtin_scheduler_switch_task;
pub(crate) use legacy_builtin_scheduler::{
    LegacyBuiltinSchedulerSnapshot,
    reset_snapshot as reset_legacy_builtin_scheduler_snapshot,
    snapshot as legacy_builtin_scheduler_snapshot,
    task_ptr_at as legacy_builtin_scheduler_task_ptr_at,
    task_role_at as legacy_builtin_scheduler_task_role_at,
};
pub fn reset_sem_trace_diag() {
    sem_trace::reset();
}

pub fn sem_trace_diag() -> SemTraceSnapshot {
    sem_trace::snapshot()
}

static TASK_CREATE_COUNT: AtomicU32 = AtomicU32::new(0);
static TASK_CREATE_LAST_REQUESTED_PRIORITY: AtomicU32 = AtomicU32::new(0);
static TASK_CREATE_LAST_EFFECTIVE_PRIORITY: AtomicU32 = AtomicU32::new(0);

pub(crate) fn backend_legacy_port_runtime_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn legacy_builtin_scheduler_diag_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RTOS_USE_LEGACY_BUILTIN_SCHEDULER_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("ESP_RTOS_USE_LEGACY_BUILTIN_SCHEDULER_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || backend_legacy_port_runtime_enabled()
}

pub(crate) fn legacy_builtin_scheduler_runtime_mode_enabled() -> bool {
    legacy_builtin_scheduler_diag_enabled()
}

pub(crate) fn legacy_preempt_builtin_timer_diag_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RTOS_USE_LEGACY_PREEMPT_BUILTIN_TIMER_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("ESP_RTOS_USE_LEGACY_PREEMPT_BUILTIN_TIMER_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || backend_legacy_port_runtime_enabled()
}

fn create_trace_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RTOS_CREATE_TRACE_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn force_wifi_task_max_prio_diag_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RTOS_FORCE_WIFI_TASK_MAX_PRIO_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn legacy_wifi_task_priority_model_diag_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RTOS_USE_LEGACY_WIFI_TASK_PRIORITY_MODEL_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("ESP_RTOS_USE_LEGACY_WIFI_TASK_PRIORITY_MODEL_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || backend_legacy_port_runtime_enabled()
}

impl esp_radio_rtos_driver::Scheduler for Scheduler {
    fn initialized(&self) -> bool {
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
        legacy_scheduler::yield_override();
        task::yield_task();
    }

    fn yield_task_from_isr(&self) {
        legacy_scheduler::yield_override();
        task::yield_task();
    }

    fn max_task_priority(&self) -> u32 {
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
        TASK_CREATE_COUNT.fetch_add(1, Ordering::Relaxed);
        TASK_CREATE_LAST_REQUESTED_PRIORITY.store(priority, Ordering::Relaxed);
        TASK_CREATE_LAST_EFFECTIVE_PRIORITY.store(effective_priority, Ordering::Relaxed);
        if legacy_builtin_scheduler_diag_enabled() {
            legacy_builtin_scheduler::allocate_main_task();
            let task_handle =
                legacy_builtin_scheduler::task_create(name, task, param, task_stack_size);
            if let Some(task_ptr) = NonNull::new(task_handle.cast::<Task>()) {
                legacy_scheduler::note_created_task(name, task_ptr);
            }
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
        if legacy_builtin_scheduler_diag_enabled() {
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
        if legacy_builtin_scheduler_diag_enabled() {
            legacy_builtin_scheduler::schedule_task_deletion(task_handle);
            return;
        }
        legacy_scheduler::note_deleted_task(NonNull::new(task_handle.cast::<Task>()));
        task::schedule_task_deletion(task_handle as *mut Task)
    }

    fn current_task_thread_semaphore(&self) -> SemaphorePtr {
        if legacy_builtin_scheduler_diag_enabled() {
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
        SCHEDULER.sleep_until(Instant::now() + Duration::from_micros(us as u64));
    }

    fn now(&self) -> u64 {
        // We're using a SingleShotTimer as the time driver, which lets us use the system timer's
        // timestamps.
        crate::now()
    }
}

impl Semaphore {
    unsafe fn from_ptr<'a>(ptr: SemaphorePtr) -> &'a Self {
        unsafe { ptr.cast::<Self>().as_ref() }
    }
}

impl SemaphoreImplementation for Semaphore {
    fn create(kind: SemaphoreKind) -> SemaphorePtr {
        let sem = Box::new(match kind {
            SemaphoreKind::Counting { max, initial } => Semaphore::new_counting(initial, max),
            SemaphoreKind::Mutex => Semaphore::new_mutex(false),
            SemaphoreKind::RecursiveMutex => Semaphore::new_mutex(true),
        });
        NonNull::from(Box::leak(sem)).cast()
    }

    unsafe fn delete(semaphore: SemaphorePtr) {
        let sem = unsafe { Box::from_raw(semaphore.cast::<Semaphore>().as_ptr()) };
        core::mem::drop(sem);
    }

    unsafe fn take(semaphore: SemaphorePtr, timeout_us: Option<u32>) -> bool {
        let semaphore = unsafe { Semaphore::from_ptr(semaphore) };
        semaphore.take(timeout_us)
    }

    unsafe fn give(semaphore: SemaphorePtr) -> bool {
        let semaphore = unsafe { Semaphore::from_ptr(semaphore) };
        semaphore.give()
    }

    unsafe fn current_count(semaphore: SemaphorePtr) -> u32 {
        let semaphore = unsafe { Semaphore::from_ptr(semaphore) };

        semaphore.current_count()
    }

    unsafe fn try_take(semaphore: SemaphorePtr) -> bool {
        let semaphore = unsafe { Semaphore::from_ptr(semaphore) };

        semaphore.try_take()
    }

    unsafe fn try_give_from_isr(semaphore: SemaphorePtr, hptw: Option<&mut bool>) -> bool {
        let semaphore = unsafe { Semaphore::from_ptr(semaphore) };

        let ok = semaphore.try_give_from_isr();
        if ok {
            if let Some(flag) = hptw {
                *flag = true;
            }
        }
        ok
    }

    unsafe fn try_take_from_isr(semaphore: SemaphorePtr, hptw: Option<&mut bool>) -> bool {
        let semaphore = unsafe { Semaphore::from_ptr(semaphore) };

        let ok = semaphore.try_take_from_isr();
        if ok {
            if let Some(flag) = hptw {
                *flag = true;
            }
        }
        ok
    }
}

register_semaphore_implementation!(Semaphore);

pub(crate) fn task_create_count() -> u32 {
    TASK_CREATE_COUNT.load(Ordering::Relaxed)
}

pub(crate) fn task_create_last_requested_priority() -> u32 {
    TASK_CREATE_LAST_REQUESTED_PRIORITY.load(Ordering::Relaxed)
}

pub(crate) fn task_create_last_effective_priority() -> u32 {
    TASK_CREATE_LAST_EFFECTIVE_PRIORITY.load(Ordering::Relaxed)
}

pub(crate) fn legacy_task_model_entry_count() -> usize {
    legacy_scheduler::entry_count()
}

pub(crate) fn legacy_task_model_current_index() -> usize {
    legacy_scheduler::current_index()
}

pub(crate) fn reset_legacy_task_model() {
    legacy_scheduler::reset()
}

pub(crate) fn legacy_task_model_task_ptr_at(index: usize) -> usize {
    legacy_scheduler::task_ptr_at(index)
}

pub(crate) fn legacy_task_model_task_state_at(index: usize) -> usize {
    legacy_scheduler::task_state_at(index)
}

pub(crate) fn legacy_task_model_last_pop_candidate_ptr() -> usize {
    legacy_scheduler::last_pop_candidate_ptr()
}

pub(crate) fn legacy_task_model_last_pop_candidate_state() -> usize {
    legacy_scheduler::last_pop_candidate_state()
}

pub(crate) fn legacy_task_model_last_pop_selected_ptr() -> usize {
    legacy_scheduler::last_pop_selected_ptr()
}
