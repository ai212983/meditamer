use core::fmt::Write;

use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle},
};
use heapless::String;

use super::META_FONT;
use super::*;

pub(crate) fn draw_swipe_debug(
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
