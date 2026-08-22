use core::ffi::c_char;
use core::ptr;

use lightvgl_sys as lv;

const STYLE_DEFAULT: lv::lv_style_selector_t = 0;
const STYLE_PRESSED: lv::lv_style_selector_t = lv::lv_state_t_LV_STATE_PRESSED;
const NAV_BUTTON_Y: i32 = 522;
const NAV_BUTTON_HIT_PADDING: i32 = 24;

struct NavigationButton {
    x: i32,
    label: *const c_char,
    callback: lv::lv_event_cb_t,
    action_index: usize,
}

pub(in crate::firmware::ui) unsafe fn add_navigation(
    screen: *mut lv::lv_obj_t,
    page_label: *const c_char,
    previous: lv::lv_event_cb_t,
    previous_action_index: usize,
    next: lv::lv_event_cb_t,
    next_action_index: usize,
    user_data: *mut core::ffi::c_void,
) -> bool {
    let black = unsafe { lv::lv_color_black() };
    let white = unsafe { lv::lv_color_white() };
    unsafe {
        if !create_button(
            screen,
            NavigationButton {
                x: 30,
                label: c"<".as_ptr(),
                callback: previous,
                action_index: previous_action_index,
            },
            user_data,
            black,
            white,
        ) || !create_button(
            screen,
            NavigationButton {
                x: 490,
                label: c">".as_ptr(),
                callback: next,
                action_index: next_action_index,
            },
            user_data,
            black,
            white,
        ) {
            return false;
        }
    }

    let indicator = unsafe { lv::lv_label_create(screen) };
    if indicator.is_null() {
        return false;
    }
    unsafe {
        lv::lv_label_set_text(indicator, page_label);
        lv::lv_obj_set_style_text_color(indicator, black, STYLE_DEFAULT);
        lv::lv_obj_set_style_text_font(
            indicator,
            ptr::addr_of!(lv::lv_font_montserrat_18),
            STYLE_DEFAULT,
        );
        lv::lv_obj_set_pos(indicator, 274, 540);
    }
    true
}

unsafe fn create_button(
    screen: *mut lv::lv_obj_t,
    spec: NavigationButton,
    user_data: *mut core::ffi::c_void,
    black: lv::lv_color_t,
    white: lv::lv_color_t,
) -> bool {
    let button = unsafe { lv::lv_button_create(screen) };
    if button.is_null() {
        return false;
    }
    unsafe {
        lv::lv_obj_remove_style_all(button);
        lv::lv_obj_set_size(button, 80, 56);
        lv::lv_obj_set_pos(button, spec.x, NAV_BUTTON_Y);
        lv::lv_obj_set_ext_click_area(button, NAV_BUTTON_HIT_PADDING);
        lv::lv_obj_set_style_bg_color(button, white, STYLE_DEFAULT);
        lv::lv_obj_set_style_bg_opa(button, 255, STYLE_DEFAULT);
        lv::lv_obj_set_style_text_color(button, black, STYLE_DEFAULT);
        lv::lv_obj_set_style_border_color(button, black, STYLE_DEFAULT);
        lv::lv_obj_set_style_border_width(button, 3, STYLE_DEFAULT);
        lv::lv_obj_set_style_radius(button, 8, STYLE_DEFAULT);
        lv::lv_obj_set_style_bg_color(button, black, STYLE_PRESSED);
        lv::lv_obj_set_style_bg_opa(button, 255, STYLE_PRESSED);
        lv::lv_obj_set_style_text_color(button, white, STYLE_PRESSED);
        lv::lv_obj_set_user_data(
            button,
            render::intent_bridge::action_user_data(spec.action_index),
        );
        lv::lv_obj_add_event_cb(
            button,
            spec.callback,
            lv::lv_event_code_t_LV_EVENT_CLICKED,
            user_data,
        );
    }

    let label = unsafe { lv::lv_label_create(button) };
    if label.is_null() {
        return false;
    }
    unsafe {
        lv::lv_label_set_text(label, spec.label);
        lv::lv_obj_set_style_text_font(
            label,
            ptr::addr_of!(lv::lv_font_montserrat_32),
            STYLE_DEFAULT,
        );
        lv::lv_obj_center(label);
    }
    true
}
