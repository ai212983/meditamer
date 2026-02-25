use core::fmt::Write;

use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle},
};
use heapless::String;
use u8g2_fonts::types::{FontColor, HorizontalAlignment, VerticalPosition};

use super::super::super::super::{
    config::{META_FONT, SCREEN_WIDTH, TITLE_FONT},
    types::InkplateDriver,
};

use super::*;

impl TouchCalibrationWizard {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            phase: if enabled {
                WizardPhase::Intro
            } else {
                WizardPhase::Closed
            },
            hint: "",
            last_tap: None,
            swipe_trace: SwipeTrace::default(),
            last_swipe: None,
            swipe_trace_pending_points: 0,
            swipe_debug: SwipeDebugStats::default(),
            swipe_case_index: 0,
            swipe_case_passed: 0,
            swipe_case_failed: 0,
            swipe_case_attempts: 0,
            manual_swipe_marks: 0,
            pending_swipe_release: None,
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        !matches!(self.phase, WizardPhase::Closed)
    }

    pub(crate) async fn render_full(&self, display: &mut InkplateDriver) {
        self.render_with_refresh(display, true).await;
    }

    pub(crate) async fn render_partial(&self, display: &mut InkplateDriver) {
        self.render_with_refresh(display, false).await;
    }

    async fn render_with_refresh(&self, display: &mut InkplateDriver, full_refresh: bool) {
        if !self.is_active() {
            return;
        }

        let width = display.width() as i32;
        let height = display.height() as i32;
        let _ = display.clear(BinaryColor::Off);

        draw_frame(display, width, height);
        draw_centered_text(display, &TITLE_FONT, "TOUCH CALIBRATION WIZARD", 40);
        draw_centered_text(display, &META_FONT, self.step_progress_text(), 74);
        draw_centered_text(display, &META_FONT, self.primary_instruction(), 120);
        draw_centered_text(display, &META_FONT, self.secondary_instruction(), 154);
        if matches!(self.phase, WizardPhase::SwipeRight) {
            if let Some(case) = self.current_swipe_case(width, height) {
                draw_swipe_case_target(display, case);
                let mut case_line: String<96> = String::new();
                let _ = write!(
                    &mut case_line,
                    "Case {}/{}: {} {}",
                    self.swipe_case_index.saturating_add(1),
                    SWIPE_CASE_COUNT,
                    swipe_dir_label(case.direction),
                    swipe_speed_label(case.speed),
                );
                draw_centered_text(display, &META_FONT, &case_line, 182);
            }
        }

        if let Some((tx, ty)) = self.target_point(width, height) {
            draw_target(display, tx, ty);
            if let Some(last_tap) = self.last_tap {
                draw_tap_attempt_feedback(display, tx, ty, last_tap);
            }
        }
        if self.shows_swipe_debug() {
            draw_swipe_debug(
                display,
                self.swipe_trace,
                self.last_swipe,
                self.swipe_debug,
                self.swipe_case_passed,
                self.swipe_case_attempts,
                self.manual_swipe_marks,
            );
        }
        if self.shows_continue_button() {
            draw_continue_button(display, width, height, self.continue_button_label());
        }
        if self.shows_swipe_mark_button() {
            draw_swipe_mark_button(display, width, height);
        }

        let footer = if self.hint.is_empty() {
            "Follow the target and gesture prompts."
        } else {
            self.hint
        };
        draw_centered_text(display, &META_FONT, footer, height - 42);

        if full_refresh {
            let _ = display.display_bw_async(false).await;
        } else {
            let _ = display.display_bw_partial_async(false).await;
        }
    }
}

pub(crate) async fn render_touch_wizard_waiting_screen(display: &mut InkplateDriver) {
    let width = display.width() as i32;
    let height = display.height() as i32;
    let _ = display.clear(BinaryColor::Off);

    draw_frame(display, width, height);
    draw_centered_text(display, &TITLE_FONT, "TOUCH CALIBRATION WIZARD", 40);
    draw_centered_text(display, &META_FONT, "Waiting For Touch Controller", 86);
    draw_centered_text(
        display,
        &META_FONT,
        "Touch init failed or disconnected.",
        126,
    );
    draw_centered_text(
        display,
        &META_FONT,
        "Keep device powered and wait for retry.",
        158,
    );
    draw_centered_text(
        display,
        &META_FONT,
        "Wizard will start automatically.",
        height - 42,
    );

    let _ = display.display_bw_async(false).await;
}

fn draw_frame(display: &mut InkplateDriver, width: i32, height: i32) {
    let style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
    let _ = Rectangle::new(
        Point::new(12, 12),
        Size::new((width - 24).max(1) as u32, (height - 24).max(1) as u32),
    )
    .into_styled(style)
    .draw(display);
}

fn draw_centered_text(
    display: &mut InkplateDriver,
    renderer: &u8g2_fonts::FontRenderer,
    text: &str,
    center_y: i32,
) {
    let _ = renderer.render_aligned(
        text,
        Point::new(SCREEN_WIDTH / 2, center_y),
        VerticalPosition::Center,
        HorizontalAlignment::Center,
        FontColor::Transparent(BinaryColor::On),
        display,
    );
}

fn draw_left_text(
    display: &mut InkplateDriver,
    renderer: &u8g2_fonts::FontRenderer,
    text: &str,
    left_x: i32,
    center_y: i32,
) {
    let _ = renderer.render_aligned(
        text,
        Point::new(left_x, center_y),
        VerticalPosition::Center,
        HorizontalAlignment::Left,
        FontColor::Transparent(BinaryColor::On),
        display,
    );
}

fn draw_target(display: &mut InkplateDriver, x: i32, y: i32) {
    let style = PrimitiveStyle::with_stroke(BinaryColor::On, 2);
    let _ = Circle::new(
        Point::new(x - TARGET_RADIUS_PX, y - TARGET_RADIUS_PX),
        (TARGET_RADIUS_PX * 2).max(1) as u32,
    )
    .into_styled(style)
    .draw(display);

    let _ = Line::new(Point::new(x - 10, y), Point::new(x + 10, y))
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(display);
    let _ = Line::new(Point::new(x, y - 10), Point::new(x, y + 10))
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(display);
}

fn draw_swipe_case_target(display: &mut InkplateDriver, case: SwipeCaseSpec) {
    let _ = Line::new(
        Point::new(case.start.x, case.start.y),
        Point::new(case.end.x, case.end.y),
    )
    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
    .draw(display);

    let _ = Circle::new(
        Point::new(
            case.start.x - SWIPE_CASE_START_RADIUS_PX,
            case.start.y - SWIPE_CASE_START_RADIUS_PX,
        ),
        (SWIPE_CASE_START_RADIUS_PX * 2) as u32,
    )
    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
    .draw(display);

    let _ = Circle::new(
        Point::new(
            case.end.x - SWIPE_CASE_END_RADIUS_PX,
            case.end.y - SWIPE_CASE_END_RADIUS_PX,
        ),
        (SWIPE_CASE_END_RADIUS_PX * 2) as u32,
    )
    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
    .draw(display);

    let vx = case.end.x.saturating_sub(case.start.x);
    let vy = case.end.y.saturating_sub(case.start.y);
    let vmax = vx.abs().max(vy.abs()).max(1);
    let ux = vx.saturating_mul(16) / vmax;
    let uy = vy.saturating_mul(16) / vmax;
    let px = -uy / 2;
    let py = ux / 2;
    let ax = case.end.x.saturating_sub(ux);
    let ay = case.end.y.saturating_sub(uy);

    let _ = Line::new(
        Point::new(ax.saturating_add(px), ay.saturating_add(py)),
        Point::new(case.end.x, case.end.y),
    )
    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
    .draw(display);
    let _ = Line::new(
        Point::new(ax.saturating_sub(px), ay.saturating_sub(py)),
        Point::new(case.end.x, case.end.y),
    )
    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
    .draw(display);

    draw_left_text(
        display,
        &META_FONT,
        "FROM",
        case.start.x.saturating_sub(34),
        case.start.y.saturating_sub(SWIPE_CASE_START_RADIUS_PX + 12),
    );
    draw_left_text(
        display,
        &META_FONT,
        "TO",
        case.end.x.saturating_sub(14),
        case.end.y.saturating_sub(SWIPE_CASE_END_RADIUS_PX + 12),
    );
}

fn swipe_speed_label(speed: SwipeSpeedTier) -> &'static str {
    match speed {
        SwipeSpeedTier::ExtraFast => "extrafast",
        SwipeSpeedTier::Fast => "fast",
        SwipeSpeedTier::Medium => "medium",
        SwipeSpeedTier::Slow => "slow",
    }
}

fn swipe_dir_label(direction: TouchSwipeDirection) -> &'static str {
    match direction {
        TouchSwipeDirection::Left => "left",
        TouchSwipeDirection::Right => "right",
        TouchSwipeDirection::Up => "up",
        TouchSwipeDirection::Down => "down",
    }
}

fn draw_swipe_debug(
    display: &mut InkplateDriver,
    trace: SwipeTrace,
    attempt: Option<SwipeAttempt>,
    stats: SwipeDebugStats,
    case_passed: u8,
    case_attempts: u16,
    manual_marks: u16,
) {
    if trace.len >= 2 {
        let mut idx = 1usize;
        while idx < trace.len as usize {
            let a = trace.points[idx - 1];
            let b = trace.points[idx];
            let _ = Line::new(Point::new(a.x, a.y), Point::new(b.x, b.y))
                .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
                .draw(display);
            idx += 1;
        }
    } else if trace.len == 1 {
        let p = trace.points[0];
        let _ = Circle::new(Point::new(p.x - 3, p.y - 3), 6)
            .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
            .draw(display);
    }

    if let Some(attempt) = attempt {
        let _ = Line::new(
            Point::new(attempt.start.x, attempt.start.y),
            Point::new(attempt.end.x, attempt.end.y),
        )
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(display);

        let _ = Circle::new(Point::new(attempt.start.x - 4, attempt.start.y - 4), 8)
            .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
            .draw(display);
        if attempt.accepted {
            let _ = Circle::new(Point::new(attempt.end.x - 5, attempt.end.y - 5), 10)
                .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
                .draw(display);
        } else {
            let _ = Line::new(
                Point::new(attempt.end.x - 7, attempt.end.y - 7),
                Point::new(attempt.end.x + 7, attempt.end.y + 7),
            )
            .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
            .draw(display);
            let _ = Line::new(
                Point::new(attempt.end.x - 7, attempt.end.y + 7),
                Point::new(attempt.end.x + 7, attempt.end.y - 7),
            )
            .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
            .draw(display);
        }
    }

    let mut counts_line: String<64> = String::new();
    let _ = write!(
        &mut counts_line,
        "D/M/U/S/C: {}/{}/{}/{}/{}",
        stats.down_count, stats.move_count, stats.up_count, stats.swipe_count, stats.cancel_count
    );
    draw_left_text(display, &META_FONT, &counts_line, 32, 404);

    let mut case_line: String<64> = String::new();
    let _ = write!(
        &mut case_line,
        "cases pass/attempt: {}/{} marks={}",
        case_passed, case_attempts, manual_marks
    );
    draw_left_text(display, &META_FONT, &case_line, 32, 430);

    let last_kind = match stats.last_kind {
        SwipeDebugKind::None => "none",
        SwipeDebugKind::Down => "down",
        SwipeDebugKind::Move => "move",
        SwipeDebugKind::Up => "up",
        SwipeDebugKind::Swipe(TouchSwipeDirection::Left) => "swipe_left",
        SwipeDebugKind::Swipe(TouchSwipeDirection::Right) => "swipe_right",
        SwipeDebugKind::Swipe(TouchSwipeDirection::Up) => "swipe_up",
        SwipeDebugKind::Swipe(TouchSwipeDirection::Down) => "swipe_down",
        SwipeDebugKind::Cancel => "cancel",
    };
    let dx = stats.last_end.x.saturating_sub(stats.last_start.x);
    let dy = stats.last_end.y.saturating_sub(stats.last_start.y);
    let mut vector_line: String<96> = String::new();
    let _ = write!(
        &mut vector_line,
        "last={} dur={}ms dx={} dy={}",
        last_kind, stats.last_duration_ms, dx, dy
    );
    draw_left_text(display, &META_FONT, &vector_line, 32, 456);

    let (from, to) = if let Some(attempt) = attempt {
        (attempt.start, attempt.end)
    } else {
        (stats.last_start, stats.last_end)
    };
    let mut points_line: String<96> = String::new();
    let _ = write!(
        &mut points_line,
        "from=({}, {}) to=({}, {})",
        from.x, from.y, to.x, to.y
    );
    draw_left_text(display, &META_FONT, &points_line, 32, 476);
}

fn draw_continue_button(display: &mut InkplateDriver, width: i32, height: i32, label: &str) {
    let (left, top, w, h) = continue_button_bounds(width, height);
    let _ = Rectangle::new(
        Point::new(left, top),
        Size::new(w.max(1) as u32, h.max(1) as u32),
    )
    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
    .draw(display);
    draw_centered_text(display, &META_FONT, label, top + h / 2);
}

fn draw_swipe_mark_button(display: &mut InkplateDriver, width: i32, height: i32) {
    let (left, top, w, h) = swipe_mark_button_bounds(width, height);
    let _ = Rectangle::new(
        Point::new(left, top),
        Size::new(w.max(1) as u32, h.max(1) as u32),
    )
    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
    .draw(display);
    draw_centered_text(display, &META_FONT, "I JUST SWIPED", top + h / 2);
}

pub(super) fn continue_button_bounds(width: i32, height: i32) -> (i32, i32, i32, i32) {
    let w = CONTINUE_BUTTON_WIDTH.min(width - 24).max(80);
    let h = CONTINUE_BUTTON_HEIGHT;
    let left = (width - w) / 2;
    let top = height - 108;
    (left, top, w, h)
}

pub(super) fn swipe_mark_button_bounds(width: i32, height: i32) -> (i32, i32, i32, i32) {
    let w = SWIPE_MARK_BUTTON_WIDTH.min(width - 24).max(100);
    let h = SWIPE_MARK_BUTTON_HEIGHT;
    let left = (width - w) / 2;
    let top = height - 166;
    (left, top, w, h)
}

fn draw_tap_attempt_feedback(
    display: &mut InkplateDriver,
    target_x: i32,
    target_y: i32,
    tap: TapAttempt,
) {
    let _ = Line::new(Point::new(target_x, target_y), Point::new(tap.x, tap.y))
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(display);

    if tap.hit {
        let _ = Circle::new(Point::new(tap.x - 5, tap.y - 5), 10)
            .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
            .draw(display);
    } else {
        let _ = Line::new(
            Point::new(tap.x - 7, tap.y - 7),
            Point::new(tap.x + 7, tap.y + 7),
        )
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(display);
        let _ = Line::new(
            Point::new(tap.x - 7, tap.y + 7),
            Point::new(tap.x + 7, tap.y - 7),
        )
        .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 1))
        .draw(display);
    }
}
