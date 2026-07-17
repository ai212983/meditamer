use super::{
    ButtonBounds, CONTINUE_BUTTON_HEIGHT, CONTINUE_BUTTON_WIDTH, SWIPE_MARK_BUTTON_HEIGHT,
    SWIPE_MARK_BUTTON_WIDTH,
};

pub(in crate::firmware::touch::wizard::engine) fn continue_button_bounds(
    width: i32,
    height: i32,
) -> ButtonBounds {
    let w = CONTINUE_BUTTON_WIDTH.min(width - 24).max(80);
    let h = CONTINUE_BUTTON_HEIGHT;
    let left = (width - w) / 2;
    let top = height - 108;
    (left, top, w, h)
}

pub(in crate::firmware::touch::wizard::engine) fn swipe_mark_button_bounds(
    width: i32,
    height: i32,
) -> ButtonBounds {
    let w = SWIPE_MARK_BUTTON_WIDTH.min(width - 24).max(100);
    let h = SWIPE_MARK_BUTTON_HEIGHT;
    let left = (width - w) / 2;
    let top = height - 166;
    (left, top, w, h)
}

pub(in crate::firmware::touch::wizard::engine) fn precision_menu_button_bounds(
    width: i32,
    height: i32,
) -> (ButtonBounds, ButtonBounds, ButtonBounds) {
    let gap = 24;
    let side_margin = 48;
    let button_width = ((width - side_margin * 2 - gap) / 2).max(100);
    let button_height = 58;
    let first_top = height / 2 - 72;
    let calibrate = (side_margin, first_top, button_width, button_height);
    let test = (
        side_margin + button_width + gap,
        first_top,
        button_width,
        button_height,
    );
    let continue_width = 240.min(width - side_margin * 2).max(120);
    let continue_button = (
        (width - continue_width) / 2,
        first_top + button_height + 34,
        continue_width,
        button_height,
    );
    (calibrate, test, continue_button)
}

pub(in crate::firmware::touch::wizard::engine) fn test_toggle_bounds(
    width: i32,
    height: i32,
) -> ButtonBounds {
    let button_width = 320.min(width - 48).max(160);
    let button_height = 58;
    (
        (width - button_width) / 2,
        (height - button_height) / 2,
        button_width,
        button_height,
    )
}

pub(in crate::firmware::touch::wizard::engine) fn test_return_bounds(
    width: i32,
    height: i32,
) -> ButtonBounds {
    continue_button_bounds(width, height)
}
