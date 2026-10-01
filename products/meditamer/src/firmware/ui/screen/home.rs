#![forbid(unsafe_code)]
//! Home screen, built on the LVGL safety adapter -- no raw `lv_obj_t`
//! pointers.

use lightvgl_sys as lv;
use render::intent_bridge::{self, CallbackLease, RouteCallback};
use render::lvgl_adapter::{
    Font, StyleState, UiAccessToken, Widget, WidgetDeleteFailure, WidgetKind,
};

use crate::firmware::ui::widget::ambient_content;

pub(in crate::firmware::ui) struct HomeScreen {
    root: Widget,
}

impl HomeScreen {
    pub(in crate::firmware::ui) fn root_widget(&self) -> &Widget {
        &self.root
    }

    /// Returns the intact screen if LVGL reports the root still valid, so the
    /// caller can retry deletion.
    pub(in crate::firmware::ui) fn destroy(self, token: &UiAccessToken) -> Result<(), Self> {
        match self.root.delete(token) {
            Ok(()) | Err(WidgetDeleteFailure::AlreadyGone) => Ok(()),
            Err(WidgetDeleteFailure::StillValid(root)) => Err(Self { root }),
        }
    }
}

pub(in crate::firmware::ui) fn create(
    token: &UiAccessToken,
    lease: &CallbackLease,
) -> Option<HomeScreen> {
    let screen = Widget::screen(token).ok()?;
    if build(&screen, token, lease) {
        return Some(HomeScreen { root: screen });
    }
    // Tear down partial construction; callers see only the `None` outcome.
    if let Err(WidgetDeleteFailure::StillValid(widget)) = screen.delete(token) {
        render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
    }
    None
}

fn build(screen: &Widget, token: &UiAccessToken, lease: &CallbackLease) -> bool {
    let black = render::lvgl_adapter::black();
    let white = render::lvgl_adapter::white();
    screen
        .set_bg_color(token, white, StyleState::Default)
        .is_ok()
        && screen.set_bg_opa(token, 255, StyleState::Default).is_ok()
        && screen
            .set_text_color(token, black, StyleState::Default)
            .is_ok()
        && create_top_test_button(screen, token, black, white, lease)
        && ambient_content::create(screen, token)
        && create_hint_label(screen, token)
        && add_test_page_navigation(screen, token, lease)
        && create_settings_button(screen, token, black, white, lease)
}

/// Opens Settings over a live Home through the screen's modal-request slot.
fn create_settings_button(
    screen: &Widget,
    token: &UiAccessToken,
    black: lv::lv_color_t,
    white: lv::lv_color_t,
    lease: &CallbackLease,
) -> bool {
    let Ok(button) = screen.child(token, WidgetKind::Button) else {
        return false;
    };
    let built = button.remove_style_all(token).is_ok()
        && button.set_size(token, 100, 48).is_ok()
        && button.set_pos(token, 16, 16).is_ok()
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
            .set_border_width(token, 2, StyleState::Default)
            .is_ok()
        && button.set_radius(token, 8, StyleState::Default).is_ok()
        && button
            .set_bg_color(token, black, StyleState::Pressed)
            .is_ok()
        && button
            .set_text_color(token, white, StyleState::Pressed)
            .is_ok()
        && intent_bridge::bind_click(&button, token, RouteCallback::ShowConfirm, lease).is_ok();
    if !built {
        return false;
    }

    let Ok(label) = button.child(token, WidgetKind::Label) else {
        return false;
    };
    label.set_text(token, c"Settings").is_ok()
        && label
            .set_text_font(token, Font::Size14, StyleState::Default)
            .is_ok()
        && label.center(token).is_ok()
}

// Home has one carousel page, so both arrows use its navigation action.
fn add_test_page_navigation(screen: &Widget, token: &UiAccessToken, lease: &CallbackLease) -> bool {
    crate::firmware::ui::widget::carousel::add_navigation(screen, token, c"1 / 3", 0, 0, lease)
}

fn create_hint_label(screen: &Widget, token: &UiAccessToken) -> bool {
    let Ok(hint) = screen.child(token, WidgetKind::Label) else {
        return false;
    };
    hint.set_text(token, c"Use the arrows to browse test pages.")
        .is_ok()
        && hint
            .set_text_font(token, Font::Size18, StyleState::Default)
            .is_ok()
        && hint.set_pos(token, 146, 376).is_ok()
}

fn create_top_test_button(
    screen: &Widget,
    token: &UiAccessToken,
    black: lv::lv_color_t,
    white: lv::lv_color_t,
    lease: &CallbackLease,
) -> bool {
    let Ok(button) = screen.child(token, WidgetKind::Button) else {
        return false;
    };
    let built = button.remove_style_all(token).is_ok()
        && button.set_size(token, 180, 64).is_ok()
        && button.set_pos(token, 210, 42).is_ok()
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
        && button
            .set_text_color(token, white, StyleState::Pressed)
            .is_ok()
        && intent_bridge::bind_click(
            &button,
            token,
            RouteCallback::Navigation { index: 0 },
            lease,
        )
        .is_ok();
    if !built {
        return false;
    }

    let Ok(label) = button.child(token, WidgetKind::Label) else {
        return false;
    };
    label.set_text(token, c"TOP TEST").is_ok()
        && label
            .set_text_font(token, Font::Size18, StyleState::Default)
            .is_ok()
        && label.center(token).is_ok()
}
