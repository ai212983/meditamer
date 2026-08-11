use super::{catalogue_presenter, intent_bridge};
use crate::firmware::ui::shell::catalogue::{CatalogueViewKind, DefaultCatalogue};
use crate::firmware::ui::shell::settings::UiSettings;

pub(super) type AmbientPickerScreen = catalogue_presenter::CatalogueScreen;

pub(super) unsafe fn create(
    catalogue: &DefaultCatalogue,
    settings: &UiSettings,
    user_data: *mut core::ffi::c_void,
) -> Option<AmbientPickerScreen> {
    unsafe {
        catalogue_presenter::create(
            catalogue,
            settings,
            CatalogueViewKind::AmbientPicker,
            c"Ambient view",
            c"Back",
            intent_bridge::BACK_NAVIGATION_INDEX,
            user_data,
        )
    }
}
