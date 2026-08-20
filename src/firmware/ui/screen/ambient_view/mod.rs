//! Ambient Home prototype surface (`docs/plans/ambient-home-prototype.md`):
//! a static bezier arc, a circle positioned by local time-of-day, and an
//! optional tap-to-show-time display that carries the Back button.
//!
//! Wall-clock time comes from the serial task -- the sole owner of RTC I2C
//! access (`docs/plans/wall-clock-sync.md`) -- over the
//! `WALL_CLOCK_REQUESTS`/`WALL_CLOCK_RESPONSES` channel pair
//! (`crate::firmware::display::wall_clock`). This module never caches a
//! clock reading across polls; [`AmbientViewScreen::poll`] only decides
//! *when* a fresh read is due, using the monotonic tick clock, and every
//! application uses whatever snapshot was just fetched.

mod clock_font;
mod model;

use core::fmt::Write as _;
use core::ptr;

use heapless::String;
use lightvgl_sys as lv;

use render::intent_bridge;
use model::AmbientHomeConfig;

const STYLE_DEFAULT: lv::lv_style_selector_t = 0;
const STYLE_PRESSED: lv::lv_style_selector_t = lv::lv_state_t_LV_STATE_PRESSED;

// Duplicated from `crate::firmware::ui::lvgl` (private there, and this
// screen cannot see across the `lvgl`/`screen` module split): the fixed
// panel surface size used to resolve the plan's fractional geometry.
const SURFACE_WIDTH: f32 = 600.0;
const SURFACE_HEIGHT: f32 = 600.0;

const ARC_SAMPLE_COUNT: usize = 48;
const TIME_DISPLAY_DURATION_MS: u64 = 10_000;
/// Retry cadence while wall-clock time is unavailable (or a request
/// timed out). Bounds "time becomes available" latency without polling
/// every frame tick.
const UNAVAILABLE_RETRY_INTERVAL_MS: u64 = 5_000;

static mut ARC_POINTS: [lv::lv_point_precise_t; ARC_SAMPLE_COUNT] =
    [lv::lv_point_precise_t { x: 0.0, y: 0.0 }; ARC_SAMPLE_COUNT];

/// Rounds to the nearest integer (half away from zero). `core` has no
/// `f32::round` without `std`/`libm`, so this is the manual equivalent for
/// the handful of pixel-coordinate conversions this screen needs.
fn round_to_i32(value: f32) -> i32 {
    if value >= 0.0 {
        (value + 0.5) as i32
    } else {
        (value - 0.5) as i32
    }
}

/// What the next Ambient Home tick should do. Returned by the cheap,
/// monotonic-clock-only [`AmbientViewScreen::poll`]/[`AmbientViewScreen::handle_tap`]
/// so the async wall-clock fetch (crossing to the serial task) only happens
/// when actually due.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AmbientHomeAction {
    None,
    /// A scheduled update boundary (or first entry) is due.
    FetchForUpdate,
    /// A tap requested (or refreshed) the time-on-tap display.
    FetchForShow,
    /// The 10-second time-on-tap window elapsed; fetch fresh time to resume.
    FetchForReturn,
}

/// Whether the Back button is reachable from the undisturbed ambient view.
///
/// The plan puts Back on the time display, which a surface tap reveals. That
/// leaves no way off the surface when tap-to-show-time is configured off, so
/// the button then stays on the ambient view instead.
fn back_button_starts_visible(config: &AmbientHomeConfig) -> bool {
    !config.tap_to_show_time
}

enum Mode {
    Ambient {
        next_check_due_ms: u64,
        last_fraction: Option<f32>,
    },
    TimeDisplay {
        shown_at_ms: u64,
    },
}

pub(in crate::firmware::ui) struct AmbientViewScreen {
    root: *mut lv::lv_obj_t,
    line: *mut lv::lv_obj_t,
    circle: *mut lv::lv_obj_t,
    time_label: *mut lv::lv_obj_t,
    back_button: *mut lv::lv_obj_t,
    mode: Mode,
}

/// This prototype has no runtime-configurable state yet (see the plan's
/// "Configuration" section and `docs/architecture/0013-compiled-only-ui-catalogue.md`
/// on keeping screen config out of the persisted-settings machinery until
/// there is a concrete product need for it), so the resolved config is
/// recomputed on demand from the compiled-in default rather than stored --
/// keeping [`AmbientViewScreen`] (embedded in every navigation's
/// `ActiveSurface`/`DestroyFailure` paths) small. `validated()` is
/// effectively free here since [`AmbientHomeConfig::DEFAULT`] is always
/// valid.
fn config() -> AmbientHomeConfig {
    AmbientHomeConfig::DEFAULT.validated()
}

/// The arc's 4 pixel-space Bezier control points for `config`, resolved
/// against the fixed panel surface size. Recomputed on demand (a handful of
/// multiplications) rather than cached in [`AmbientViewScreen`].
fn curve_points(config: &AmbientHomeConfig) -> [model::PixelPoint; 4] {
    [
        model::to_pixels(config.arc_start, SURFACE_WIDTH, SURFACE_HEIGHT),
        model::to_pixels(config.control_1, SURFACE_WIDTH, SURFACE_HEIGHT),
        model::to_pixels(config.control_2, SURFACE_WIDTH, SURFACE_HEIGHT),
        model::to_pixels(config.arc_end, SURFACE_WIDTH, SURFACE_HEIGHT),
    ]
}

impl AmbientViewScreen {
    pub(in crate::firmware::ui) fn root(&self) -> *mut lv::lv_obj_t {
        self.root
    }

    /// Cheap, monotonic-clock-only check: does anything need a fresh
    /// wall-clock read right now? Never touches RTC/I2C itself.
    pub(in crate::firmware::ui) fn poll(&self, now_ms: u64) -> AmbientHomeAction {
        match self.mode {
            Mode::TimeDisplay { shown_at_ms } => {
                if now_ms.saturating_sub(shown_at_ms) >= TIME_DISPLAY_DURATION_MS {
                    AmbientHomeAction::FetchForReturn
                } else {
                    AmbientHomeAction::None
                }
            }
            Mode::Ambient {
                next_check_due_ms, ..
            } => {
                if now_ms >= next_check_due_ms {
                    AmbientHomeAction::FetchForUpdate
                } else {
                    AmbientHomeAction::None
                }
            }
        }
    }

    /// A tap landed on the screen background (outside the Back button).
    pub(in crate::firmware::ui) fn handle_tap(&self) -> AmbientHomeAction {
        if config().tap_to_show_time {
            AmbientHomeAction::FetchForShow
        } else {
            // "When the option is disabled, surface taps retain the ambient
            // view."
            AmbientHomeAction::None
        }
    }

    /// Applies a wall-clock query outcome for the given `action`. Returns
    /// `true` if the visible surface changed and needs a full-screen
    /// repaint.
    pub(in crate::firmware::ui) unsafe fn apply(
        &mut self,
        action: AmbientHomeAction,
        snapshot: Option<rtc::driver::WallClockSnapshot>,
        now_ms: u64,
    ) -> bool {
        let snapshot = snapshot.filter(|snapshot| snapshot.valid);
        match action {
            AmbientHomeAction::None => false,
            AmbientHomeAction::FetchForUpdate => unsafe {
                self.apply_ambient_update(snapshot, now_ms)
            },
            AmbientHomeAction::FetchForShow => unsafe { self.apply_show_time(snapshot, now_ms) },
            AmbientHomeAction::FetchForReturn => unsafe {
                self.apply_return_to_ambient(snapshot, now_ms)
            },
        }
    }

    unsafe fn apply_ambient_update(
        &mut self,
        snapshot: Option<rtc::driver::WallClockSnapshot>,
        now_ms: u64,
    ) -> bool {
        let Mode::Ambient { last_fraction, .. } = self.mode else {
            // A stale response arriving after a tap already switched modes.
            return false;
        };
        let Some(snapshot) = snapshot else {
            let changed = last_fraction.is_some();
            if changed {
                unsafe { self.set_circle_visible(false) };
            }
            self.mode = Mode::Ambient {
                next_check_due_ms: now_ms + UNAVAILABLE_RETRY_INTERVAL_MS,
                last_fraction: None,
            };
            return changed;
        };

        let config = config();
        let seconds_of_day = model::local_seconds_of_day(snapshot.local_epoch_seconds);
        let fraction = model::journey_fraction(&config, seconds_of_day);
        let changed = last_fraction != Some(fraction);
        if changed {
            unsafe {
                self.position_circle(&config, fraction);
                self.set_circle_visible(true);
            }
        }
        let delay_seconds = model::seconds_until_next_boundary(&config, seconds_of_day);
        self.mode = Mode::Ambient {
            next_check_due_ms: now_ms + u64::from(delay_seconds) * 1_000,
            last_fraction: Some(fraction),
        };
        changed
    }

    unsafe fn apply_show_time(
        &mut self,
        snapshot: Option<rtc::driver::WallClockSnapshot>,
        now_ms: u64,
    ) -> bool {
        let Some(snapshot) = snapshot else {
            // "A tap with unavailable local time retains the ambient view."
            // Also covers a failed refresh-tap while already showing time:
            // the display keeps counting down toward its existing deadline.
            return false;
        };
        let seconds_of_day = model::local_seconds_of_day(snapshot.local_epoch_seconds);
        unsafe {
            self.render_time_label(seconds_of_day);
            self.set_ambient_visible(false);
            self.set_time_label_visible(true);
            self.set_back_visible(true);
        }
        self.mode = Mode::TimeDisplay {
            shown_at_ms: now_ms,
        };
        true
    }

    unsafe fn apply_return_to_ambient(
        &mut self,
        snapshot: Option<rtc::driver::WallClockSnapshot>,
        now_ms: u64,
    ) -> bool {
        unsafe {
            self.set_time_label_visible(false);
            self.set_back_visible(back_button_starts_visible(&config()));
            self.set_ambient_visible(true);
        }
        self.mode = match snapshot {
            Some(snapshot) => {
                let config = config();
                let seconds_of_day = model::local_seconds_of_day(snapshot.local_epoch_seconds);
                let fraction = model::journey_fraction(&config, seconds_of_day);
                unsafe {
                    self.position_circle(&config, fraction);
                    self.set_circle_visible(true);
                }
                let delay_seconds = model::seconds_until_next_boundary(&config, seconds_of_day);
                Mode::Ambient {
                    next_check_due_ms: now_ms + u64::from(delay_seconds) * 1_000,
                    last_fraction: Some(fraction),
                }
            }
            None => {
                unsafe { self.set_circle_visible(false) };
                Mode::Ambient {
                    next_check_due_ms: now_ms + UNAVAILABLE_RETRY_INTERVAL_MS,
                    last_fraction: None,
                }
            }
        };
        true
    }

    unsafe fn position_circle(&self, config: &AmbientHomeConfig, fraction: f32) {
        let [p0, p1, p2, p3] = curve_points(config);
        let point = model::point_on_curve(p0, p1, p2, p3, fraction);
        let radius = circle_radius_px(config);
        unsafe {
            lv::lv_obj_set_pos(
                self.circle,
                round_to_i32(point.x - radius),
                round_to_i32(point.y - radius),
            );
        }
    }

    unsafe fn set_circle_visible(&self, visible: bool) {
        unsafe { set_hidden(self.circle, !visible) };
    }

    unsafe fn set_ambient_visible(&self, visible: bool) {
        unsafe {
            set_hidden(self.line, !visible);
            if !visible {
                // Circle visibility is otherwise driven by time
                // availability; force it off whenever the ambient view
                // itself is hidden (entering time-display mode).
                set_hidden(self.circle, true);
            }
        }
    }

    unsafe fn set_time_label_visible(&self, visible: bool) {
        unsafe { set_hidden(self.time_label, !visible) };
    }

    unsafe fn set_back_visible(&self, visible: bool) {
        unsafe { set_hidden(self.back_button, !visible) };
    }

    unsafe fn render_time_label(&self, seconds_of_day: u32) {
        let hours = seconds_of_day / 3_600;
        let minutes = (seconds_of_day % 3_600) / 60;
        let mut text = String::<8>::new();
        let _ = write!(text, "{hours:02}:{minutes:02}");
        if text.push('\0').is_err() {
            return;
        }
        unsafe {
            lv::lv_label_set_text(self.time_label, text.as_ptr().cast());
            lv::lv_obj_center(self.time_label);
        }
    }
}

fn circle_radius_px(config: &AmbientHomeConfig) -> f32 {
    config.circle_radius_fraction * SURFACE_WIDTH.min(SURFACE_HEIGHT)
}

unsafe fn set_hidden(obj: *mut lv::lv_obj_t, hidden: bool) {
    if obj.is_null() {
        return;
    }
    unsafe {
        if hidden {
            lv::lv_obj_add_flag(obj, lv::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
        } else {
            lv::lv_obj_remove_flag(obj, lv::lv_obj_flag_t_LV_OBJ_FLAG_HIDDEN);
        }
    }
}

pub(in crate::firmware::ui) unsafe fn create(
    user_data: *mut core::ffi::c_void,
) -> Option<AmbientViewScreen> {
    let screen = unsafe { lv::lv_obj_create(ptr::null_mut()) };
    if screen.is_null() {
        return None;
    }
    let black = unsafe { lv::lv_color_black() };
    let white = unsafe { lv::lv_color_white() };
    unsafe {
        lv::lv_obj_set_style_bg_color(screen, white, STYLE_DEFAULT);
        lv::lv_obj_set_style_bg_opa(screen, 255, STYLE_DEFAULT);
        lv::lv_obj_set_style_text_color(screen, black, STYLE_DEFAULT);
    }

    let config = config();
    let curve = curve_points(&config);

    let Some(line) = (unsafe { create_arc_line(screen, black, curve) }) else {
        unsafe { lv::lv_obj_delete(screen) };
        return None;
    };
    let Some(circle) = (unsafe { create_circle(screen, black, white, &config) }) else {
        unsafe { lv::lv_obj_delete(screen) };
        return None;
    };
    let Some(time_label) = (unsafe { create_time_label(screen, black) }) else {
        unsafe { lv::lv_obj_delete(screen) };
        return None;
    };
    // Time is unknown until the first poll fetches a snapshot; start with
    // the circle hidden, matching "the surface shows the arc alone" while
    // time is unavailable.
    unsafe { set_hidden(circle, true) };

    let Some(back_button) = (unsafe { create_back_button(screen, user_data, black, white) }) else {
        unsafe { lv::lv_obj_delete(screen) };
        return None;
    };
    // Back rides with the time display, which a tap reveals.
    unsafe { set_hidden(back_button, !back_button_starts_visible(&config)) };

    if !unsafe { install_tap_handler(screen) } {
        unsafe { lv::lv_obj_delete(screen) };
        return None;
    }

    Some(AmbientViewScreen {
        root: screen,
        line,
        circle,
        time_label,
        back_button,
        mode: Mode::Ambient {
            // Due immediately: "entering Ambient Home immediately shows the
            // current position" when local time is available.
            next_check_due_ms: 0,
            last_fraction: None,
        },
    })
}

unsafe fn create_arc_line(
    screen: *mut lv::lv_obj_t,
    black: lv::lv_color_t,
    curve: [model::PixelPoint; 4],
) -> Option<*mut lv::lv_obj_t> {
    let line = unsafe { lv::lv_line_create(screen) };
    if line.is_null() {
        return None;
    }
    let [p0, p1, p2, p3] = curve;
    let mut samples = [model::PixelPoint { x: 0.0, y: 0.0 }; ARC_SAMPLE_COUNT];
    model::sample_curve(p0, p1, p2, p3, &mut samples);
    unsafe {
        let points = &mut *ptr::addr_of_mut!(ARC_POINTS);
        for (slot, sample) in points.iter_mut().zip(samples.iter()) {
            *slot = lv::lv_point_precise_t {
                x: sample.x,
                y: sample.y,
            };
        }
        lv::lv_line_set_points(
            line,
            ptr::addr_of!(ARC_POINTS).cast(),
            ARC_SAMPLE_COUNT as u32,
        );
        lv::lv_obj_set_style_line_color(line, black, STYLE_DEFAULT);
        lv::lv_obj_set_style_line_width(line, 4, STYLE_DEFAULT);
        lv::lv_obj_set_style_line_rounded(line, true, STYLE_DEFAULT);
        lv::lv_obj_remove_flag(line, lv::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE);
    }
    Some(line)
}

unsafe fn create_circle(
    screen: *mut lv::lv_obj_t,
    black: lv::lv_color_t,
    white: lv::lv_color_t,
    config: &AmbientHomeConfig,
) -> Option<*mut lv::lv_obj_t> {
    let circle = unsafe { lv::lv_obj_create(screen) };
    if circle.is_null() {
        return None;
    }
    let diameter = round_to_i32(circle_radius_px(config) * 2.0);
    unsafe {
        lv::lv_obj_remove_style_all(circle);
        lv::lv_obj_set_size(circle, diameter, diameter);
        lv::lv_obj_set_style_radius(circle, lv::LV_RADIUS_CIRCLE as i32, STYLE_DEFAULT);
        lv::lv_obj_set_style_bg_color(circle, white, STYLE_DEFAULT);
        lv::lv_obj_set_style_bg_opa(circle, 255, STYLE_DEFAULT);
        lv::lv_obj_set_style_border_color(circle, black, STYLE_DEFAULT);
        lv::lv_obj_set_style_border_width(circle, 4, STYLE_DEFAULT);
        lv::lv_obj_remove_flag(circle, lv::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE);
    }
    Some(circle)
}

unsafe fn create_time_label(
    screen: *mut lv::lv_obj_t,
    black: lv::lv_color_t,
) -> Option<*mut lv::lv_obj_t> {
    let label = unsafe { lv::lv_label_create(screen) };
    if label.is_null() {
        return None;
    }
    unsafe {
        lv::lv_obj_set_style_text_font(label, clock_font::font(), STYLE_DEFAULT);
        lv::lv_obj_set_style_text_color(label, black, STYLE_DEFAULT);
        lv::lv_label_set_text(label, c"--:--".as_ptr());
        lv::lv_obj_center(label);
        lv::lv_obj_remove_flag(label, lv::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE);
    }
    Some(label)
}

unsafe fn install_tap_handler(screen: *mut lv::lv_obj_t) -> bool {
    // Tap-to-show-time is purely local screen state (no shell navigation or
    // overlay side effects), so this bypasses the `IntentBindings` route
    // system and just flags an atomic, mirroring
    // `intent_bridge::mark_full_repaint_requested`'s "flag it happened"
    // idiom. The Back button below keeps its own route-backed callback
    // since it does drive navigation.
    !unsafe {
        lv::lv_obj_add_event_cb(
            screen,
            Some(intent_bridge::ambient_tap_callback),
            lv::lv_event_code_t_LV_EVENT_CLICKED,
            ptr::null_mut(),
        )
    }
    .is_null()
}

unsafe fn create_back_button(
    screen: *mut lv::lv_obj_t,
    user_data: *mut core::ffi::c_void,
    black: lv::lv_color_t,
    white: lv::lv_color_t,
) -> Option<*mut lv::lv_obj_t> {
    let button = unsafe { lv::lv_button_create(screen) };
    if button.is_null() {
        return None;
    }
    unsafe {
        lv::lv_obj_remove_style_all(button);
        lv::lv_obj_set_size(button, 180, 56);
        lv::lv_obj_set_pos(button, 210, 522);
        lv::lv_obj_set_ext_click_area(button, 16);
        lv::lv_obj_set_style_bg_color(button, white, STYLE_DEFAULT);
        lv::lv_obj_set_style_bg_opa(button, 255, STYLE_DEFAULT);
        lv::lv_obj_set_style_text_color(button, black, STYLE_DEFAULT);
        lv::lv_obj_set_style_border_color(button, black, STYLE_DEFAULT);
        lv::lv_obj_set_style_border_width(button, 3, STYLE_DEFAULT);
        lv::lv_obj_set_style_radius(button, 8, STYLE_DEFAULT);
        lv::lv_obj_set_style_bg_color(button, black, STYLE_PRESSED);
        lv::lv_obj_set_style_bg_opa(button, 255, STYLE_PRESSED);
        lv::lv_obj_set_style_text_color(button, white, STYLE_PRESSED);
        lv::lv_obj_set_user_data(
            button,
            intent_bridge::action_user_data(intent_bridge::BACK_NAVIGATION_INDEX),
        );
        if lv::lv_obj_add_event_cb(
            button,
            Some(intent_bridge::navigation_callback),
            lv::lv_event_code_t_LV_EVENT_CLICKED,
            user_data,
        )
        .is_null()
        {
            return None;
        }
    }

    let label = unsafe { lv::lv_label_create(button) };
    if label.is_null() {
        return None;
    }
    unsafe {
        lv::lv_label_set_text(label, c"Back".as_ptr());
        lv::lv_obj_set_style_text_font(
            label,
            ptr::addr_of!(lv::lv_font_montserrat_18),
            STYLE_DEFAULT,
        );
        lv::lv_obj_center(label);
    }
    Some(button)
}
