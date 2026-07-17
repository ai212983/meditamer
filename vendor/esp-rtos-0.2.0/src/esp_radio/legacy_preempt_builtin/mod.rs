#[cfg_attr(target_arch = "riscv32", path = "preempt_riscv.rs")]
#[cfg_attr(target_arch = "xtensa", path = "preempt_xtensa.rs")]
mod arch_specific;
mod locked;
pub(crate) mod timer;

use core::{ffi::c_void, mem::MaybeUninit, ptr::NonNull};

use allocator_api2::boxed::Box;

use crate::{task::CpuContext, InternalMemory};

use arch_specific::*;
use locked::Locked;
pub(crate) use timer::{disable_timebase, setup_multitasking};
pub(crate) use timer::setup_timer;
use timer::disable_multitasking;

struct Context {
    cpu_context: CpuContext,
    pub thread_semaphore: u32,
    pub next: *mut Context,
    pub _allocated_stack: Box<[MaybeUninit<u8>], InternalMemory>,
}

impl Context {
    fn new(
        task_fn: extern "C" fn(*mut c_void),
        param: *mut c_void,
        task_stack_size: usize,
    ) -> Self {
        let mut stack = Box::<[u8], _>::new_uninit_slice_in(task_stack_size, InternalMemory);
        let stack_top = unsafe { stack.as_mut_ptr().add(task_stack_size).cast() };

        Self {
            cpu_context: new_task_context(task_fn, param, stack_top),
            thread_semaphore: 0,
            next: core::ptr::null_mut(),
            _allocated_stack: stack,
        }
    }
}

struct SchedulerState {
    current_task: *mut Context,
    to_delete: *mut Context,
}

unsafe impl Send for SchedulerState {}

impl SchedulerState {
    const fn new() -> Self {
        Self {
            current_task: core::ptr::null_mut(),
            to_delete: core::ptr::null_mut(),
        }
    }

    fn delete_task(&mut self, task: *mut Context) {
        let mut current_task = self.current_task;
        let initial = current_task;

        loop {
            let next_task = unsafe { (*current_task).next };
            if core::ptr::eq(next_task, task) {
                unsafe {
                    (*current_task).next = (*next_task).next;
                    core::ptr::drop_in_place(task);
                }
                break;
            }

            if core::ptr::eq(next_task, initial) {
                break;
            }

            current_task = next_task;
        }
    }

    fn switch_task(&mut self, trap_frame: &mut CpuContext) {
        save_task_context(unsafe { &mut *self.current_task }, trap_frame);

        if !self.to_delete.is_null() {
            let task_to_delete = core::mem::take(&mut self.to_delete);
            self.delete_task(task_to_delete);
        }

        unsafe { self.current_task = (*self.current_task).next };
        restore_task_context(unsafe { &mut *self.current_task }, trap_frame);
    }

    fn schedule_task_deletion(&mut self, task: *mut Context) -> bool {
        self.to_delete = task;
        core::ptr::eq(task, self.current_task)
    }
}

static SCHEDULER_STATE: Locked<SchedulerState> = Locked::new(SchedulerState::new());

struct BuiltinScheduler;

static SCHEDULER: BuiltinScheduler = BuiltinScheduler;

impl BuiltinScheduler {
    fn initialized(&self) -> bool {
        !current_task_context().is_null()
    }

    fn enable(&self) {
        if self.initialized() {
            return;
        }
        allocate_main_task();
        setup_multitasking();
    }

    fn disable(&self) {
        disable_timebase();
        disable_multitasking();
        delete_all_tasks();
    }

    fn yield_task(&self) {
        timer::yield_task()
    }

    fn task_create(
        &self,
        task: extern "C" fn(*mut c_void),
        param: *mut c_void,
        task_stack_size: usize,
    ) -> *mut c_void {
        let task = Box::new_in(Context::new(task, param, task_stack_size), InternalMemory);
        let task_ptr = Box::into_raw(task);

        SCHEDULER_STATE.with(|state| unsafe {
            let current_task = state.current_task;
            debug_assert!(
                !current_task.is_null(),
                "Tried to allocate a task before allocating the main task"
            );
            let next = (*current_task).next;
            (*task_ptr).next = next;
            (*current_task).next = task_ptr;
        });

        task_ptr.cast()
    }

    fn current_task(&self) -> *mut c_void {
        current_task_context().cast()
    }

    fn current_task_thread_semaphore(&self) -> *mut c_void {
        unsafe { &mut ((*current_task_context()).thread_semaphore) as *mut _ as *mut c_void }
    }

    fn schedule_task_deletion(&self, task_handle: *mut c_void) {
        let deleting_current = SCHEDULER_STATE
            .with(|state| state.schedule_task_deletion(task_handle.cast::<Context>()));

        if deleting_current {
            loop {
                timer::yield_task();
            }
        }
    }

    fn max_task_priority(&self) -> u32 {
        255
    }
}

pub(crate) fn initialized() -> bool {
    SCHEDULER.initialized()
}

pub(crate) fn enable() {
    SCHEDULER.enable();
}

pub(crate) fn disable() {
    SCHEDULER.disable();
}

pub(crate) fn yield_task() {
    SCHEDULER.yield_task()
}

pub(crate) fn task_create(
    task: extern "C" fn(*mut c_void),
    param: *mut c_void,
    task_stack_size: usize,
) -> *mut c_void {
    SCHEDULER.task_create(task, param, task_stack_size)
}

pub(crate) fn current_task() -> *mut c_void {
    SCHEDULER.current_task()
}

pub(crate) fn current_task_thread_semaphore() -> *mut c_void {
    SCHEDULER.current_task_thread_semaphore()
}

pub(crate) fn schedule_task_deletion(task_handle: *mut c_void) {
    SCHEDULER.schedule_task_deletion(task_handle)
}

pub(crate) fn max_task_priority() -> u32 {
    SCHEDULER.max_task_priority()
}

fn allocate_main_task() {
    let context = Box::new_in(
        Context {
            cpu_context: CpuContext::default(),
            thread_semaphore: 0,
            next: core::ptr::null_mut(),
            _allocated_stack: Box::<[u8], _>::new_uninit_slice_in(0, InternalMemory),
        },
        InternalMemory,
    );

    let context_ptr = Box::into_raw(context);
    unsafe {
        (*context_ptr).next = context_ptr;
    }

    let already_initialized = SCHEDULER_STATE.with(|state| {
        if state.current_task.is_null() {
            state.current_task = context_ptr;
            false
        } else {
            true
        }
    });

    if already_initialized {
        unsafe {
            core::ptr::drop_in_place(context_ptr);
        }
    }
}

fn delete_all_tasks() {
    let first_task = SCHEDULER_STATE.with(|state| core::mem::take(&mut state.current_task));
    if first_task.is_null() {
        return;
    }

    let mut task_to_delete = first_task;
    loop {
        let next_task = unsafe {
            let next_task = (*task_to_delete).next;
            core::ptr::drop_in_place(task_to_delete);
            next_task
        };

        if core::ptr::eq(next_task, first_task) {
            break;
        }

        task_to_delete = next_task;
    }
}

fn current_task_context() -> *mut Context {
    SCHEDULER_STATE.with(|state| state.current_task)
}

pub(crate) fn task_switch(trap_frame: &mut CpuContext) {
    SCHEDULER_STATE.with(|state| state.switch_task(trap_frame));
}

pub(crate) fn current_task_ptr() -> usize {
    current_task_context() as usize
}

pub(crate) fn current_task_thread_semaphore_ptr() -> usize {
    current_task_thread_semaphore() as usize
}

pub(crate) fn current_task_nonnull() -> Option<NonNull<c_void>> {
    NonNull::new(current_task())
}
