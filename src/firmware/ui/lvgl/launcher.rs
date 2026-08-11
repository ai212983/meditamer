use core::ptr;

use lightvgl_sys as lv;

use super::{carousel, intent_bridge};

const STYLE_DEFAULT: lv::lv_style_selector_t = 0;
const STYLE_PRESSED: lv::lv_style_selector_t = lv::lv_state_t_LV_STATE_PRESSED;

pub(super) struct LauncherScreen {
    root: *mut lv::lv_obj_t,
}

impl LauncherScreen {
    pub(super) fn root(&self) -> *mut lv::lv_obj_t {
        self.root
    }
}

pub(super) unsafe fn create(user_data: *mut core::ffi::c_void) -> Option<LauncherScreen> {
    let screen = unsafe { lv::lv_obj_create(ptr::null_mut()) };
    if screen.is_null() {
        return None;
    }

    let black = unsafe { lv::lv_color_black() };
    let white = unsafe { lv::lv_color_white() };
    unsafe {
        lv::lv_obj_set_style_bg_color(screen, white, STYLE_DEFAULT);
        lv::lv_obj_set_style_bg_opa(screen, 255, STYLE_DEFAULT);
        lv::lv_obj_set_style_text_color(screen, black, STYLE_DEFAULT);
    }

    let title = unsafe { lv::lv_label_create(screen) };
    if title.is_null() {
        unsafe { lv::lv_obj_delete(screen) };
        return None;
    }
    unsafe {
        lv::lv_label_set_text(title, c"Launcher".as_ptr());
        lv::lv_obj_set_style_text_font(
            title,
            ptr::addr_of!(lv::lv_font_montserrat_24),
            STYLE_DEFAULT,
        );
        lv::lv_obj_set_pos(title, 252, 90);
    }

    let button = unsafe { lv::lv_button_create(screen) };
    if button.is_null() {
        unsafe { lv::lv_obj_delete(screen) };
        return None;
    }
    unsafe {
        lv::lv_obj_remove_style_all(button);
        lv::lv_obj_set_size(button, 360, 100);
        lv::lv_obj_set_pos(button, 120, 230);
        lv::lv_obj_set_style_bg_color(button, white, STYLE_DEFAULT);
        lv::lv_obj_set_style_bg_opa(button, 255, STYLE_DEFAULT);
        lv::lv_obj_set_style_text_color(button, black, STYLE_DEFAULT);
        lv::lv_obj_set_style_border_color(button, black, STYLE_DEFAULT);
        lv::lv_obj_set_style_border_width(button, 3, STYLE_DEFAULT);
        lv::lv_obj_set_style_radius(button, 8, STYLE_DEFAULT);
        lv::lv_obj_set_style_bg_color(button, black, STYLE_PRESSED);
        lv::lv_obj_set_style_text_color(button, white, STYLE_PRESSED);
        lv::lv_obj_add_event_cb(
            button,
            Some(intent_bridge::launch_diagnostics_callback),
            lv::lv_event_code_t_LV_EVENT_CLICKED,
            user_data,
        );
    }

    let label = unsafe { lv::lv_label_create(button) };
    if label.is_null() {
        unsafe { lv::lv_obj_delete(screen) };
        return None;
    }
    unsafe {
        lv::lv_label_set_text(label, c"Gesture diagnostics".as_ptr());
        lv::lv_obj_set_style_text_font(
            label,
            ptr::addr_of!(lv::lv_font_montserrat_20),
            STYLE_DEFAULT,
        );
        lv::lv_obj_center(label);
        if !carousel::add_navigation(
            screen,
            c"2 / 3".as_ptr(),
            Some(intent_bridge::home_callback),
            Some(intent_bridge::launch_diagnostics_callback),
            user_data,
        ) {
            lv::lv_obj_delete(screen);
            return None;
        }
    }
    Some(LauncherScreen { root: screen })
}
