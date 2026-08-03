use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

use lightvgl_sys as lv;

use super::dither::DirtyArea;
use super::home;
use super::{io, HEIGHT, WIDTH};
use crate::firmware::{
    psram::{self, BufferPlacement},
    touch::types::TouchEvent,
    types::InkplateDriver,
};

const BUFFER_LINES: usize = 16;
const BUFFER_BYTES: usize = WIDTH as usize * BUFFER_LINES;
const BUFFER_WORDS: usize = BUFFER_BYTES.div_ceil(core::mem::size_of::<u32>());
const MEMORY_POOL_BYTES: usize = 128 * 1024;

static LVGL_DISPLAY: AtomicPtr<lv::lv_display_t> = AtomicPtr::new(ptr::null_mut());
static LVGL_INPUT: AtomicPtr<lv::lv_indev_t> = AtomicPtr::new(ptr::null_mut());
static MEMORY_POOL: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());
static mut DRAW_BUFFER: [u32; BUFFER_WORDS] = [0; BUFFER_WORDS];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InitError {
    MemoryPoolUnavailable,
    DisplayCreationFailed,
}

pub(crate) struct Backend {
    _private: (),
}

impl Backend {
    pub(crate) fn initialize(
        display: &mut InkplateDriver,
    ) -> Result<(Self, Option<DirtyArea>), InitError> {
        prepare_memory_pool()?;

        unsafe {
            lv::lv_init();
            let lv_display = lv::lv_display_create(WIDTH, HEIGHT);
            if lv_display.is_null() {
                return Err(InitError::DisplayCreationFailed);
            }
            LVGL_DISPLAY.store(lv_display, Ordering::Release);
            lv::lv_display_set_color_format(lv_display, lv::lv_color_format_t_LV_COLOR_FORMAT_L8);
            lv::lv_display_set_buffers(
                lv_display,
                ptr::addr_of_mut!(DRAW_BUFFER).cast(),
                ptr::null_mut(),
                BUFFER_BYTES as u32,
                lv::lv_display_render_mode_t_LV_DISPLAY_RENDER_MODE_PARTIAL,
            );
            lv::lv_display_set_flush_cb(lv_display, Some(io::flush_callback));

            let input = lv::lv_indev_create();
            if !input.is_null() {
                lv::lv_indev_set_type(input, lv::lv_indev_type_t_LV_INDEV_TYPE_POINTER);
                lv::lv_indev_set_read_cb(input, Some(io::input_callback));
                LVGL_INPUT.store(input, Ordering::Release);
            }
            home::create();
        }

        let mut backend = Self { _private: () };
        let rendered = backend
            .run_timers(display, 250)
            .or_else(|| backend.invalidate(display));
        Ok((backend, rendered))
    }

    pub(crate) fn handle_touch(
        &mut self,
        display: &mut InkplateDriver,
        event: TouchEvent,
    ) -> Option<DirtyArea> {
        io::update_touch(event);
        render_with(display, || unsafe {
            let input = LVGL_INPUT.load(Ordering::Acquire);
            if !input.is_null() {
                // Force one read per pipeline event so a queued Down+Up pair cannot
                // collapse into a single released sample before LVGL observes it.
                lv::lv_indev_read(input);
            }
        })
    }

    pub(crate) fn run_timers(
        &mut self,
        display: &mut InkplateDriver,
        elapsed_ms: u32,
    ) -> Option<DirtyArea> {
        io::begin(display);
        unsafe {
            lv::lv_tick_inc(elapsed_ms);
            lv::lv_timer_handler();
        }
        io::finish()
    }

    pub(crate) fn invalidate(&mut self, display: &mut InkplateDriver) -> Option<DirtyArea> {
        render_with(display, || unsafe {
            let screen = lv::lv_screen_active();
            if !screen.is_null() {
                let _ = lv::lv_obj_invalidate(screen);
            }
        })
    }
}

fn prepare_memory_pool() -> Result<(), InitError> {
    if !MEMORY_POOL.load(Ordering::Acquire).is_null() {
        return Ok(());
    }
    let Ok(mut pool) = psram::alloc_large_byte_buffer(MEMORY_POOL_BYTES) else {
        return Err(InitError::MemoryPoolUnavailable);
    };
    if pool.placement() != BufferPlacement::Psram {
        return Err(InitError::MemoryPoolUnavailable);
    }
    let pool_ptr = pool.as_mut_slice().as_mut_ptr();
    if !(pool_ptr as usize).is_multiple_of(core::mem::align_of::<u32>()) {
        return Err(InitError::MemoryPoolUnavailable);
    }
    MEMORY_POOL.store(pool_ptr, Ordering::Release);
    core::mem::forget(pool);
    psram::log_allocator_high_water("lvgl_memory_pool_alloc");
    Ok(())
}

pub(super) fn alloc_pool(size: usize) -> *mut core::ffi::c_void {
    if size > MEMORY_POOL_BYTES {
        return ptr::null_mut();
    }
    MEMORY_POOL.load(Ordering::Acquire).cast()
}

fn render_with(display: &mut InkplateDriver, update: impl FnOnce()) -> Option<DirtyArea> {
    io::begin(display);
    update();
    unsafe {
        let lv_display = LVGL_DISPLAY.load(Ordering::Acquire);
        if !lv_display.is_null() {
            lv::lv_refr_now(lv_display);
        }
    }
    io::finish()
}
