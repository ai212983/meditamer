#![forbid(unsafe_code)]
//! Checked-LVGL presentation for Medinote's KEY-driven Counter app.
//!
//! Counter doubles as the on-glass harness for the `flipclock` crate: one KEY
//! press runs a whole flip, animated from the target runtime's tick. Speed and
//! easing are `flipclock`'s `FLIP_DURATION_MS` and `FLIP_EASING`.

use core::ffi::CStr;

use render::lvgl_adapter::{
    i1_canvas_buffer_bytes, Align, Font, I1CanvasWriter, MonochromePixel, StaticI1CanvasBuffer,
    StyleState, UiAccessToken, Widget, WidgetDeleteFailure, WidgetKind,
};

use flipclock::flap;
use flipclock::{card, housing, FlipClock, SURFACE_H, SURFACE_W};
use raster::Surface;

const CANVAS_BYTES: usize = i1_canvas_buffer_bytes(SURFACE_W as usize, SURFACE_H as usize);

/// Retained black/white pixels, packed eight per byte.
static CANVAS_BUFFER: StaticI1CanvasBuffer<CANVAS_BYTES> = StaticI1CanvasBuffer::new(
    SURFACE_W as usize,
    SURFACE_H as usize,
    MonochromePixel::Paper,
);

/// Bridges the renderer's [`Surface`] onto the retained canvas bytes.
///
/// The `flipclock` module names no LVGL type, so the adapter lives here, on the
/// screen that owns the canvas, rather than travelling with the renderer.
struct CanvasSurface<'a, 'w> {
    writer: &'a mut I1CanvasWriter<'w, CANVAS_BYTES>,
}

impl Surface for CanvasSurface<'_, '_> {
    fn width(&self) -> i32 {
        SURFACE_W
    }

    fn height(&self) -> i32 {
        SURFACE_H
    }

    fn set(&mut self, x: i32, y: i32, ink: bool) {
        if x < 0 || y < 0 || x >= SURFACE_W || y >= SURFACE_H {
            return;
        }
        let _ = self.writer.set(
            x as usize,
            y as usize,
            if ink {
                MonochromePixel::Ink
            } else {
                MonochromePixel::Paper
            },
        );
    }
}

pub struct Counter {
    root: Widget,
    count: Widget,
    flip: Widget,
}

impl Counter {
    pub fn activate(&self, token: &UiAccessToken) -> bool {
        self.root.activate(token).unwrap_or(false)
    }

    /// Rewrites the displayed count. No lifetime requirement on `text`
    /// beyond the call -- see `Widget::set_text`.
    pub fn set_count(&self, token: &UiAccessToken, text: &CStr) -> bool {
        self.count.set_text(token, text).is_ok()
    }

    /// Repaints the flip card for `step` and invalidates the canvas so LVGL's
    /// normal render/flush pipeline picks it up on the next `lv_timer_handler`
    /// pass. Touches no panel power mode or refresh cadence -- that stays the
    /// target runtime's policy.
    pub fn set_flip(&self, token: &UiAccessToken, clock: &FlipClock) {
        paint(token, clock);
        let _ = self.flip.invalidate(token);
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

fn paint(token: &UiAccessToken, clock: &FlipClock) {
    let geometry = card();
    let _ = CANVAS_BUFFER.with_writer(token, |writer| {
        let mut surface = CanvasSurface { writer };
        housing::draw_ground(&mut surface, &geometry, Default::default());
        flap::render_frame(
            &mut surface,
            &geometry,
            clock.pose(),
            &clock.faces(),
            Default::default(),
        );
        housing::draw_rails(&mut surface, &geometry, Default::default());
    });
}

/// A partially built screen is torn down here rather than leaked; the
/// caller (the target's `SurfaceRuntime::enter`) sees only the `None`
/// outcome.
pub fn create(token: &UiAccessToken, initial_count: &CStr, clock: &FlipClock) -> Option<Counter> {
    let screen = Widget::screen(token).ok()?;
    match build(&screen, token, initial_count) {
        Some((count, flip)) => {
            let counter = Counter {
                root: screen,
                count,
                flip,
            };
            counter.set_flip(token, clock);
            Some(counter)
        }
        None => {
            if let Err(WidgetDeleteFailure::StillValid(widget)) = screen.delete(token) {
                render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
            }
            None
        }
    }
}

fn build(screen: &Widget, token: &UiAccessToken, initial_count: &CStr) -> Option<(Widget, Widget)> {
    let white = render::lvgl_adapter::white();
    screen
        .set_bg_color(token, white, StyleState::Default)
        .ok()?;

    let heading = screen.child(token, WidgetKind::Label).ok()?;
    heading.set_text(token, c"Flip").ok()?;
    heading
        .set_text_font(token, Font::Size18, StyleState::Default)
        .ok()?;
    heading.align(token, Align::TopMid, 0, 8).ok()?;

    let flip = screen.child(token, WidgetKind::Canvas).ok()?;
    flip.set_i1_canvas_buffer(token, &CANVAS_BUFFER).ok()?;
    flip.align(token, Align::Center, 0, -8).ok()?;

    let count = screen.child(token, WidgetKind::Label).ok()?;
    count.set_text(token, initial_count).ok()?;
    count
        .set_text_font(token, Font::Size14, StyleState::Default)
        .ok()?;
    count.align(token, Align::BottomMid, 0, -24).ok()?;

    let actions = screen.child(token, WidgetKind::Label).ok()?;
    actions.set_text(token, c"KEY step   BOOT back").ok()?;
    actions
        .set_text_font(token, Font::Size14, StyleState::Default)
        .ok()?;
    actions.align(token, Align::BottomMid, 0, -6).ok()?;

    Some((count, flip))
}
