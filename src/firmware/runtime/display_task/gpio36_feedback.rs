use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
use u8g2_fonts::types::{FontColor, HorizontalAlignment, VerticalPosition};

use super::super::super::{
    config::TITLE_FONT,
    input::gpio36::{Gpio36Action, Gpio36Mode},
    types::{DisplayContext, InkplateDriver},
};
use super::{super::trigger_backlight_cycle, state::DisplayLoopState};

pub(super) async fn handle_gpio36_action(
    action: Gpio36Action,
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
) {
    match action {
        Gpio36Action::Touch => {}
        Gpio36Action::WakeButtonPressed => {
            esp_println::println!("input: gpio36 source=wake_button state=pressed");
            if matches!(state.gpio36_mode, Gpio36Mode::ButtonOnly) {
                render_gpio36_feedback(
                    &mut context.inkplate,
                    "WAKE: PRESSED",
                    Gpio36FeedbackStyle::Inverted,
                )
                .await;
            } else if !state.in_touch_wizard_mode() {
                trigger_backlight_cycle(
                    &mut context.inkplate,
                    &mut state.backlight_cycle_start,
                    &mut state.backlight_level,
                )
                .await;
            }
        }
        Gpio36Action::WakeButtonReleased => {
            esp_println::println!("input: gpio36 source=wake_button state=released");
            if matches!(state.gpio36_mode, Gpio36Mode::ButtonOnly) {
                render_gpio36_feedback(
                    &mut context.inkplate,
                    "WAKE: RELEASED",
                    Gpio36FeedbackStyle::Outlined,
                )
                .await;
            }
        }
    }
}

pub(super) async fn render_gpio36_ready_feedback(display: &mut InkplateDriver) {
    esp_println::println!("input: gpio36 state=ready");
    render_gpio36_feedback(
        display,
        "WAKE: READY - HOLD BUTTON",
        Gpio36FeedbackStyle::Outlined,
    )
    .await;
}

#[derive(Clone, Copy)]
enum Gpio36FeedbackStyle {
    Inverted,
    Outlined,
}

async fn render_gpio36_feedback(
    display: &mut InkplateDriver,
    label: &str,
    style: Gpio36FeedbackStyle,
) {
    const BANNER_MARGIN_PX: i32 = 8;
    const BANNER_HEIGHT_PX: i32 = 76;

    let width = display.width() as i32;
    let height = display.height() as i32;
    let top = (height - BANNER_HEIGHT_PX - BANNER_MARGIN_PX).max(BANNER_MARGIN_PX);
    let banner = Rectangle::new(
        Point::new(BANNER_MARGIN_PX, top),
        Size::new(
            (width - BANNER_MARGIN_PX * 2).max(1) as u32,
            BANNER_HEIGHT_PX as u32,
        ),
    );
    let (background, foreground) = match style {
        Gpio36FeedbackStyle::Inverted => (BinaryColor::On, BinaryColor::Off),
        Gpio36FeedbackStyle::Outlined => (BinaryColor::Off, BinaryColor::On),
    };

    let _ = banner
        .into_styled(PrimitiveStyle::with_fill(background))
        .draw(display);
    let _ = banner
        .into_styled(PrimitiveStyle::with_stroke(foreground, 3))
        .draw(display);
    let _ = TITLE_FONT.render_aligned(
        label,
        Point::new(width / 2, top + BANNER_HEIGHT_PX / 2),
        VerticalPosition::Center,
        HorizontalAlignment::Center,
        FontColor::Transparent(foreground),
        display,
    );
    let _ = display.display_bw_partial_async(false).await;
}
