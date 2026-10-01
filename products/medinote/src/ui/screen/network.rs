use core::ffi::CStr;
use render::lvgl_adapter::{
    Align, Font, StyleState, UiAccessToken, Widget, WidgetDeleteFailure, WidgetKind,
};

pub struct Network {
    root: Widget,
    status: Widget,
}

impl Network {
    pub fn activate(&self, token: &UiAccessToken) -> bool {
        self.root.activate(token).unwrap_or(false)
    }
    pub fn set_status(&self, token: &UiAccessToken, status: &CStr) -> bool {
        self.status.set_text(token, status).is_ok()
    }
    pub fn destroy(self, token: &UiAccessToken) -> Result<(), Self> {
        match self.root.delete(token) {
            Ok(()) | Err(WidgetDeleteFailure::AlreadyGone) => Ok(()),
            Err(WidgetDeleteFailure::StillValid(root)) => Err(Self { root, ..self }),
        }
    }
}

pub fn create(token: &UiAccessToken, status: &CStr) -> Option<Network> {
    let root = Widget::screen(token).ok()?;
    match build(&root, token, status) {
        Some(status) => Some(Network { root, status }),
        None => {
            if let Err(WidgetDeleteFailure::StillValid(root)) = root.delete(token) {
                render::lvgl_adapter::park_orphaned_widget_or_panic(root);
            }
            None
        }
    }
}

fn build(root: &Widget, token: &UiAccessToken, status: &CStr) -> Option<Widget> {
    let heading = root.child(token, WidgetKind::Label).ok()?;
    heading.set_text(token, c"Wi-Fi").ok()?;
    heading
        .set_text_font(token, Font::Size18, StyleState::Default)
        .ok()?;
    heading.align(token, Align::TopMid, 0, 12).ok()?;
    let status_widget = root.child(token, WidgetKind::Label).ok()?;
    status_widget.set_text(token, status).ok()?;
    status_widget.align(token, Align::Center, 0, 0).ok()?;
    let action = root.child(token, WidgetKind::Label).ok()?;
    action.set_text(token, c"KEY: toggle").ok()?;
    action.align(token, Align::BottomMid, 0, -14).ok()?;
    Some(status_widget)
}
