#![forbid(unsafe_code)]
//! Home screen's ambient title/status labels, built on the LVGL safety
//! adapter.

use render::lvgl_adapter::{Font, StyleState, UiAccessToken, Widget, WidgetKind};

pub(in crate::firmware::ui) fn create(screen: &Widget, token: &UiAccessToken) -> bool {
    let Ok(title) = screen.child(token, WidgetKind::Label) else {
        return false;
    };
    if title.set_text(token, c"Meditamer").is_err()
        || title
            .set_text_font(token, Font::Size24, StyleState::Default)
            .is_err()
        || title.set_pos(token, 234, 260).is_err()
    {
        return false;
    }

    let Ok(status) = screen.child(token, WidgetKind::Label) else {
        return false;
    };
    status.set_text(token, c"Ready").is_ok()
        && status
            .set_text_font(token, Font::Size18, StyleState::Default)
            .is_ok()
        && status.set_pos(token, 274, 306).is_ok()
}
