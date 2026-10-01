#![forbid(unsafe_code)]
//! Launcher screen: Medinote's compiled app launcher, built through the
//! checked LVGL adapter. Renders every catalogue-compiled launchable entry
//! -- not a single hardcoded label -- with one highlighted at a time; the
//! target owns which index is selected and how KEY input advances it.

use core::ffi::CStr;

use render::lvgl_adapter::{
    Align, Font, StyleState, UiAccessToken, Widget, WidgetDeleteFailure, WidgetKind,
};

/// Sized to `MedinoteCatalogue`'s own structural ceiling
/// (`CompiledCatalogue<4>`: Home plus at most 3 apps) -- see
/// `catalogue::MedinoteCatalogue`'s doc.
pub const MAX_ENTRIES: usize = 3;

/// Scratch space for the "> "/"  " selection marker plus one catalogue
/// entry's own name -- generous for any name this compiled catalogue's
/// entries actually carry.
const LABEL_BUFFER_BYTES: usize = 40;

pub struct Launcher {
    root: Widget,
    entries: heapless::Vec<Widget, MAX_ENTRIES>,
}

impl Launcher {
    pub fn activate(&self, token: &UiAccessToken) -> bool {
        self.root.activate(token).unwrap_or(false)
    }

    /// Re-renders every entry's label, marking `selected` with a leading
    /// "> " and leaving the rest unmarked. `labels` must be the same slice
    /// (same order, same length) `create` built this screen from -- the
    /// target's own catalogue view does not change shape between calls.
    pub fn set_selected(
        &self,
        token: &UiAccessToken,
        labels: &[&'static CStr],
        selected: usize,
    ) -> bool {
        let mut ok = true;
        for (index, (entry, label)) in self.entries.iter().zip(labels).enumerate() {
            let mut buffer = [0u8; LABEL_BUFFER_BYTES];
            let len = format_entry_label(&mut buffer, index == selected, label);
            let text = CStr::from_bytes_with_nul(&buffer[..len]).unwrap_or(c"?");
            ok &= entry.set_text(token, text).is_ok();
        }
        ok
    }

    /// Deletes the screen through the adapter's checked-handle contract.
    /// LVGL reporting the object still valid after deletion hands the whole
    /// screen back so the caller can retry, matching `DestroyFailure::Live`
    /// one level up in the target's `SurfaceRuntime::destroy`.
    #[allow(clippy::result_large_err)]
    pub fn destroy(self, token: &UiAccessToken) -> Result<(), Self> {
        match self.root.delete(token) {
            Ok(()) | Err(WidgetDeleteFailure::AlreadyGone) => Ok(()),
            Err(WidgetDeleteFailure::StillValid(root)) => Err(Self { root, ..self }),
        }
    }
}

/// Writes `"> "` (selected) or `"  "` (not) plus `label`'s own text into
/// `buffer`, NUL-terminated, returning the length including the NUL. A name
/// longer than the buffer is truncated rather than rejected -- a cosmetic
/// limit, not a construction failure.
fn format_entry_label(
    buffer: &mut [u8; LABEL_BUFFER_BYTES],
    selected: bool,
    label: &CStr,
) -> usize {
    let prefix: &[u8] = if selected { b"> " } else { b"  " };
    let mut at = 0;
    for &byte in prefix.iter().chain(label.to_bytes().iter()) {
        if at >= buffer.len() - 1 {
            break;
        }
        buffer[at] = byte;
        at += 1;
    }
    buffer[at] = 0;
    at + 1
}

/// Builds one label per `labels` entry (at most [`MAX_ENTRIES`]; the target
/// never compiles more than that), the first marked selected. A partially
/// built screen is torn down here rather than leaked; the caller (the
/// target's `SurfaceRuntime::enter`) sees only the `None` outcome.
pub fn create(token: &UiAccessToken, labels: &[&'static CStr]) -> Option<Launcher> {
    let screen = Widget::screen(token).ok()?;
    match build(&screen, token, labels) {
        Some(entries) => Some(Launcher {
            root: screen,
            entries,
        }),
        None => {
            if let Err(WidgetDeleteFailure::StillValid(widget)) = screen.delete(token) {
                render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
            }
            None
        }
    }
}

fn build(
    screen: &Widget,
    token: &UiAccessToken,
    labels: &[&'static CStr],
) -> Option<heapless::Vec<Widget, MAX_ENTRIES>> {
    let black = render::lvgl_adapter::black();
    let white = render::lvgl_adapter::white();

    let heading = screen.child(token, WidgetKind::Label).ok()?;
    heading.set_text(token, c"Apps").ok()?;
    heading
        .set_text_font(token, Font::Size24, StyleState::Default)
        .ok()?;
    heading.align(token, Align::TopMid, 0, 28).ok()?;

    let card = screen.child(token, WidgetKind::Container).ok()?;
    card.set_size(token, 280, 120).ok()?;
    card.align(token, Align::Center, 0, -5).ok()?;
    card.set_border_width(token, 3, StyleState::Default).ok()?;
    card.set_border_color(token, black, StyleState::Default)
        .ok()?;
    card.set_bg_color(token, white, StyleState::Default).ok()?;
    card.set_radius(token, 8, StyleState::Default).ok()?;

    let mut entries = heapless::Vec::new();
    for (index, label) in labels.iter().take(MAX_ENTRIES).enumerate() {
        let row = card.child(token, WidgetKind::Label).ok()?;
        let mut buffer = [0u8; LABEL_BUFFER_BYTES];
        let len = format_entry_label(&mut buffer, index == 0, label);
        let text = CStr::from_bytes_with_nul(&buffer[..len]).unwrap_or(c"?");
        row.set_text(token, text).ok()?;
        row.set_text_font(token, Font::Size18, StyleState::Default)
            .ok()?;
        row.align(token, Align::TopMid, 0, 12 + (index as i32) * 30)
            .ok()?;
        if entries.push(row).is_err() {
            return None;
        }
    }

    let cycle_hint = screen.child(token, WidgetKind::Label).ok()?;
    cycle_hint.set_text(token, c"KEY tap  next").ok()?;
    cycle_hint
        .set_text_font(token, Font::Size14, StyleState::Default)
        .ok()?;
    cycle_hint.align(token, Align::Center, 0, 70).ok()?;

    let open_hint = screen.child(token, WidgetKind::Label).ok()?;
    open_hint.set_text(token, c"KEY hold  open").ok()?;
    open_hint
        .set_text_font(token, Font::Size14, StyleState::Default)
        .ok()?;
    open_hint.align(token, Align::BottomMid, 0, -24).ok()?;

    let back = screen.child(token, WidgetKind::Label).ok()?;
    back.set_text(token, c"BOOT  back").ok()?;
    back.set_text_font(token, Font::Size14, StyleState::Default)
        .ok()?;
    back.align(token, Align::BottomMid, 0, -6).ok()?;

    Some(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn as_str(buffer: &[u8], len: usize) -> &str {
        core::str::from_utf8(&buffer[..len - 1]).unwrap()
    }

    #[test]
    fn marks_the_selected_entry_and_leaves_others_unmarked() {
        let mut buffer = [0u8; LABEL_BUFFER_BYTES];
        let len = format_entry_label(&mut buffer, true, c"Hourglass");
        assert_eq!(as_str(&buffer, len), "> Hourglass");

        let len = format_entry_label(&mut buffer, false, c"Counter");
        assert_eq!(as_str(&buffer, len), "  Counter");
    }

    #[test]
    fn truncates_a_name_longer_than_the_buffer_instead_of_panicking() {
        let long_name = c"AnAppNameThatIsDeliberatelyMuchLongerThanTheLabelBufferCanHold";
        let mut buffer = [0u8; LABEL_BUFFER_BYTES];
        let len = format_entry_label(&mut buffer, true, long_name);
        assert_eq!(len, LABEL_BUFFER_BYTES);
        assert_eq!(buffer[LABEL_BUFFER_BYTES - 1], 0);
    }
}
