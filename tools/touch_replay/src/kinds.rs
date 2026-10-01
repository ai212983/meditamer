use crate::touch_core::{TouchEventKind, TouchSwipeDirection};

pub(crate) fn kind_label(kind: TouchEventKind) -> &'static str {
    match kind {
        TouchEventKind::Down => "down",
        TouchEventKind::Move => "move",
        TouchEventKind::Up => "up",
        TouchEventKind::Tap => "tap",
        TouchEventKind::LongPress => "long_press",
        TouchEventKind::Swipe(TouchSwipeDirection::Left) => "swipe_left",
        TouchEventKind::Swipe(TouchSwipeDirection::Right) => "swipe_right",
        TouchEventKind::Swipe(TouchSwipeDirection::Up) => "swipe_up",
        TouchEventKind::Swipe(TouchSwipeDirection::Down) => "swipe_down",
        TouchEventKind::Cancel => "cancel",
    }
}
