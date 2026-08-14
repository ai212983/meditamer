use super::catalogue_presenter;
use crate::firmware::ui::lvgl::intent_bridge;
use crate::firmware::ui::shell::catalogue::{CatalogueViewKind, DefaultCatalogue};
use crate::firmware::ui::shell::settings::UiSettings;

pub(in crate::firmware::ui) type OverlaySettingsScreen = catalogue_presenter::CatalogueScreen;

pub(in crate::firmware::ui) unsafe fn create(
    catalogue: &DefaultCatalogue,
    settings: &UiSettings,
    user_data: *mut core::ffi::c_void,
) -> Option<OverlaySettingsScreen> {
    unsafe {
        catalogue_presenter::create(
            catalogue,
            settings,
            CatalogueViewKind::OverlaySettings,
            c"Overlay settings",
            c"Back",
            intent_bridge::BACK_NAVIGATION_INDEX,
            user_data,
        )
    }
}
