#![forbid(unsafe_code)]
//! Medinote's ambient Home screen, with its root and dynamic labels held as
//! checked, owned widget handles.

use core::ffi::CStr;

use render::lvgl_adapter::{
    Align, Font, StyleState, UiAccessToken, Widget, WidgetDeleteFailure, WidgetKind,
};

use super::{home_caption_font, home_heading_font};

pub struct Home {
    root: Widget,
    heading: Widget,
    caption: Widget,
    clock: Widget,
    reading: Widget,
    status: Widget,
    battery: Widget,
}

impl Home {
    pub fn activate(&self, token: &UiAccessToken) -> bool {
        self.root.activate(token).unwrap_or(false)
    }

    /// Rewrites the top heading/caption pair -- the target's own wake-from-
    /// sleep path briefly relabels these ("Sleep"/"Awake") as a wake
    /// confirmation. Home owns no policy on when this changes; the target
    /// runtime decides that.
    pub fn set_heading(&self, token: &UiAccessToken, heading: &CStr, caption: &CStr) -> bool {
        self.heading.set_text(token, heading).is_ok()
            && self.caption.set_text(token, caption).is_ok()
    }

    /// Rewrites the reading label. No lifetime requirement on `text` beyond
    /// the call -- see `Widget::set_text`.
    pub fn set_reading(&self, token: &UiAccessToken, text: &CStr) -> bool {
        self.reading.set_text(token, text).is_ok()
    }

    pub fn set_status(&self, token: &UiAccessToken, text: &CStr) -> bool {
        self.status.set_text(token, text).is_ok()
    }

    pub fn set_clock(&self, token: &UiAccessToken, text: &CStr) -> bool {
        self.clock.set_text(token, text).is_ok()
    }

    pub fn set_battery(&self, token: &UiAccessToken, text: &CStr) -> bool {
        self.battery.set_text(token, text).is_ok()
    }

    /// Deletes the screen through the adapter's checked-handle contract.
    /// LVGL reporting the object still valid after deletion hands the whole
    /// screen back so the caller can retry, matching `DestroyFailure::Live`
    /// one level up in the target's `SurfaceRuntime::destroy`.
    // `Home`'s six `Widget` handles make this retry-by-value `Err` larger
    // than clippy's default threshold; handing the whole screen back is the
    // deliberate contract every checked-adapter `destroy`/`delete` in this
    // tree already uses (`WidgetDeleteFailure::StillValid`, `DestroyFailure
    // ::Live`), not something worth boxing just to move bytes elsewhere --
    // this path is retry-on-failure, not a hot loop.
    #[allow(clippy::result_large_err)]
    pub fn destroy(self, token: &UiAccessToken) -> Result<(), Self> {
        match self.root.delete(token) {
            Ok(()) | Err(WidgetDeleteFailure::AlreadyGone) => Ok(()),
            Err(WidgetDeleteFailure::StillValid(root)) => Err(Self { root, ..self }),
        }
    }
}

/// Builds a screen from what a shell provider registered, so the surface on
/// the glass is the one the registry resolved rather than a hardcoded
/// layout. `title` and `subtitle` are provider-owned content.
///
/// A partially built screen is torn down here rather than leaked; the
/// caller (the target's `SurfaceRuntime::enter`) sees only the `None`
/// outcome.
pub fn create(
    token: &UiAccessToken,
    title: &'static CStr,
    subtitle: &'static CStr,
) -> Option<Home> {
    let screen = Widget::screen(token).ok()?;
    match build(&screen, token, title, subtitle) {
        Some((heading, caption, clock, reading, status, battery)) => Some(Home {
            root: screen,
            heading,
            caption,
            clock,
            reading,
            status,
            battery,
        }),
        None => {
            if let Err(WidgetDeleteFailure::StillValid(widget)) = screen.delete(token) {
                render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
            }
            None
        }
    }
}

#[allow(clippy::type_complexity)]
fn build(
    screen: &Widget,
    token: &UiAccessToken,
    title: &'static CStr,
    subtitle: &'static CStr,
) -> Option<(Widget, Widget, Widget, Widget, Widget, Widget)> {
    screen
        .set_bg_color(token, render::lvgl_adapter::white(), StyleState::Default)
        .ok()?;

    // Ark Pixel, proportional Latin build (`assets/fonts/ArkPixel-OFL.txt`).
    // Heading at 16px, caption at 12px -- see `home_heading_font`/
    // `home_caption_font` for the compiled tables and `build.rs` for the
    // source TTFs.
    let heading = screen.child(token, WidgetKind::Label).ok()?;
    heading.set_text(token, title).ok()?;
    heading
        .set_text_font(
            token,
            Font::custom(home_heading_font::font()),
            StyleState::Default,
        )
        .ok()?;
    heading.align(token, Align::TopMid, 0, 10).ok()?;

    let caption = screen.child(token, WidgetKind::Label).ok()?;
    caption.set_text(token, subtitle).ok()?;
    caption
        .set_text_font(
            token,
            Font::custom(home_caption_font::font()),
            StyleState::Default,
        )
        .ok()?;
    caption.align(token, Align::TopMid, 0, 28).ok()?;

    // Largest face this build carries (lv_conf.h enables 14/18/24 only). A
    // wall clock is the one thing on this screen worth the extra size.
    let clock = screen.child(token, WidgetKind::Label).ok()?;
    clock.set_text(token, c"--:--").ok()?;
    clock
        .set_text_font(token, Font::Size24, StyleState::Default)
        .ok()?;
    clock.align(token, Align::TopMid, 0, 55).ok()?;

    let panel = screen.child(token, WidgetKind::Container).ok()?;
    panel.set_size(token, 280, 110).ok()?;
    panel.align(token, Align::Center, 0, 25).ok()?;
    panel.set_border_width(token, 3, StyleState::Default).ok()?;
    panel
        .set_border_color(token, render::lvgl_adapter::black(), StyleState::Default)
        .ok()?;
    panel
        .set_bg_color(token, render::lvgl_adapter::white(), StyleState::Default)
        .ok()?;
    panel.set_radius(token, 8, StyleState::Default).ok()?;

    let reading = panel.child(token, WidgetKind::Label).ok()?;
    reading.set_text(token, c"--.- C   --.- %").ok()?;
    reading
        .set_text_font(token, Font::Size18, StyleState::Default)
        .ok()?;
    reading.align(token, Align::Center, 0, -14).ok()?;

    let status = panel.child(token, WidgetKind::Label).ok()?;
    status.set_text(token, c"waiting for sensor").ok()?;
    status
        .set_text_font(token, Font::Size14, StyleState::Default)
        .ok()?;
    status.align(token, Align::Center, 0, 18).ok()?;

    let battery = screen.child(token, WidgetKind::Label).ok()?;
    battery.set_text(token, c"battery --").ok()?;
    battery
        .set_text_font(token, Font::Size14, StyleState::Default)
        .ok()?;
    battery.align(token, Align::BottomMid, 0, -30).ok()?;

    let actions = screen.child(token, WidgetKind::Label).ok()?;
    actions.set_text(token, c"KEY apps     BOOT sleep").ok()?;
    actions
        .set_text_font(token, Font::Size14, StyleState::Default)
        .ok()?;
    actions.align(token, Align::BottomMid, 0, -8).ok()?;

    Some((heading, caption, clock, reading, status, battery))
}
