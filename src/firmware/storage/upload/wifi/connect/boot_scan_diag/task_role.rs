use esp_wifi_sys::c_types::{c_char, c_void};

unsafe extern "C" {
    fn esp_rtos_task_role(task: *const c_void) -> *const c_char;
}

pub(super) fn format_task_role(task_ptr: usize) -> &'static str {
    if task_ptr == 0 {
        return "<none>";
    }
    let role_ptr = unsafe { esp_rtos_task_role(task_ptr as *const c_void) };
    if role_ptr.is_null() {
        return "<null>";
    }
    unsafe { core::str::from_utf8_unchecked(core::ffi::CStr::from_ptr(role_ptr.cast()).to_bytes()) }
}
