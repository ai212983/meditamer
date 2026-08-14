use core::ptr;

use lightvgl_sys as lv;

use crate::firmware::ui::lvgl::intent_bridge;
use crate::firmware::ui::widget::carousel;

const STYLE_DEFAULT: lv::lv_style_selector_t = 0;
const STYLE_PRESSED: lv::lv_style_selector_t = lv::lv_state_t_LV_STATE_PRESSED;

pub(in crate::firmware::ui) struct HomeScreen {
    root: *mut lv::lv_obj_t,
}

impl HomeScreen {
    pub(in crate::firmware::ui) fn root(&self) -> *mut lv::lv_obj_t {
        self.root
    }
}

pub(in crate::firmware::ui) unsafe fn create(
    user_data: *mut core::ffi::c_void,
) -> Option<HomeScreen> {
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

    if !create_top_test_button(screen, black, white, user_data) {
        unsafe { lv::lv_obj_delete(screen) };
        return None;
    }

    if !unsafe { create_ambient_content(screen) } {
        unsafe { lv::lv_obj_delete(screen) };
        return None;
    }

    let hint = unsafe { lv::lv_label_create(screen) };
    if hint.is_null() {
        unsafe { lv::lv_obj_delete(screen) };
        return None;
    }
    unsafe {
        lv::lv_label_set_text(hint, c"Use the arrows to browse test pages.".as_ptr());
        lv::lv_obj_set_style_text_font(
            hint,
            ptr::addr_of!(lv::lv_font_montserrat_18),
            STYLE_DEFAULT,
        );
        lv::lv_obj_set_pos(hint, 146, 376);
        if !carousel::add_navigation(
            screen,
            c"1 / 3".as_ptr(),
            Some(intent_bridge::navigation_callback),
            0,
            Some(intent_bridge::navigation_callback),
            0,
            user_data,
        ) {
            lv::lv_obj_delete(screen);
            return None;
        }
    }
    Some(HomeScreen { root: screen })
}

pub(in crate::firmware::ui) unsafe fn create_ambient_content(screen: *mut lv::lv_obj_t) -> bool {
    let title = unsafe { lv::lv_label_create(screen) };
    if title.is_null() {
        return false;
    }
    unsafe {
        lv::lv_label_set_text(title, c"Meditamer".as_ptr());
        lv::lv_obj_set_style_text_font(
            title,
            ptr::addr_of!(lv::lv_font_montserrat_24),
            STYLE_DEFAULT,
        );
        lv::lv_obj_set_pos(title, 234, 260);
    }

    let status = unsafe { lv::lv_label_create(screen) };
    if status.is_null() {
        return false;
    }
    unsafe {
        lv::lv_label_set_text(status, c"Ready".as_ptr());
        lv::lv_obj_set_style_text_font(
            status,
            ptr::addr_of!(lv::lv_font_montserrat_18),
            STYLE_DEFAULT,
        );
        lv::lv_obj_set_pos(status, 274, 306);
    }

    true
}

unsafe fn create_top_test_button(
    screen: *mut lv::lv_obj_t,
    black: lv::lv_color_t,
    white: lv::lv_color_t,
    user_data: *mut core::ffi::c_void,
) -> bool {
    let button = unsafe { lv::lv_button_create(screen) };
    if button.is_null() {
        return false;
    }
    unsafe {
        // Own the complete button style so LVGL's theme cannot add shadows or
        // pressed-state geometry outside the requested object bounds.
        lv::lv_obj_remove_style_all(button);
        lv::lv_obj_set_size(button, 180, 64);
        lv::lv_obj_set_pos(button, 210, 42);
        lv::lv_obj_set_style_bg_color(button, white, STYLE_DEFAULT);
        lv::lv_obj_set_style_bg_opa(button, 255, STYLE_DEFAULT);
        lv::lv_obj_set_style_text_color(button, black, STYLE_DEFAULT);
        lv::lv_obj_set_style_border_color(button, black, STYLE_DEFAULT);
        lv::lv_obj_set_style_border_width(button, 3, STYLE_DEFAULT);
        lv::lv_obj_set_style_radius(button, 8, STYLE_DEFAULT);
        lv::lv_obj_set_style_bg_color(button, black, STYLE_PRESSED);
        lv::lv_obj_set_style_text_color(button, white, STYLE_PRESSED);
        lv::lv_obj_set_user_data(button, intent_bridge::action_user_data(0));
        if lv::lv_obj_add_event_cb(
            button,
            Some(intent_bridge::navigation_callback),
            lv::lv_event_code_t_LV_EVENT_CLICKED,
            user_data,
        )
        .is_null()
        {
            return false;
        }
    }

    let label = unsafe { lv::lv_label_create(button) };
    if label.is_null() {
        return false;
    }
    unsafe {
        lv::lv_label_set_text(label, c"TOP TEST".as_ptr());
        lv::lv_obj_set_style_text_font(
            label,
            ptr::addr_of!(lv::lv_font_montserrat_18),
            STYLE_DEFAULT,
        );
        lv::lv_obj_center(label);
    }
    true
}
