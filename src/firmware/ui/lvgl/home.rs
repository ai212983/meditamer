use core::ptr;

use lightvgl_sys as lv;

const STYLE_DEFAULT: lv::lv_style_selector_t = 0;

pub(super) unsafe fn create() {
    let screen = unsafe { lv::lv_screen_active() };
    let black = unsafe { lv::lv_color_black() };
    let white = unsafe { lv::lv_color_white() };

    unsafe {
        lv::lv_obj_set_style_bg_color(screen, white, STYLE_DEFAULT);
        lv::lv_obj_set_style_bg_opa(screen, 255, STYLE_DEFAULT);
        lv::lv_obj_set_style_text_color(screen, black, STYLE_DEFAULT);
    }

    let title = unsafe { lv::lv_label_create(screen) };
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
    unsafe {
        lv::lv_label_set_text(status, c"Ready".as_ptr());
        lv::lv_obj_set_style_text_font(
            status,
            ptr::addr_of!(lv::lv_font_montserrat_18),
            STYLE_DEFAULT,
        );
        lv::lv_obj_set_pos(status, 274, 306);
    }
}
