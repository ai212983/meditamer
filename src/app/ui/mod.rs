#![allow(dead_code)]

use super::types::InkplateDriver;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DirtyRegion {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: i32,
    pub(crate) height: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UiRenderResult {
    Disabled,
    NotImplemented,
}

pub(crate) fn render_ui_full(_display: &mut InkplateDriver) -> UiRenderResult {
    if cfg!(feature = "ui-hybrid") {
        UiRenderResult::NotImplemented
    } else {
        UiRenderResult::Disabled
    }
}

pub(crate) fn render_ui_dirty(
    _display: &mut InkplateDriver,
    _dirty: DirtyRegion,
) -> UiRenderResult {
    if cfg!(feature = "ui-hybrid") {
        UiRenderResult::NotImplemented
    } else {
        UiRenderResult::Disabled
    }
}
