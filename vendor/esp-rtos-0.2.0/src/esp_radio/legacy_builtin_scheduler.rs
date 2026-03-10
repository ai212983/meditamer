#![allow(dead_code)]

use core::{cell::RefCell, ffi::c_void, mem::MaybeUninit, ptr::NonNull};

use allocator_api2::boxed::Box;
use embassy_sync::blocking_mutex::Mutex;
use esp_sync::RawMutex;
use portable_atomic::{AtomicUsize, Ordering};

use crate::{semaphore::Semaphore, task::CpuContext, InternalMemory};

const MAX_ENTRIES: usize = 8;
const TASK_ROLE_LEN: usize = 16;

fn write_role(dst: &mut [u8; TASK_ROLE_LEN], role: &str) {
    dst.fill(0);
    let bytes = role.as_bytes();
    let len = bytes.len().min(TASK_ROLE_LEN.saturating_sub(1));
    dst[..len].copy_from_slice(&bytes[..len]);
}

pub(crate) struct LegacyContext {
    pub(crate) cpu_context: CpuContext,
    pub(crate) thread_semaphore: Option<Semaphore>,
    pub(crate) next: *mut LegacyContext,
    pub(crate) task_role: [u8; TASK_ROLE_LEN],
    pub(crate) _allocated_stack: Box<[MaybeUninit<u8>], InternalMemory>,
}

impl LegacyContext {
    pub(crate) fn new(
        role: &str,
        task_fn: extern "C" fn(*mut c_void),
        param: *mut c_void,
        task_stack_size: usize,
    ) -> Self {
        let mut stack = Box::<[u8], _>::new_uninit_slice_in(task_stack_size, InternalMemory);
        let stack_top = unsafe { stack.as_mut_ptr().add(task_stack_size).cast() };
        let mut task_role = [0; TASK_ROLE_LEN];
        write_role(&mut task_role, role);

        Self {
            cpu_context: crate::task::new_task_context(task_fn, param, stack_top),
            thread_semaphore: None,
            next: core::ptr::null_mut(),
            task_role,
            _allocated_stack: stack,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LegacyBuiltinSchedulerSnapshot {
    pub(crate) initialized: bool,
    pub(crate) current_task: usize,
    pub(crate) to_delete: usize,
    pub(crate) switch_count: usize,
    pub(crate) last_selected_task: usize,
}

pub(crate) struct LegacyBuiltinSchedulerState {
    current_task: *mut LegacyContext,
    to_delete: *mut LegacyContext,
}

unsafe impl Send for LegacyBuiltinSchedulerState {}

static STATE: Mutex<RawMutex, RefCell<LegacyBuiltinSchedulerState>> =
    Mutex::new(RefCell::new(LegacyBuiltinSchedulerState::new()));
static SWITCH_COUNT: AtomicUsize = AtomicUsize::new(0);
static LAST_SELECTED_TASK: AtomicUsize = AtomicUsize::new(0);

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
            switch_count: SWITCH_COUNT.load(Ordering::Relaxed),
            last_selected_task: LAST_SELECTED_TASK.load(Ordering::Relaxed),
        }
    }

    pub(crate) fn allocate_main_task(&mut self) {
        if !self.current_task.is_null() {
            return;
        }

        let context = Box::new_in(
            LegacyContext {
                cpu_context: CpuContext::new(),
                thread_semaphore: None,
                next: core::ptr::null_mut(),
                task_role: {
                    let mut task_role = [0; TASK_ROLE_LEN];
                    write_role(&mut task_role, "main");
                    task_role
                },
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
        unsafe {
            let task = &mut *self.current_task;
            let sem = task
                .thread_semaphore
                .get_or_insert_with(|| Semaphore::new_counting(0, 1));
            sem as *mut _ as *mut c_void
        }
    }

    pub(crate) fn task_create(
        &mut self,
        role: &str,
        task: extern "C" fn(*mut c_void),
        param: *mut c_void,
        task_stack_size: usize,
    ) -> *mut c_void {
        let task = Box::new_in(
            LegacyContext::new(role, task, param, task_stack_size),
            InternalMemory,
        );
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

    fn delete_task(&mut self, task: *mut LegacyContext) -> bool {
        if task.is_null() || self.current_task.is_null() {
            return false;
        }

        let mut current_task = self.current_task;
        let initial = current_task;

        loop {
            if current_task.is_null() {
                break;
            }
            let next_task = unsafe { (*current_task).next };
            if next_task.is_null() {
                break;
            }
            if core::ptr::eq(next_task, task) {
                unsafe {
                    (*current_task).next = (*next_task).next;
                    core::ptr::drop_in_place(task);
                }
                return true;
            }

            if core::ptr::eq(next_task, initial) {
                break;
            }

            current_task = next_task;
        }

        false
    }

    pub(crate) fn switch_task(&mut self, trap_frame: &mut CpuContext) {
        if self.current_task.is_null() {
            return;
        }

        let deleting_current = core::ptr::eq(self.to_delete, self.current_task);
        let current_context = if deleting_current {
            core::ptr::null_mut()
        } else {
            unsafe { &mut (*self.current_task).cpu_context as *mut CpuContext }
        };

        if !self.to_delete.is_null() {
            let task_to_delete = core::mem::take(&mut self.to_delete);
            let next_after_current = unsafe { (*self.current_task).next };
            let deleted = self.delete_task(task_to_delete);
            if deleting_current && deleted {
                self.current_task = next_after_current;
            }
        }

        if self.current_task.is_null() {
            return;
        }

        unsafe {
            self.current_task = (*self.current_task).next;
            SWITCH_COUNT.fetch_add(1, Ordering::Relaxed);
            LAST_SELECTED_TASK.store(self.current_task as usize, Ordering::Relaxed);
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
    role: &str,
    task: extern "C" fn(*mut c_void),
    param: *mut c_void,
    task_stack_size: usize,
) -> *mut c_void {
    with_state(|state| state.task_create(role, task, param, task_stack_size))
}

pub(crate) fn schedule_task_deletion(task_handle: *mut c_void) -> bool {
    with_state(|state| state.schedule_task_deletion(task_handle))
}

pub(crate) fn switch_task(trap_frame: &mut CpuContext) {
    with_state(|state| state.switch_task(trap_frame));
}

pub(crate) fn reset_snapshot() {
    SWITCH_COUNT.store(0, Ordering::Relaxed);
    LAST_SELECTED_TASK.store(0, Ordering::Relaxed);
}

pub(crate) fn task_ptr_at(index: usize) -> usize {
    with_state(|state| {
        let mut ptr = state.current_task;
        if ptr.is_null() || index >= MAX_ENTRIES {
            return 0;
        }
        for _ in 0..index {
            unsafe {
                ptr = (*ptr).next;
            }
            if ptr.is_null() || ptr == state.current_task {
                return 0;
            }
        }
        ptr as usize
    })
}

pub(crate) fn task_role_at(index: usize) -> [u8; TASK_ROLE_LEN] {
    with_state(|state| {
        let mut ptr = state.current_task;
        if ptr.is_null() || index >= MAX_ENTRIES {
            return [0; TASK_ROLE_LEN];
        }
        for _ in 0..index {
            unsafe {
                ptr = (*ptr).next;
            }
            if ptr.is_null() || ptr == state.current_task {
                return [0; TASK_ROLE_LEN];
            }
        }
        unsafe { (*ptr).task_role }
    })
}
