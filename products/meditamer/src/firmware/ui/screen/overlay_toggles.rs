#![forbid(unsafe_code)]

use super::catalogue_presenter;
use render::intent_bridge::{self, CallbackLease};
use render::lvgl_adapter::UiAccessToken;
use shell::catalogue::{CatalogueViewKind, DefaultCatalogue};
use shell::settings::UiSettings;

pub(in crate::firmware::ui) type OverlayTogglesScreen = catalogue_presenter::CatalogueScreen;

pub(in crate::firmware::ui) fn create(
    token: &UiAccessToken,
    catalogue: &DefaultCatalogue,
    settings: &UiSettings,
    lease: &CallbackLease,
) -> Option<OverlayTogglesScreen> {
    catalogue_presenter::create(
        token,
        catalogue,
        settings,
        CatalogueViewKind::OverlayToggles,
        c"Overlay toggles",
        c"Back",
        intent_bridge::BACK_NAVIGATION_INDEX,
        lease,
    )
}
