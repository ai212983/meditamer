use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle},
};
use u8g2_fonts::types::{FontColor, HorizontalAlignment, VerticalPosition};

use super::super::super::super::{
    config::{META_FONT, SCREEN_WIDTH},
    types::InkplateDriver,
};
use super::*;

mod debug;
pub(super) use debug::draw_swipe_debug;
mod layout;
pub(super) use layout::{
    continue_button_bounds, precision_menu_button_bounds, swipe_mark_button_bounds,
    test_return_bounds, test_toggle_bounds,
};

pub(super) type ButtonBounds = (i32, i32, i32, i32);

pub(super) fn draw_frame(display: &mut InkplateDriver, width: i32, height: i32) {
    let style = PrimitiveStyle::with_stroke(BinaryColor::On, 1);
    let _ = Rectangle::new(
        Point::new(12, 12),
        Size::new((width - 24).max(1) as u32, (height - 24).max(1) as u32),
    )
    .into_styled(style)
    .draw(display);
}

pub(super) fn draw_centered_text(
    display: &mut InkplateDriver,
    renderer: &u8g2_fonts::FontRenderer,
    text: &str,
    center_y: i32,
) {
    draw_centered_text_at(display, renderer, text, SCREEN_WIDTH / 2, center_y);
}

fn draw_centered_text_at(
    display: &mut InkplateDriver,
    renderer: &u8g2_fonts::FontRenderer,
    text: &str,
    center_x: i32,
    center_y: i32,
) {
    let _ = renderer.render_aligned(
        text,
        Point::new(center_x, center_y),
        VerticalPosition::Center,
        HorizontalAlignment::Center,
        FontColor::Transparent(BinaryColor::On),
        display,
    );
}

pub(super) fn draw_left_text(
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

pub(super) fn draw_calibration_targets(
    display: &mut InkplateDriver,
    targets: [SwipePoint; CALIBRATION_CORNER_COUNT],
    observations: [Option<TapObservation>; CALIBRATION_CORNER_COUNT],
) {
    for (index, target) in targets.iter().enumerate() {
        let radius = if observations[index].is_some() { 8 } else { 11 };
        let style = if observations[index].is_some() {
            PrimitiveStyle::with_fill(BinaryColor::On)
        } else {
            PrimitiveStyle::with_stroke(BinaryColor::On, 2)
        };
        let _ = Circle::new(
            Point::new(target.x - radius, target.y - radius),
            (radius * 2) as u32,
        )
        .into_styled(style)
        .draw(display);
    }
}

pub(super) fn draw_precision_menu_buttons(display: &mut InkplateDriver, width: i32, height: i32) {
    let (calibrate, test, continue_button) = precision_menu_button_bounds(width, height);
    draw_button(display, calibrate, "CALIBRATE");
    draw_button(display, test, "TEST");
    draw_button(display, continue_button, "CONTINUE");
}

pub(super) fn draw_test_toggle(
    display: &mut InkplateDriver,
    width: i32,
    height: i32,
    mode: TestCoordinateMode,
) {
    let label = match mode {
        TestCoordinateMode::Calibrated => "CALIBRATED: CIRCLE",
        TestCoordinateMode::Uncalibrated => "UNCALIBRATED: X",
    };
    draw_button(display, test_toggle_bounds(width, height), label);
}

pub(super) fn draw_return_button(display: &mut InkplateDriver, width: i32, height: i32) {
    draw_button(display, test_return_bounds(width, height), "RETURN");
}

pub(super) fn draw_test_touch(
    display: &mut InkplateDriver,
    touch: TestTouch,
    mode: TestCoordinateMode,
) {
    let point = match mode {
        TestCoordinateMode::Calibrated => touch.calibrated,
        TestCoordinateMode::Uncalibrated => touch.raw,
    };
    match mode {
        TestCoordinateMode::Calibrated => {
            let radius = 12;
            let _ = Circle::new(
                Point::new(point.x - radius, point.y - radius),
                (radius * 2) as u32,
            )
            .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
            .draw(display);
        }
        TestCoordinateMode::Uncalibrated => {
            let radius = 10;
            let style = PrimitiveStyle::with_stroke(BinaryColor::On, 2);
            let _ = Line::new(
                Point::new(point.x - radius, point.y - radius),
                Point::new(point.x + radius, point.y + radius),
            )
            .into_styled(style)
            .draw(display);
            let _ = Line::new(
                Point::new(point.x - radius, point.y + radius),
                Point::new(point.x + radius, point.y - radius),
            )
            .into_styled(style)
            .draw(display);
        }
    }
}

pub(super) fn draw_swipe_case_target(display: &mut InkplateDriver, case: SwipeCaseSpec) {
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

pub(super) fn swipe_speed_label(speed: SwipeSpeedTier) -> &'static str {
    match speed {
        SwipeSpeedTier::ExtraFast => "extrafast",
        SwipeSpeedTier::Fast => "fast",
        SwipeSpeedTier::Medium => "medium",
        SwipeSpeedTier::Slow => "slow",
    }
}

pub(super) fn swipe_dir_label(direction: TouchSwipeDirection) -> &'static str {
    match direction {
        TouchSwipeDirection::Left => "left",
        TouchSwipeDirection::Right => "right",
        TouchSwipeDirection::Up => "up",
        TouchSwipeDirection::Down => "down",
    }
}

pub(super) fn draw_continue_button(
    display: &mut InkplateDriver,
    width: i32,
    height: i32,
    label: &str,
) {
    let (left, top, w, h) = continue_button_bounds(width, height);
    let _ = Rectangle::new(
        Point::new(left, top),
        Size::new(w.max(1) as u32, h.max(1) as u32),
    )
    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
    .draw(display);
    draw_centered_text(display, &META_FONT, label, top + h / 2);
}

fn draw_button(display: &mut InkplateDriver, bounds: ButtonBounds, label: &str) {
    let (left, top, width, height) = bounds;
    let _ = Rectangle::new(
        Point::new(left, top),
        Size::new(width.max(1) as u32, height.max(1) as u32),
    )
    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
    .draw(display);
    draw_centered_text_at(
        display,
        &META_FONT,
        label,
        left + width / 2,
        top + height / 2,
    );
}

pub(super) fn draw_swipe_mark_button(display: &mut InkplateDriver, width: i32, height: i32) {
    let (left, top, w, h) = swipe_mark_button_bounds(width, height);
    let _ = Rectangle::new(
        Point::new(left, top),
        Size::new(w.max(1) as u32, h.max(1) as u32),
    )
    .into_styled(PrimitiveStyle::with_stroke(BinaryColor::On, 2))
    .draw(display);
    draw_centered_text(display, &META_FONT, "I JUST SWIPED", top + h / 2);
}
