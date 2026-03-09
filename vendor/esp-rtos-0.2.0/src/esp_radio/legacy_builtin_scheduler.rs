#![allow(dead_code)]

use core::{cell::RefCell, ffi::c_void, mem::MaybeUninit, ptr::NonNull};

use allocator_api2::boxed::Box;
use embassy_sync::blocking_mutex::Mutex;
use esp_sync::RawMutex;

use crate::{task::CpuContext, InternalMemory};

pub(crate) struct LegacyContext {
    pub(crate) cpu_context: CpuContext,
    pub(crate) thread_semaphore: u32,
    pub(crate) next: *mut LegacyContext,
    pub(crate) _allocated_stack: Box<[MaybeUninit<u8>], InternalMemory>,
}

impl LegacyContext {
    pub(crate) fn new(
        task_fn: extern "C" fn(*mut c_void),
        param: *mut c_void,
        task_stack_size: usize,
    ) -> Self {
        let mut stack = Box::<[u8], _>::new_uninit_slice_in(task_stack_size, InternalMemory);
        let stack_top = unsafe { stack.as_mut_ptr().add(task_stack_size).cast() };

        Self {
            cpu_context: crate::task::new_task_context(task_fn, param, stack_top),
            thread_semaphore: 0,
            next: core::ptr::null_mut(),
            _allocated_stack: stack,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LegacyBuiltinSchedulerSnapshot {
    pub(crate) initialized: bool,
    pub(crate) current_task: usize,
    pub(crate) to_delete: usize,
}

pub(crate) struct LegacyBuiltinSchedulerState {
    current_task: *mut LegacyContext,
    to_delete: *mut LegacyContext,
}

unsafe impl Send for LegacyBuiltinSchedulerState {}

static STATE: Mutex<RawMutex, RefCell<LegacyBuiltinSchedulerState>> =
    Mutex::new(RefCell::new(LegacyBuiltinSchedulerState::new()));

pub(crate) fn with_state<R>(f: impl FnOnce(&mut LegacyBuiltinSchedulerState) -> R) -> R {
    STATE.lock(|shared| f(&mut *shared.borrow_mut()))
}

impl LegacyBuiltinSchedulerState {
    pub(crate) const fn new() -> Self {
        Self {
            current_task: core::ptr::null_mut(),
            to_delete: core::ptr::null_mut(),
        }
    }

    pub(crate) fn snapshot(&self) -> LegacyBuiltinSchedulerSnapshot {
        LegacyBuiltinSchedulerSnapshot {
            initialized: !self.current_task.is_null(),
            current_task: self.current_task as usize,
            to_delete: self.to_delete as usize,
        }
    }

    pub(crate) fn allocate_main_task(&mut self) {
        if !self.current_task.is_null() {
            return;
        }

        let context = Box::new_in(
            LegacyContext {
                cpu_context: CpuContext::new(),
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
        self.current_task = context_ptr;
    }

    pub(crate) fn current_task(&self) -> Option<NonNull<LegacyContext>> {
        NonNull::new(self.current_task)
    }

    pub(crate) fn current_task_thread_semaphore_ptr(&mut self) -> *mut c_void {
        unsafe { &mut (*self.current_task).thread_semaphore as *mut _ as *mut c_void }
    }

    pub(crate) fn task_create(
        &mut self,
        task: extern "C" fn(*mut c_void),
        param: *mut c_void,
        task_stack_size: usize,
    ) -> *mut c_void {
        let task = Box::new_in(LegacyContext::new(task, param, task_stack_size), InternalMemory);
        let task_ptr = Box::into_raw(task);

        unsafe {
            let current_task = self.current_task;
            debug_assert!(
                !current_task.is_null(),
                "legacy builtin scheduler requires main task before create"
            );
            let next = (*current_task).next;
            (*task_ptr).next = next;
            (*current_task).next = task_ptr;
        }

        task_ptr.cast()
    }

    pub(crate) fn schedule_task_deletion(&mut self, task_handle: *mut c_void) -> bool {
        let task = task_handle.cast::<LegacyContext>();
        self.to_delete = task;
        core::ptr::eq(task, self.current_task)
    }

    fn delete_task(&mut self, task: *mut LegacyContext) {
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

    pub(crate) fn switch_task(&mut self, trap_frame: &mut CpuContext) {
        let current_context = unsafe { &mut (*self.current_task).cpu_context as *mut CpuContext };

        if !self.to_delete.is_null() {
            let task_to_delete = core::mem::take(&mut self.to_delete);
            self.delete_task(task_to_delete);
        }

        unsafe {
            self.current_task = (*self.current_task).next;
            crate::task::task_switch(
                current_context,
                &mut (*self.current_task).cpu_context,
                trap_frame,
            );
        }
    }
}

pub(crate) fn snapshot() -> LegacyBuiltinSchedulerSnapshot {
    with_state(|state| state.snapshot())
}

pub(crate) fn allocate_main_task() {
    with_state(|state| state.allocate_main_task());
}

pub(crate) fn current_task() -> Option<NonNull<LegacyContext>> {
    with_state(|state| state.current_task())
}

pub(crate) fn current_task_thread_semaphore_ptr() -> *mut c_void {
    with_state(|state| state.current_task_thread_semaphore_ptr())
}

pub(crate) fn task_create(
    task: extern "C" fn(*mut c_void),
    param: *mut c_void,
    task_stack_size: usize,
) -> *mut c_void {
    with_state(|state| state.task_create(task, param, task_stack_size))
}

pub(crate) fn schedule_task_deletion(task_handle: *mut c_void) -> bool {
    with_state(|state| state.schedule_task_deletion(task_handle))
}

pub(crate) fn switch_task(trap_frame: &mut CpuContext) {
    with_state(|state| state.switch_task(trap_frame));
}
