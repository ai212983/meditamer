#![forbid(unsafe_code)]

//! Hourglass app screen: one `lv_canvas`, painted entirely by hand each
//! frame -- silhouette, throat, and grains -- directly into a packed monochrome pixel
//! buffer this module owns, then handed to LVGL only as "here is a canvas,
//! invalidate it". Screen construction uses the checked adapter; the target
//! runtime owns physics and calls
//! [`Hourglass::render`] with a read-only snapshot.
//!
use render::lvgl_adapter::{
    i1_canvas_buffer_bytes, Align, Font, I1CanvasWriter, MonochromePixel, StaticI1CanvasBuffer,
    StyleState, UiAccessToken, Widget, WidgetDeleteFailure, WidgetKind,
};

use hourglass::presentation::{self, FrameData};
use raster::Surface;

pub const WIDTH: i32 = 140;
pub const HEIGHT: i32 = 200;
const CANVAS_BYTES: usize = i1_canvas_buffer_bytes(WIDTH as usize, HEIGHT as usize);

/// Retained black/white pixels, packed eight per byte with padded rows.
static CANVAS_BUFFER: StaticI1CanvasBuffer<CANVAS_BYTES> =
    StaticI1CanvasBuffer::new(WIDTH as usize, HEIGHT as usize, MonochromePixel::Paper);

pub struct Hourglass {
    root: Widget,
    canvas: Widget,
    remaining: Widget,
    status: Widget,
}

impl Hourglass {
    pub fn activate(&self, token: &UiAccessToken) -> bool {
        self.root.activate(token).unwrap_or(false)
    }

    // Returning `Self` intact lets the caller retry a partial teardown; this
    // is a cold error path, so boxing only to satisfy the lint is unnecessary.
    #[allow(clippy::result_large_err)]
    pub fn destroy(self, token: &UiAccessToken) -> Result<(), Self> {
        match self.root.delete(token) {
            Ok(()) | Err(WidgetDeleteFailure::AlreadyGone) => Ok(()),
            Err(WidgetDeleteFailure::StillValid(root)) => Err(Self { root, ..self }),
        }
    }

    /// Paints one frame from a model snapshot and invalidates the canvas so
    /// LVGL's normal render/flush pipeline picks it up on the next
    /// `lv_timer_handler` pass. Does not touch panel power mode or refresh
    /// cadence -- that is the target runtime's policy, informed by
    /// [`FrameData::state`].
    pub fn render(&self, token: &UiAccessToken, frame: &FrameData<'_>) {
        let _ = CANVAS_BUFFER.with_writer(token, |writer| {
            writer.fill(MonochromePixel::Paper);
            let mut surface = CanvasSurface { writer };
            hourglass::frame::render_frame(&mut surface, frame);
        });

        let mut remaining_text = [0u8; 8];
        presentation::format_remaining(&mut remaining_text, frame.remaining_seconds);
        let status_text = presentation::status_label(frame.state);

        if let Ok(text) = core::ffi::CStr::from_bytes_until_nul(&remaining_text) {
            let _ = self.remaining.set_text(token, text);
        }
        let _ = self.status.set_text(token, status_text);
        let _ = self.canvas.invalidate(token);
    }
}

/// A partially built screen is torn down here rather than leaked; the
/// caller (the target's `SurfaceRuntime::enter`) sees only the `None`
/// outcome.
pub fn create(token: &UiAccessToken) -> Option<Hourglass> {
    let screen = Widget::screen(token).ok()?;
    match build(&screen, token) {
        Some((canvas, remaining, status)) => Some(Hourglass {
            root: screen,
            canvas,
            remaining,
            status,
        }),
        None => {
            if let Err(WidgetDeleteFailure::StillValid(widget)) = screen.delete(token) {
                render::lvgl_adapter::park_orphaned_widget_or_panic(widget);
            }
            None
        }
    }
}

fn build(screen: &Widget, token: &UiAccessToken) -> Option<(Widget, Widget, Widget)> {
    let heading = screen.child(token, WidgetKind::Label).ok()?;
    heading.set_text(token, c"Hourglass").ok()?;
    heading
        .set_text_font(token, Font::Size14, StyleState::Default)
        .ok()?;
    heading.align(token, Align::TopMid, 0, 8).ok()?;

    let canvas = screen.child(token, WidgetKind::Canvas).ok()?;
    canvas.set_i1_canvas_buffer(token, &CANVAS_BUFFER).ok()?;
    canvas.align(token, Align::Center, 0, -15).ok()?;

    let remaining = screen.child(token, WidgetKind::Label).ok()?;
    remaining.set_text(token, c"--:--").ok()?;
    remaining
        .set_text_font(token, Font::Size24, StyleState::Default)
        .ok()?;
    remaining.align(token, Align::BottomMid, 0, -50).ok()?;

    let status = screen.child(token, WidgetKind::Label).ok()?;
    status.set_text(token, c"Ready").ok()?;
    status
        .set_text_font(token, Font::Size14, StyleState::Default)
        .ok()?;
    status.align(token, Align::BottomMid, 0, -27).ok()?;

    let actions = screen.child(token, WidgetKind::Label).ok()?;
    actions
        .set_text(token, c"KEY start/rotate   BOOT back")
        .ok()?;
    actions
        .set_text_font(token, Font::Size14, StyleState::Default)
        .ok()?;
    actions.align(token, Align::BottomMid, 0, -6).ok()?;

    Some((canvas, remaining, status))
}

struct CanvasSurface<'writer, 'buffer> {
    writer: &'writer mut I1CanvasWriter<'buffer, CANVAS_BYTES>,
}

impl Surface for CanvasSurface<'_, '_> {
    fn width(&self) -> i32 {
        WIDTH
    }

    fn height(&self) -> i32 {
        HEIGHT
    }

    fn set(&mut self, x: i32, y: i32, ink: bool) {
        if x < 0 || y < 0 || x >= WIDTH || y >= HEIGHT {
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
