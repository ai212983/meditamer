#![forbid(unsafe_code)]

use super::catalogue_presenter;
use render::intent_bridge::{self, CallbackLease};
use render::lvgl_adapter::UiAccessToken;
use shell::catalogue::{CatalogueViewKind, DefaultCatalogue};
use shell::settings::UiSettings;

pub(in crate::firmware::ui) type LauncherScreen = catalogue_presenter::CatalogueScreen;

pub(in crate::firmware::ui) fn create(
    token: &UiAccessToken,
    catalogue: &DefaultCatalogue,
    settings: &UiSettings,
    lease: &CallbackLease,
) -> Option<LauncherScreen> {
    catalogue_presenter::create(
        token,
        catalogue,
        settings,
        CatalogueViewKind::Launcher,
        c"Launcher",
        c"Home",
        intent_bridge::HOME_NAVIGATION_INDEX,
        lease,
    )
}
