//! Product and base-resident screens (full-panel surfaces). Built on the LVGL
//! toolkit adapter in [`super::lvgl`]; owned/composed by [`super::lvgl::backend`].

pub(in crate::firmware::ui) mod ambient_picker;
mod catalogue_presenter;
pub(in crate::firmware::ui) mod gesture_test;
pub(in crate::firmware::ui) mod home;
pub(in crate::firmware::ui) mod launcher;
pub(in crate::firmware::ui) mod overlay_settings;
#[cfg(feature = "ui-provider-fixture")]
pub(in crate::firmware::ui) mod provider_fixture;
