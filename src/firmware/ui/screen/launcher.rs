use super::catalogue_presenter;
use crate::firmware::ui::lvgl::intent_bridge;
use shell::catalogue::{CatalogueViewKind, DefaultCatalogue};
use shell::settings::UiSettings;

pub(in crate::firmware::ui) type LauncherScreen = catalogue_presenter::CatalogueScreen;

pub(in crate::firmware::ui) unsafe fn create(
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
