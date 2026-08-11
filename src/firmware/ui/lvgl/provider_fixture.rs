use core::ptr;

use lightvgl_sys as lv;

use super::intent_bridge;

const STYLE_DEFAULT: lv::lv_style_selector_t = 0;
const STYLE_PRESSED: lv::lv_style_selector_t = lv::lv_state_t_LV_STATE_PRESSED;

pub(super) struct ProviderFixtureScreen {
    root: *mut lv::lv_obj_t,
    remove_button: *mut lv::lv_obj_t,
}

impl ProviderFixtureScreen {
    pub(super) fn root(&self) -> *mut lv::lv_obj_t {
        self.root
    }

    pub(super) fn send_remove_clicked(&self) -> bool {
        unsafe {
            lv::lv_obj_send_event(
                self.remove_button,
                lv::lv_event_code_t_LV_EVENT_CLICKED,
                ptr::null_mut(),
            ) == lv::lv_result_t_LV_RESULT_OK
        }
    }
}

pub(super) unsafe fn create(user_data: *mut core::ffi::c_void) -> Option<ProviderFixtureScreen> {
    let screen = unsafe { lv::lv_obj_create(ptr::null_mut()) };
    if screen.is_null() {
        return None;
    }
    let white = unsafe { lv::lv_color_white() };
    let black = unsafe { lv::lv_color_black() };
    unsafe {
        lv::lv_obj_set_style_bg_color(screen, white, STYLE_DEFAULT);
        lv::lv_obj_set_style_bg_opa(screen, 255, STYLE_DEFAULT);
        lv::lv_obj_set_style_text_color(screen, black, STYLE_DEFAULT);
    }
    let show_button = unsafe {
        create_button(
            screen,
            230,
            c"Show provider modal".as_ptr(),
            Some(intent_bridge::show_confirm_callback),
            user_data,
        )
    };
    let remove_button = unsafe {
        create_button(
            screen,
            370,
            c"Remove provider".as_ptr(),
            Some(intent_bridge::navigation_callback),
            user_data,
        )
    };
    if !unsafe { create_label(screen, c"Provider removal fixture".as_ptr(), 150, 90, 24) }
        || show_button.is_null()
        || remove_button.is_null()
    {
        unsafe { lv::lv_obj_delete(screen) };
        return None;
    }
    unsafe {
        lv::lv_obj_set_user_data(
            remove_button,
            intent_bridge::action_user_data(intent_bridge::HOME_NAVIGATION_INDEX),
        )
    };
    Some(ProviderFixtureScreen {
        root: screen,
        remove_button,
    })
}

unsafe fn create_label(
    parent: *mut lv::lv_obj_t,
    text: *const core::ffi::c_char,
    x: i32,
    y: i32,
    font_size: u8,
) -> bool {
    let label = unsafe { lv::lv_label_create(parent) };
    if label.is_null() {
        return false;
    }
    unsafe {
        lv::lv_label_set_text(label, text);
        let font = if font_size == 24 {
            ptr::addr_of!(lv::lv_font_montserrat_24)
        } else {
            ptr::addr_of!(lv::lv_font_montserrat_20)
        };
        lv::lv_obj_set_style_text_font(label, font, STYLE_DEFAULT);
        lv::lv_obj_set_pos(label, x, y);
    }
    true
}

unsafe fn create_button(
    parent: *mut lv::lv_obj_t,
    y: i32,
    text: *const core::ffi::c_char,
    callback: lv::lv_event_cb_t,
    user_data: *mut core::ffi::c_void,
) -> *mut lv::lv_obj_t {
    let button = unsafe { lv::lv_button_create(parent) };
    if button.is_null() {
        return ptr::null_mut();
    }
    let black = unsafe { lv::lv_color_black() };
    let white = unsafe { lv::lv_color_white() };
    unsafe {
        lv::lv_obj_remove_style_all(button);
        lv::lv_obj_set_size(button, 360, 96);
        lv::lv_obj_set_pos(button, 120, y);
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
            callback,
            lv::lv_event_code_t_LV_EVENT_CLICKED,
            user_data,
        );
    }
    let label = unsafe { lv::lv_label_create(button) };
    if label.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        lv::lv_label_set_text(label, text);
        lv::lv_obj_set_style_text_font(
            label,
            ptr::addr_of!(lv::lv_font_montserrat_20),
            STYLE_DEFAULT,
        );
        lv::lv_obj_center(label);
    }
    button
}
