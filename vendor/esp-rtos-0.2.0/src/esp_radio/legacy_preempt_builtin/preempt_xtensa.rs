use core::ffi::c_void;

pub(crate) use crate::task::CpuContext;

pub(crate) fn new_task_context(
    task_fn: extern "C" fn(*mut c_void),
    param: *mut c_void,
    stack_top: *mut (),
) -> CpuContext {
    let stack_top = stack_top as u32;
    let stack_top = stack_top - (stack_top % 16);

    unsafe {
        *((stack_top - 4) as *mut u32) = 0;
        *((stack_top - 8) as *mut u32) = 0;
        *((stack_top - 12) as *mut u32) = stack_top;
        *((stack_top - 16) as *mut u32) = 0;
    }

    CpuContext {
        PC: task_fn as usize as u32,
        A0: 0,
        A1: stack_top,
        A6: param as usize as u32,
        PS: 0x00040000 | ((1 & 3) << 16),
        ..Default::default()
    }
}

pub(crate) fn restore_task_context(ctx: &mut super::Context, trap_frame: &mut CpuContext) {
    *trap_frame = ctx.cpu_context;
}

pub(crate) fn save_task_context(ctx: &mut super::Context, trap_frame: &CpuContext) {
    ctx.cpu_context = *trap_frame;
}
