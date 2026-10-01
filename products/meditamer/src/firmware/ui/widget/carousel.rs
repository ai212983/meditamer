#![forbid(unsafe_code)]
//! Shared prev/next/page-indicator carousel footer, built on the LVGL safety
//! adapter. Used by every full-panel screen with more than one page:
//! `catalogue_presenter`, `gesture_test`, and `home.rs` all call this
//! directly rather than each keeping its own copy.

use core::ffi::CStr;

use render::intent_bridge::{self, CallbackLease, RouteCallback};
use render::lvgl_adapter::{Font, StyleState, UiAccessToken, Widget, WidgetKind};

const NAV_BUTTON_Y: i32 = 522;
const NAV_BUTTON_HIT_PADDING: i32 = 24;

struct NavigationButton {
    x: i32,
    label: &'static CStr,
    action_index: usize,
}

/// Every caller navigates on both arrows (just to different indices), so
/// this always binds [`RouteCallback::Navigation`] rather than taking a
/// callback per side.
pub(in crate::firmware::ui) fn add_navigation(
    screen: &Widget,
    token: &UiAccessToken,
    page_label: &'static CStr,
    previous_action_index: usize,
    next_action_index: usize,
    lease: &CallbackLease,
) -> bool {
    if !create_button(
        screen,
        token,
        NavigationButton {
            x: 30,
            label: c"<",
            action_index: previous_action_index,
        },
        lease,
    ) || !create_button(
        screen,
        token,
        NavigationButton {
            x: 490,
            label: c">",
            action_index: next_action_index,
        },
        lease,
    ) {
        return false;
    }

    let Ok(indicator) = screen.child(token, WidgetKind::Label) else {
        return false;
    };
    indicator.set_text(token, page_label).is_ok()
        && indicator
            .set_text_color(token, render::lvgl_adapter::black(), StyleState::Default)
            .is_ok()
        && indicator
            .set_text_font(token, Font::Size18, StyleState::Default)
            .is_ok()
        && indicator.set_pos(token, 274, 540).is_ok()
}

fn create_button(
    screen: &Widget,
    token: &UiAccessToken,
    spec: NavigationButton,
    lease: &CallbackLease,
) -> bool {
    let Ok(button) = screen.child(token, WidgetKind::Button) else {
        return false;
    };
    let black = render::lvgl_adapter::black();
    let white = render::lvgl_adapter::white();
    let built = button.remove_style_all(token).is_ok()
        && button.set_size(token, 80, 56).is_ok()
        && button.set_pos(token, spec.x, NAV_BUTTON_Y).is_ok()
        && button
            .set_ext_click_area(token, NAV_BUTTON_HIT_PADDING)
            .is_ok()
        && button
            .set_bg_color(token, white, StyleState::Default)
            .is_ok()
        && button.set_bg_opa(token, 255, StyleState::Default).is_ok()
        && button
            .set_text_color(token, black, StyleState::Default)
            .is_ok()
        && button
            .set_border_color(token, black, StyleState::Default)
            .is_ok()
        && button
            .set_border_width(token, 3, StyleState::Default)
            .is_ok()
        && button.set_radius(token, 8, StyleState::Default).is_ok()
        && button
            .set_bg_color(token, black, StyleState::Pressed)
            .is_ok()
        && button.set_bg_opa(token, 255, StyleState::Pressed).is_ok()
        && button
            .set_text_color(token, white, StyleState::Pressed)
            .is_ok()
        && intent_bridge::bind_click(
            &button,
            token,
            RouteCallback::Navigation {
                index: spec.action_index,
            },
            lease,
        )
        .is_ok();
    if !built {
        return false;
    }

    let Ok(label) = button.child(token, WidgetKind::Label) else {
        return false;
    };
    label.set_text(token, spec.label).is_ok()
        && label
            .set_text_font(token, Font::Size32, StyleState::Default)
            .is_ok()
        && label.center(token).is_ok()
}
