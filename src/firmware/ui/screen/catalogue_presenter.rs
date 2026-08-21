use core::{ffi::CStr, ptr};

use lightvgl_sys as lv;

use crate::firmware::ui::widget::carousel;
use render::intent_bridge;
use shell::catalogue::{CatalogueAction, CatalogueEntry, CatalogueViewKind, DefaultCatalogue};
use shell::settings::UiSettings;

const STYLE_DEFAULT: lv::lv_style_selector_t = 0;
const STYLE_PRESSED: lv::lv_style_selector_t = lv::lv_state_t_LV_STATE_PRESSED;
const ROW_X: i32 = 50;
const ROW_Y: i32 = 78;
const ROW_WIDTH: i32 = 500;
const ROW_HEIGHT: i32 = 48;
const ROW_STEP: i32 = 53;

pub(in crate::firmware::ui) struct CatalogueScreen {
    root: *mut lv::lv_obj_t,
}

impl CatalogueScreen {
    pub(in crate::firmware::ui) fn root(&self) -> *mut lv::lv_obj_t {
        self.root
    }
}

pub(in crate::firmware::ui) unsafe fn create(
    catalogue: &DefaultCatalogue,
    settings: &UiSettings,
    kind: CatalogueViewKind,
    title_text: &'static CStr,
    footer_text: &'static CStr,
    footer_action_index: usize,
    user_data: *mut core::ffi::c_void,
) -> Option<CatalogueScreen> {
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

    if !unsafe { create_title(screen, title_text, black) } {
        unsafe { lv::lv_obj_delete(screen) };
        return None;
    }

    let view = catalogue.view(kind);
    if view.entries().is_empty() {
        if !unsafe { create_empty_state(screen, black) } {
            unsafe { lv::lv_obj_delete(screen) };
            return None;
        }
    } else {
        for (index, entry) in view.entries().iter().enumerate() {
            if !unsafe {
                create_row(
                    screen, *entry, index, kind, settings, user_data, black, white,
                )
            } {
                unsafe { lv::lv_obj_delete(screen) };
                return None;
            }
        }
    }

    if !unsafe {
        carousel::add_navigation(
            screen,
            footer_text.as_ptr(),
            Some(intent_bridge::navigation_callback),
            footer_action_index,
            Some(intent_bridge::navigation_callback),
            footer_action_index,
            user_data,
        )
    } {
        unsafe { lv::lv_obj_delete(screen) };
        return None;
    }
    Some(CatalogueScreen { root: screen })
}

unsafe fn create_title(
    screen: *mut lv::lv_obj_t,
    text: &'static CStr,
    black: lv::lv_color_t,
) -> bool {
    let title = unsafe { lv::lv_label_create(screen) };
    if title.is_null() {
        return false;
    }
    unsafe {
        lv::lv_label_set_text(title, text.as_ptr());
        lv::lv_obj_set_style_text_color(title, black, STYLE_DEFAULT);
        lv::lv_obj_set_style_text_font(
            title,
            ptr::addr_of!(lv::lv_font_montserrat_24),
            STYLE_DEFAULT,
        );
        lv::lv_obj_set_width(title, ROW_WIDTH);
        lv::lv_obj_set_pos(title, ROW_X, 30);
    }
    true
}

unsafe fn create_empty_state(screen: *mut lv::lv_obj_t, black: lv::lv_color_t) -> bool {
    let label = unsafe { lv::lv_label_create(screen) };
    if label.is_null() {
        return false;
    }
    unsafe {
        lv::lv_label_set_text(label, c"No entries available".as_ptr());
        lv::lv_obj_set_style_text_color(label, black, STYLE_DEFAULT);
        lv::lv_obj_set_style_text_font(
            label,
            ptr::addr_of!(lv::lv_font_montserrat_20),
            STYLE_DEFAULT,
        );
        lv::lv_obj_set_pos(label, 188, 240);
    }
    true
}

#[allow(clippy::too_many_arguments)]
unsafe fn create_row(
    screen: *mut lv::lv_obj_t,
    entry: CatalogueEntry,
    index: usize,
    kind: CatalogueViewKind,
    settings: &UiSettings,
    user_data: *mut core::ffi::c_void,
    black: lv::lv_color_t,
    white: lv::lv_color_t,
) -> bool {
    let row = unsafe { lv::lv_button_create(screen) };
    if row.is_null() {
        return false;
    }
    let action_enabled = matches!(entry.action(), CatalogueAction::Enter(_));
    unsafe {
        lv::lv_obj_remove_style_all(row);
        lv::lv_obj_set_size(row, ROW_WIDTH, ROW_HEIGHT);
        lv::lv_obj_set_pos(row, ROW_X, ROW_Y + index as i32 * ROW_STEP);
        lv::lv_obj_set_style_bg_color(row, white, STYLE_DEFAULT);
        lv::lv_obj_set_style_bg_opa(row, 255, STYLE_DEFAULT);
        lv::lv_obj_set_style_text_color(row, black, STYLE_DEFAULT);
        lv::lv_obj_set_style_border_color(row, black, STYLE_DEFAULT);
        lv::lv_obj_set_style_border_width(row, 2, STYLE_DEFAULT);
        lv::lv_obj_set_style_radius(row, 6, STYLE_DEFAULT);
        if action_enabled {
            lv::lv_obj_set_style_bg_color(row, black, STYLE_PRESSED);
            lv::lv_obj_set_style_text_color(row, white, STYLE_PRESSED);
            lv::lv_obj_set_user_data(row, intent_bridge::action_user_data(index));
            if lv::lv_obj_add_event_cb(
                row,
                Some(intent_bridge::navigation_callback),
                lv::lv_event_code_t_LV_EVENT_CLICKED,
                user_data,
            )
            .is_null()
            {
                return false;
            }
        } else {
            lv::lv_obj_remove_flag(row, lv::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE);
        }
    }

    let label = unsafe { lv::lv_label_create(row) };
    let badge = unsafe { lv::lv_label_create(row) };
    if label.is_null() || badge.is_null() {
        return false;
    }
    unsafe {
        lv::lv_label_set_text(label, entry.label.as_ptr());
        lv::lv_obj_set_style_text_font(
            label,
            ptr::addr_of!(lv::lv_font_montserrat_18),
            STYLE_DEFAULT,
        );
        lv::lv_obj_set_pos(label, 14, 13);

        let badge_text = match kind {
            CatalogueViewKind::Launcher => entry.availability.label(),
            CatalogueViewKind::AmbientPicker
                if entry.availability == shell::catalogue::CatalogueAvailability::Ready =>
            {
                if settings.ambient_binding() == entry.id {
                    c"Selected"
                } else {
                    c"Available"
                }
            }
            CatalogueViewKind::OverlaySettings
                if entry.availability == shell::catalogue::CatalogueAvailability::Ready =>
            {
                if settings.overlay_enabled(entry.id) {
                    c"Enabled"
                } else {
                    c"Disabled"
                }
            }
            CatalogueViewKind::AmbientPicker | CatalogueViewKind::OverlaySettings => {
                entry.availability.label()
            }
        };
        lv::lv_label_set_text(badge, badge_text.as_ptr());
        lv::lv_obj_set_style_text_font(
            badge,
            ptr::addr_of!(lv::lv_font_montserrat_14),
            STYLE_DEFAULT,
        );
        lv::lv_obj_set_pos(badge, 360, 15);
    }
    true
}
