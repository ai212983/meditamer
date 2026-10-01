#![forbid(unsafe_code)]
//! Shared full-panel presenter for every compiled-catalogue view (Launcher,
//! Ambient-picker, Overlay-toggles), built on the LVGL safety adapter.

use render::intent_bridge::{self, CallbackLease, RouteCallback};
use render::lvgl_adapter::{Font, StyleState, UiAccessToken, Widget, WidgetKind};
use shell::catalogue::{CatalogueAction, CatalogueEntry, CatalogueViewKind, DefaultCatalogue};
use shell::settings::UiSettings;

use crate::firmware::ui::widget::carousel;

const ROW_X: i32 = 50;
const ROW_Y: i32 = 78;
const ROW_WIDTH: i32 = 500;
const ROW_HEIGHT: i32 = 48;
const ROW_STEP: i32 = 53;

pub(in crate::firmware::ui) struct CatalogueScreen {
    root: Widget,
}

impl CatalogueScreen {
    pub(in crate::firmware::ui) fn root_widget(&self) -> &Widget {
        &self.root
    }

    pub(in crate::firmware::ui) fn destroy(self, token: &UiAccessToken) -> Result<(), Self> {
        match self.root.delete(token) {
            Ok(()) | Err(render::lvgl_adapter::WidgetDeleteFailure::AlreadyGone) => Ok(()),
            Err(render::lvgl_adapter::WidgetDeleteFailure::StillValid(root)) => Err(Self { root }),
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(in crate::firmware::ui) fn create(
    token: &UiAccessToken,
    catalogue: &DefaultCatalogue,
    settings: &UiSettings,
    kind: CatalogueViewKind,
    title_text: &'static core::ffi::CStr,
    footer_text: &'static core::ffi::CStr,
    footer_action_index: usize,
    lease: &CallbackLease,
) -> Option<CatalogueScreen> {
    let screen = Widget::screen(token).ok()?;
    let black = render::lvgl_adapter::black();
    let white = render::lvgl_adapter::white();
    let built = screen
        .set_bg_color(token, white, StyleState::Default)
        .is_ok()
        && screen.set_bg_opa(token, 255, StyleState::Default).is_ok()
        && screen
            .set_text_color(token, black, StyleState::Default)
            .is_ok()
        && create_title(&screen, token, title_text)
        && create_entries(&screen, token, catalogue, settings, kind, lease)
        && carousel::add_navigation(
            &screen,
            token,
            footer_text,
            footer_action_index,
            footer_action_index,
            lease,
        );
    if !built {
        if let Err(render::lvgl_adapter::WidgetDeleteFailure::StillValid(widget)) =
            screen.delete(token)
        {
            render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
        }
        return None;
    }
    Some(CatalogueScreen { root: screen })
}

fn create_entries(
    screen: &Widget,
    token: &UiAccessToken,
    catalogue: &DefaultCatalogue,
    settings: &UiSettings,
    kind: CatalogueViewKind,
    lease: &CallbackLease,
) -> bool {
    let view = catalogue.view(kind);
    if view.entries().is_empty() {
        return create_empty_state(screen, token);
    }
    view.entries()
        .iter()
        .enumerate()
        .all(|(index, entry)| create_row(screen, token, *entry, index, kind, settings, lease))
}

fn create_title(screen: &Widget, token: &UiAccessToken, text: &'static core::ffi::CStr) -> bool {
    let Ok(title) = screen.child(token, WidgetKind::Label) else {
        return false;
    };
    title.set_text(token, text).is_ok()
        && title
            .set_text_color(token, render::lvgl_adapter::black(), StyleState::Default)
            .is_ok()
        && title
            .set_text_font(token, Font::Size24, StyleState::Default)
            .is_ok()
        && title.set_width(token, ROW_WIDTH).is_ok()
        && title.set_pos(token, ROW_X, 30).is_ok()
}

fn create_empty_state(screen: &Widget, token: &UiAccessToken) -> bool {
    let Ok(label) = screen.child(token, WidgetKind::Label) else {
        return false;
    };
    label.set_text(token, c"No entries available").is_ok()
        && label
            .set_text_color(token, render::lvgl_adapter::black(), StyleState::Default)
            .is_ok()
        && label
            .set_text_font(token, Font::Size20, StyleState::Default)
            .is_ok()
        && label.set_pos(token, 188, 240).is_ok()
}

fn create_row(
    screen: &Widget,
    token: &UiAccessToken,
    entry: CatalogueEntry,
    index: usize,
    kind: CatalogueViewKind,
    settings: &UiSettings,
    lease: &CallbackLease,
) -> bool {
    let Ok(row) = screen.child(token, WidgetKind::Button) else {
        return false;
    };
    let black = render::lvgl_adapter::black();
    let white = render::lvgl_adapter::white();
    let action_enabled = matches!(entry.action(), CatalogueAction::Enter(_));
    let built = row.remove_style_all(token).is_ok()
        && row.set_size(token, ROW_WIDTH, ROW_HEIGHT).is_ok()
        && row
            .set_pos(token, ROW_X, ROW_Y + index as i32 * ROW_STEP)
            .is_ok()
        && row.set_bg_color(token, white, StyleState::Default).is_ok()
        && row.set_bg_opa(token, 255, StyleState::Default).is_ok()
        && row
            .set_text_color(token, black, StyleState::Default)
            .is_ok()
        && row
            .set_border_color(token, black, StyleState::Default)
            .is_ok()
        && row.set_border_width(token, 2, StyleState::Default).is_ok()
        && row.set_radius(token, 6, StyleState::Default).is_ok()
        && if action_enabled {
            row.set_bg_color(token, black, StyleState::Pressed).is_ok()
                && row
                    .set_text_color(token, white, StyleState::Pressed)
                    .is_ok()
                && intent_bridge::bind_click(
                    &row,
                    token,
                    RouteCallback::Navigation { index },
                    lease,
                )
                .is_ok()
        } else {
            row.set_clickable(token, false).is_ok()
        };
    if !built {
        return false;
    }

    let Ok(label) = row.child(token, WidgetKind::Label) else {
        return false;
    };
    if label.set_text(token, entry.label).is_err()
        || label
            .set_text_font(token, Font::Size18, StyleState::Default)
            .is_err()
        || label.set_pos(token, 14, 13).is_err()
    {
        return false;
    }

    let Ok(badge) = row.child(token, WidgetKind::Label) else {
        return false;
    };
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
        CatalogueViewKind::OverlayToggles
            if entry.availability == shell::catalogue::CatalogueAvailability::Ready =>
        {
            if settings.overlay_enabled(entry.id) {
                c"Enabled"
            } else {
                c"Disabled"
            }
        }
        CatalogueViewKind::AmbientPicker | CatalogueViewKind::OverlayToggles => {
            entry.availability.label()
        }
    };
    badge.set_text(token, badge_text).is_ok()
        && badge
            .set_text_font(token, Font::Size14, StyleState::Default)
            .is_ok()
        && badge.set_pos(token, 360, 15).is_ok()
}
