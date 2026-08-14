use core::ptr;

use lightvgl_sys as lv;

use crate::firmware::ui::lvgl::intent_bridge;

const STYLE_DEFAULT: lv::lv_style_selector_t = 0;
const STYLE_PRESSED: lv::lv_style_selector_t = lv::lv_state_t_LV_STATE_PRESSED;

pub(in crate::firmware::ui) struct AmbientViewScreen {
    root: *mut lv::lv_obj_t,
}

impl AmbientViewScreen {
    pub(in crate::firmware::ui) fn root(&self) -> *mut lv::lv_obj_t {
        self.root
    }
}

pub(in crate::firmware::ui) unsafe fn create(
    user_data: *mut core::ffi::c_void,
) -> Option<AmbientViewScreen> {
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

    if !unsafe { super::home::create_ambient_content(screen) }
        || !unsafe { create_back_button(screen, user_data, black, white) }
    {
        unsafe { lv::lv_obj_delete(screen) };
        return None;
    }

    Some(AmbientViewScreen { root: screen })
}

unsafe fn create_back_button(
    screen: *mut lv::lv_obj_t,
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
        lv::lv_obj_set_size(button, 180, 56);
        lv::lv_obj_set_pos(button, 210, 522);
        lv::lv_obj_set_ext_click_area(button, 16);
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
            intent_bridge::action_user_data(intent_bridge::BACK_NAVIGATION_INDEX),
        );
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
        lv::lv_label_set_text(label, c"Back".as_ptr());
        lv::lv_obj_set_style_text_font(
            label,
            ptr::addr_of!(lv::lv_font_montserrat_18),
            STYLE_DEFAULT,
        );
        lv::lv_obj_center(label);
    }
    true
}
