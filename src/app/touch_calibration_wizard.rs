use embedded_graphics::{
    pixelcolor::BinaryColor,
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle},
};
use u8g2_fonts::types::{FontColor, HorizontalAlignment, VerticalPosition};

use super::{
    config::{META_FONT, SCREEN_WIDTH, TITLE_FONT},
    types::{InkplateDriver, TouchEvent, TouchEventKind, TouchSwipeDirection},
};

const TARGET_RADIUS_PX: i32 = 26;
const TARGET_HIT_RADIUS_PX: i32 = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WizardPhase {
    Intro,
    TapCenter,
    TapTopLeft,
    TapBottomRight,
    SwipeRight,
    Complete,
    Closed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WizardDispatch {
    Inactive,
    Consumed,
    Finished,
}

pub(crate) struct TouchCalibrationWizard {
    phase: WizardPhase,
    hint: &'static str,
}

impl TouchCalibrationWizard {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            phase: if enabled {
                WizardPhase::Intro
            } else {
                WizardPhase::Closed
            },
            hint: "",
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        !matches!(self.phase, WizardPhase::Closed)
    }

    pub(crate) fn render(&self, display: &mut InkplateDriver) {
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

        if let Some((tx, ty)) = self.target_point(width, height) {
            draw_target(display, tx, ty);
        }

        let footer = if self.hint.is_empty() {
            "Follow the target and gesture prompts."
        } else {
            self.hint
        };
        draw_centered_text(display, &META_FONT, footer, height - 42);

        let _ = display.display_bw(false);
    }

    pub(crate) fn handle_event(
        &mut self,
        display: &mut InkplateDriver,
        event: TouchEvent,
    ) -> WizardDispatch {
        if !self.is_active() {
            return WizardDispatch::Inactive;
        }

        let mut finished = false;
        match event.kind {
            TouchEventKind::Tap => {
                self.on_tap(
                    event.x,
                    event.y,
                    display.width() as i32,
                    display.height() as i32,
                );
                finished = matches!(self.phase, WizardPhase::Closed);
            }
            TouchEventKind::Swipe(direction) => {
                self.on_swipe(direction);
            }
            TouchEventKind::Cancel => {
                self.hint = "Touch canceled. Retry current step.";
            }
            _ => {}
        }

        if finished {
            return WizardDispatch::Finished;
        }

        self.render(display);
        WizardDispatch::Consumed
    }

    fn on_tap(&mut self, x: u16, y: u16, width: i32, height: i32) {
        let px = x as i32;
        let py = y as i32;

        match self.phase {
            WizardPhase::Intro => {
                self.phase = WizardPhase::TapCenter;
                self.hint = "Step 1 started.";
            }
            WizardPhase::TapCenter => {
                if self.tap_hits_target(px, py, width, height) {
                    self.phase = WizardPhase::TapTopLeft;
                    self.hint = "Center accepted.";
                } else {
                    self.hint = "Missed center target. Tap ring.";
                }
            }
            WizardPhase::TapTopLeft => {
                if self.tap_hits_target(px, py, width, height) {
                    self.phase = WizardPhase::TapBottomRight;
                    self.hint = "Top-left accepted.";
                } else {
                    self.hint = "Missed top-left target. Tap ring.";
                }
            }
            WizardPhase::TapBottomRight => {
                if self.tap_hits_target(px, py, width, height) {
                    self.phase = WizardPhase::SwipeRight;
                    self.hint = "Tap targets complete.";
                } else {
                    self.hint = "Missed bottom-right target. Tap ring.";
                }
            }
            WizardPhase::SwipeRight => {
                self.hint = "Swipe right to complete.";
            }
            WizardPhase::Complete => {
                self.phase = WizardPhase::Closed;
            }
            WizardPhase::Closed => {}
        }
    }

    fn on_swipe(&mut self, direction: TouchSwipeDirection) {
        match self.phase {
            WizardPhase::SwipeRight if matches!(direction, TouchSwipeDirection::Right) => {
                self.phase = WizardPhase::Complete;
                self.hint = "Calibration complete. Tap to continue.";
            }
            WizardPhase::SwipeRight => {
                self.hint = "Wrong direction. Swipe right.";
            }
            _ => {}
        }
    }

    fn tap_hits_target(&self, x: i32, y: i32, width: i32, height: i32) -> bool {
        let Some((tx, ty)) = self.target_point(width, height) else {
            return false;
        };
        squared_distance_i32(x, y, tx, ty) <= TARGET_HIT_RADIUS_PX * TARGET_HIT_RADIUS_PX
    }

    fn step_progress_text(&self) -> &'static str {
        match self.phase {
            WizardPhase::Intro => "Step 0/4",
            WizardPhase::TapCenter => "Step 1/4",
            WizardPhase::TapTopLeft => "Step 2/4",
            WizardPhase::TapBottomRight => "Step 3/4",
            WizardPhase::SwipeRight => "Step 4/4",
            WizardPhase::Complete => "Done",
            WizardPhase::Closed => "",
        }
    }

    fn primary_instruction(&self) -> &'static str {
        match self.phase {
            WizardPhase::Intro => "Tap anywhere to begin touch checks.",
            WizardPhase::TapCenter => "Tap the center target.",
            WizardPhase::TapTopLeft => "Tap the top-left target.",
            WizardPhase::TapBottomRight => "Tap the bottom-right target.",
            WizardPhase::SwipeRight => "Swipe right across the screen.",
            WizardPhase::Complete => "Calibration complete.",
            WizardPhase::Closed => "",
        }
    }

    fn secondary_instruction(&self) -> &'static str {
        match self.phase {
            WizardPhase::Intro => "This validates tap and swipe tracking.",
            WizardPhase::TapCenter => "Aim inside the ring.",
            WizardPhase::TapTopLeft => "Aim inside the ring.",
            WizardPhase::TapBottomRight => "Aim inside the ring.",
            WizardPhase::SwipeRight => "Start left, end right, one finger.",
            WizardPhase::Complete => "Tap anywhere to continue.",
            WizardPhase::Closed => "",
        }
    }

    fn target_point(&self, width: i32, height: i32) -> Option<(i32, i32)> {
        let w = width.max(1);
        let h = height.max(1);
        match self.phase {
            WizardPhase::TapCenter => Some((w / 2, h / 2 + 24)),
            WizardPhase::TapTopLeft => Some((w / 5, h / 3)),
            WizardPhase::TapBottomRight => Some((w * 4 / 5, h * 2 / 3)),
            _ => None,
        }
    }
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

fn squared_distance_i32(ax: i32, ay: i32, bx: i32, by: i32) -> i32 {
    let dx = ax.saturating_sub(bx);
    let dy = ay.saturating_sub(by);
    dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
}
