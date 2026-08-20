//! An LVGL backend written against `board::Panel` and `shell`, rather than
//! retrofitted from Meditamer's.
//!
//! The Inkplate's backend owns a closed `SurfaceModel` enum over six concrete
//! Meditamer screens, which is what stops `firmware::ui::lvgl` moving into
//! `platform/render` (ADR-0015). Starting again here shows what that layer
//! looks like when nothing product-specific is baked in: the surfaces come from
//! the shell's registry, and the pixels leave through the `Panel` trait. If this
//! shape holds, the Inkplate conforms to it rather than the reverse.
//!
//! What this board cannot show is the other half. It has no touchscreen, so
//! nothing here exercises gesture-driven navigation — the part of the shell's
//! interaction model that stays Inkplate-only until Medinote grows input.

use core::ffi::c_void;
use core::ptr;

use board::{DirtyArea, Panel, RefreshMode};
use lightvgl_sys as lv;

/// LVGL renders into this in L8 and hands us dirty rectangles; the panel packs
/// them. Sized for a horizontal band rather than the whole surface, which is
/// how LVGL prefers to work and keeps the buffer off the 120KB the full
/// 400x300 L8 image would need.
const DRAW_LINES: usize = 40;

static mut DRAW_BUFFER: [u8; crate::panel::WIDTH * DRAW_LINES] =
    [0; crate::panel::WIDTH * DRAW_LINES];

/// The panel the flush callback writes into.
///
/// LVGL calls back through a C function pointer with no useful user-data
/// lifetime, so the target is published here for the duration of the UI, the
/// same shape `firmware::ui::lvgl::io` uses on the Inkplate.
static mut ACTIVE_PANEL: *mut dyn Panel = ptr::null_mut::<crate::panel::St7305<'static>>();

/// Set once before `lv_init`, then never again.
pub unsafe fn set_active_panel(panel: &'static mut dyn Panel) {
    unsafe { ACTIVE_PANEL = panel as *mut dyn Panel };
}

unsafe extern "C" fn flush_cb(
    display: *mut lv::lv_display_t,
    area: *const lv::lv_area_t,
    pixels: *mut u8,
) {
    let panel = unsafe { ACTIVE_PANEL };
    if !panel.is_null() && !area.is_null() && !pixels.is_null() {
        let area = unsafe { *area };
        let accepted = unsafe {
            (*panel).blit_l8(
                DirtyArea {
                    x1: area.x1,
                    y1: area.y1,
                    x2: area.x2,
                    y2: area.y2,
                },
                pixels,
            )
        };
        // LVGL flushes a frame in several chunks and only the last is marked;
        // pushing to the glass on every chunk would tear and be slow.
        if accepted && unsafe { lv::lv_display_flush_is_last(display) } {
            let _ = unsafe { (*panel).refresh(RefreshMode::Full) };
        }
    }
    unsafe { lv::lv_display_flush_ready(display) };
}

/// Bring LVGL up against the active panel. Returns the display handle.
pub unsafe fn init(width: i32, height: i32) -> *mut lv::lv_display_t {
    unsafe {
        lv::lv_init();

        let display = lv::lv_display_create(width, height);
        lv::lv_display_set_flush_cb(display, Some(flush_cb));
        lv::lv_display_set_color_format(display, lv::lv_color_format_t_LV_COLOR_FORMAT_L8);
        lv::lv_display_set_buffers(
            display,
            (&raw mut DRAW_BUFFER).cast::<c_void>(),
            ptr::null_mut(),
            core::mem::size_of_val(&*(&raw const DRAW_BUFFER)) as u32,
            lv::lv_display_render_mode_t_LV_DISPLAY_RENDER_MODE_PARTIAL,
        );
        display
    }
}

/// The two labels the sensor task rewrites in place. LVGL owns the objects;
/// these are handles, published once at build time.
static mut READING_LABEL: *mut lv::lv_obj_t = ptr::null_mut();
static mut STATUS_LABEL: *mut lv::lv_obj_t = ptr::null_mut();

/// Rewrite the reading, and mark the label dirty so the next `lv_timer_handler`
/// redraws just that region rather than the whole screen.
pub unsafe fn set_reading(text: &core::ffi::CStr) {
    unsafe {
        if !READING_LABEL.is_null() {
            lv::lv_label_set_text(READING_LABEL, text.as_ptr());
        }
    }
}

pub unsafe fn set_status(text: &core::ffi::CStr) {
    unsafe {
        if !STATUS_LABEL.is_null() {
            lv::lv_label_set_text(STATUS_LABEL, text.as_ptr());
        }
    }
}

/// Build a screen from what a shell provider registered, so the surface on the
/// glass is the one the registry resolved rather than a hardcoded layout.
pub unsafe fn build_screen(title: &core::ffi::CStr, subtitle: &core::ffi::CStr) {
    unsafe {
        let screen = lv::lv_screen_active();
        lv::lv_obj_set_style_bg_color(screen, lv::lv_color_white(), 0);

        let heading = lv::lv_label_create(screen);
        lv::lv_label_set_text(heading, title.as_ptr());
        lv::lv_obj_set_style_text_font(heading, &raw const lv::lv_font_montserrat_24, 0);
        lv::lv_obj_align(heading, lv::lv_align_t_LV_ALIGN_TOP_MID, 0, 24);

        let caption = lv::lv_label_create(screen);
        lv::lv_label_set_text(caption, subtitle.as_ptr());
        lv::lv_obj_set_style_text_font(caption, &raw const lv::lv_font_montserrat_14, 0);
        lv::lv_obj_align(caption, lv::lv_align_t_LV_ALIGN_TOP_MID, 0, 64);

        // A framed box: proves fills, borders and text composite correctly
        // through the L8 path, not just glyphs.
        let panel = lv::lv_obj_create(screen);
        lv::lv_obj_set_size(panel, 240, 120);
        lv::lv_obj_align(panel, lv::lv_align_t_LV_ALIGN_CENTER, 0, 30);
        lv::lv_obj_set_style_border_width(panel, 3, 0);
        lv::lv_obj_set_style_border_color(panel, lv::lv_color_black(), 0);
        lv::lv_obj_set_style_bg_color(panel, lv::lv_color_white(), 0);
        lv::lv_obj_set_style_radius(panel, 8, 0);

        let reading = lv::lv_label_create(panel);
        lv::lv_label_set_text(reading, c"--.- C   --.- %".as_ptr());
        lv::lv_obj_set_style_text_font(reading, &raw const lv::lv_font_montserrat_24, 0);
        lv::lv_obj_align(reading, lv::lv_align_t_LV_ALIGN_CENTER, 0, -12);
        READING_LABEL = reading;

        let status = lv::lv_label_create(panel);
        lv::lv_label_set_text(status, c"waiting for sensor".as_ptr());
        lv::lv_obj_set_style_text_font(status, &raw const lv::lv_font_montserrat_14, 0);
        lv::lv_obj_align(status, lv::lv_align_t_LV_ALIGN_CENTER, 0, 26);
        STATUS_LABEL = status;
    }
}
