#![forbid(unsafe_code)]
//! Provider-removal fixture screen, built on the LVGL safety adapter.

use render::intent_bridge::{self, CallbackLease, RouteCallback};
use render::lvgl_adapter::{
    Font, StyleState, UiAccessToken, Widget, WidgetDeleteFailure, WidgetKind,
};

pub(in crate::firmware::ui) struct ProviderFixtureScreen {
    root: Widget,
    remove_button: Widget,
}

impl ProviderFixtureScreen {
    pub(in crate::firmware::ui) fn root_widget(&self) -> &Widget {
        &self.root
    }

    pub(in crate::firmware::ui) fn destroy(self, token: &UiAccessToken) -> Result<(), Self> {
        let Self {
            root,
            remove_button,
        } = self;
        match root.delete(token) {
            Ok(()) | Err(WidgetDeleteFailure::AlreadyGone) => Ok(()),
            Err(WidgetDeleteFailure::StillValid(root)) => Err(Self {
                root,
                remove_button,
            }),
        }
    }

    pub(in crate::firmware::ui) fn send_remove_clicked(&self, token: &UiAccessToken) -> bool {
        self.remove_button.send_click(token).unwrap_or(false)
    }
}

pub(in crate::firmware::ui) fn create(
    token: &UiAccessToken,
    lease: &CallbackLease,
) -> Option<ProviderFixtureScreen> {
    let screen = Widget::screen(token).ok()?;
    let white = render::lvgl_adapter::white();
    let black = render::lvgl_adapter::black();
    let built = screen
        .set_bg_color(token, white, StyleState::Default)
        .is_ok()
        && screen.set_bg_opa(token, 255, StyleState::Default).is_ok()
        && screen
            .set_text_color(token, black, StyleState::Default)
            .is_ok()
        && create_label(
            &screen,
            token,
            c"Provider removal fixture",
            150,
            90,
            Font::Size24,
        );
    if !built {
        if let Err(WidgetDeleteFailure::StillValid(widget)) = screen.delete(token) {
            render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
        }
        return None;
    }

    let Some(_show_button) = create_button(
        &screen,
        token,
        230,
        c"Show provider modal",
        RouteCallback::ShowConfirm,
        lease,
    ) else {
        if let Err(WidgetDeleteFailure::StillValid(widget)) = screen.delete(token) {
            render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
        }
        return None;
    };
    let Some(remove_button) = create_button(
        &screen,
        token,
        370,
        c"Remove provider",
        RouteCallback::Navigation {
            index: intent_bridge::HOME_NAVIGATION_INDEX,
        },
        lease,
    ) else {
        if let Err(WidgetDeleteFailure::StillValid(widget)) = screen.delete(token) {
            render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
        }
        return None;
    };
    Some(ProviderFixtureScreen {
        root: screen,
        remove_button,
    })
}

fn create_label(
    parent: &Widget,
    token: &UiAccessToken,
    text: &'static core::ffi::CStr,
    x: i32,
    y: i32,
    font: Font,
) -> bool {
    let Ok(label) = parent.child(token, WidgetKind::Label) else {
        return false;
    };
    label.set_text(token, text).is_ok()
        && label
            .set_text_font(token, font, StyleState::Default)
            .is_ok()
        && label.set_pos(token, x, y).is_ok()
}

fn create_button(
    parent: &Widget,
    token: &UiAccessToken,
    y: i32,
    text: &'static core::ffi::CStr,
    callback: RouteCallback,
    lease: &CallbackLease,
) -> Option<Widget> {
    let button = parent.child(token, WidgetKind::Button).ok()?;
    let black = render::lvgl_adapter::black();
    let white = render::lvgl_adapter::white();
    let built = button.remove_style_all(token).is_ok()
        && button.set_size(token, 360, 96).is_ok()
        && button.set_pos(token, 120, y).is_ok()
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
        && intent_bridge::bind_click(&button, token, callback, lease).is_ok();
    if !built {
        return None;
    }
    let label = button.child(token, WidgetKind::Label).ok()?;
    if label.set_text(token, text).is_err()
        || label
            .set_text_font(token, Font::Size20, StyleState::Default)
            .is_err()
        || label.center(token).is_err()
    {
        return None;
    }
    Some(button)
}
