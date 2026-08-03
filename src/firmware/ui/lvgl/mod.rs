mod backend;
mod dither;
mod home;
mod io;

pub(crate) use backend::{Backend, InitError};
pub(crate) use dither::DirtyArea;

const WIDTH: i32 = 600;
const HEIGHT: i32 = 600;

#[no_mangle]
pub extern "C" fn meditamer_lvgl_alloc_pool(size: usize) -> *mut core::ffi::c_void {
    backend::alloc_pool(size)
}
