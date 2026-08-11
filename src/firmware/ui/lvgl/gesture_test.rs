use core::fmt::Write;
use core::ptr;

use heapless::String;
use lightvgl_sys as lv;

use super::io::{LvglGestureDirection, LvglGestureEvent, LvglGestureKind, LvglGestureState};
use super::{carousel, intent_bridge};

const STYLE_DEFAULT: lv::lv_style_selector_t = 0;
const RESULT_TEXT_CAPACITY: usize = 192;

pub(super) struct GestureTestScreen {
    root: *mut lv::lv_obj_t,
    result_label: *mut lv::lv_obj_t,
    gesture_count: u32,
}

impl GestureTestScreen {
    pub(super) fn root(&self) -> *mut lv::lv_obj_t {
        self.root
    }

    pub(super) unsafe fn show_gesture(&mut self, event: LvglGestureEvent, active: bool) -> bool {
        if event.state != LvglGestureState::Ended || !active || self.result_label.is_null() {
            return false;
        }

        self.gesture_count = self.gesture_count.saturating_add(1);
        let sequence = self.gesture_count;
        let mut text = String::<RESULT_TEXT_CAPACITY>::new();
        match event.kind {
            LvglGestureKind::Pinch { scale } => {
                let motion = if scale >= 1.0 { "out" } else { "in" };
                let _ = write!(
                    text,
                    "Gesture #{sequence}\n\nPinch {motion}\nScale: {scale:.3}\nLVGL state: ended"
                );
            }
            LvglGestureKind::Rotation { radians } => {
                let degrees = radians * 57.295_78;
                let _ = write!(
                    text,
                    "Gesture #{sequence}\n\nRotation\nRadians: {radians:.3}\nDegrees: {degrees:.1}\nLVGL state: ended"
                );
            }
            LvglGestureKind::TwoFingerSwipe {
                direction,
                distance_px,
            } => {
                let _ = write!(
                    text,
                    "Gesture #{sequence}\n\nTwo-finger swipe\nDirection: {}\nDistance: {distance_px:.1} px\nLVGL state: ended",
                    direction_label(direction)
                );
            }
        }
        if text.push('\0').is_err() {
            return false;
        }
        unsafe { lv::lv_label_set_text(self.result_label, text.as_ptr().cast()) };
        true
    }
}

pub(super) unsafe fn create(user_data: *mut core::ffi::c_void) -> Option<GestureTestScreen> {
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
        lv::lv_label_set_text(title, c"Multi-gesture test".as_ptr());
        lv::lv_obj_set_style_text_font(
            title,
            ptr::addr_of!(lv::lv_font_montserrat_24),
            STYLE_DEFAULT,
        );
        lv::lv_obj_set_pos(title, 182, 42);
    }

    let instructions = unsafe { lv::lv_label_create(screen) };
    if instructions.is_null() {
        unsafe { lv::lv_obj_delete(screen) };
        return None;
    }
    unsafe {
        lv::lv_label_set_text(
            instructions,
            c"Use two fingers to pinch, rotate, or swipe.\nThe result appears after both fingers are released."
                .as_ptr(),
        );
        lv::lv_obj_set_style_text_font(
            instructions,
            ptr::addr_of!(lv::lv_font_montserrat_18),
            STYLE_DEFAULT,
        );
        lv::lv_obj_set_pos(instructions, 42, 126);
    }

    let result_panel = unsafe { lv::lv_obj_create(screen) };
    if result_panel.is_null() {
        unsafe { lv::lv_obj_delete(screen) };
        return None;
    }
    unsafe {
        lv::lv_obj_set_size(result_panel, 516, 290);
        lv::lv_obj_set_pos(result_panel, 42, 210);
        lv::lv_obj_set_style_bg_color(result_panel, white, STYLE_DEFAULT);
        lv::lv_obj_set_style_bg_opa(result_panel, 255, STYLE_DEFAULT);
        lv::lv_obj_set_style_border_color(result_panel, black, STYLE_DEFAULT);
        lv::lv_obj_set_style_border_width(result_panel, 3, STYLE_DEFAULT);
        lv::lv_obj_set_style_radius(result_panel, 8, STYLE_DEFAULT);
    }

    let result_label = unsafe { lv::lv_label_create(result_panel) };
    if result_label.is_null() {
        unsafe { lv::lv_obj_delete(screen) };
        return None;
    }
    unsafe {
        lv::lv_label_set_text(result_label, c"No gesture detected yet.".as_ptr());
        lv::lv_obj_set_style_text_font(
            result_label,
            ptr::addr_of!(lv::lv_font_montserrat_20),
            STYLE_DEFAULT,
        );
        lv::lv_obj_set_width(result_label, 460);
        lv::lv_obj_set_pos(result_label, 22, 24);
        if !create_overlay_demo_button(result_panel, user_data) {
            lv::lv_obj_delete(screen);
            return None;
        }
        if !carousel::add_navigation(
            screen,
            c"3 / 3".as_ptr(),
            Some(intent_bridge::navigation_callback),
            intent_bridge::HOME_NAVIGATION_INDEX,
            Some(intent_bridge::navigation_callback),
            intent_bridge::HOME_NAVIGATION_INDEX,
            user_data,
        ) {
            lv::lv_obj_delete(screen);
            return None;
        }
    }
    Some(GestureTestScreen {
        root: screen,
        result_label,
        gesture_count: 0,
    })
}

unsafe fn create_overlay_demo_button(
    parent: *mut lv::lv_obj_t,
    user_data: *mut core::ffi::c_void,
) -> bool {
    let button = unsafe { lv::lv_button_create(parent) };
    if button.is_null() {
        return false;
    }
    let black = unsafe { lv::lv_color_black() };
    let white = unsafe { lv::lv_color_white() };
    unsafe {
        lv::lv_obj_remove_style_all(button);
        lv::lv_obj_set_size(button, 190, 64);
        lv::lv_obj_set_pos(button, 292, 202);
        lv::lv_obj_set_style_bg_color(button, white, STYLE_DEFAULT);
        lv::lv_obj_set_style_bg_opa(button, 255, STYLE_DEFAULT);
        lv::lv_obj_set_style_text_color(button, black, STYLE_DEFAULT);
        lv::lv_obj_set_style_border_color(button, black, STYLE_DEFAULT);
        lv::lv_obj_set_style_border_width(button, 3, STYLE_DEFAULT);
        lv::lv_obj_set_style_radius(button, 8, STYLE_DEFAULT);
        lv::lv_obj_set_style_bg_color(button, black, lv::lv_state_t_LV_STATE_PRESSED);
        lv::lv_obj_set_style_text_color(button, white, lv::lv_state_t_LV_STATE_PRESSED);
        if lv::lv_obj_add_event_cb(
            button,
            Some(intent_bridge::show_confirm_callback),
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
        lv::lv_label_set_text(label, c"Overlay demo".as_ptr());
        lv::lv_obj_set_style_text_font(
            label,
            ptr::addr_of!(lv::lv_font_montserrat_18),
            STYLE_DEFAULT,
        );
        lv::lv_obj_center(label);
    }
    true
}

const fn direction_label(direction: LvglGestureDirection) -> &'static str {
    match direction {
        LvglGestureDirection::Left => "left",
        LvglGestureDirection::Right => "right",
        LvglGestureDirection::Up => "up",
        LvglGestureDirection::Down => "down",
        LvglGestureDirection::Unknown => "unknown",
    }
}
