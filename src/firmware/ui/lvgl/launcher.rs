use super::{catalogue_presenter, intent_bridge};
use crate::firmware::ui::shell::catalogue::{CatalogueViewKind, DefaultCatalogue};
use crate::firmware::ui::shell::settings::UiSettings;

pub(super) type LauncherScreen = catalogue_presenter::CatalogueScreen;

pub(super) unsafe fn create(
    catalogue: &DefaultCatalogue,
    settings: &UiSettings,
    user_data: *mut core::ffi::c_void,
) -> Option<LauncherScreen> {
    unsafe {
        catalogue_presenter::create(
            catalogue,
            settings,
            CatalogueViewKind::Launcher,
            c"Launcher",
            c"Home",
            intent_bridge::HOME_NAVIGATION_INDEX,
            user_data,
        )
    }
}
