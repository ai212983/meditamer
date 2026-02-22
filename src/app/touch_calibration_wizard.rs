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
const TARGET_HIT_RADIUS_PX: i32 = TARGET_RADIUS_PX;
const WIZARD_SWIPE_RELEASE_MIN_DX_PX: i32 = 72;
const WIZARD_SWIPE_RELEASE_MAX_ABS_DY_PX: i32 = 180;
const WIZARD_SWIPE_RELEASE_DOMINANCE_X100: i32 = 115;
const WIZARD_SWIPE_RELEASE_MAX_DURATION_MS: u16 = 1_600;

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
    last_tap: Option<TapAttempt>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TapAttempt {
    x: i32,
    y: i32,
    hit: bool,
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
            last_tap: None,
        }
    }

    pub(crate) fn is_active(&self) -> bool {
        !matches!(self.phase, WizardPhase::Closed)
    }

    pub(crate) fn render_full(&self, display: &mut InkplateDriver) {
        self.render_with_refresh(display, true);
    }

    pub(crate) fn render_partial(&self, display: &mut InkplateDriver) {
        self.render_with_refresh(display, false);
    }

    fn render_with_refresh(&self, display: &mut InkplateDriver, full_refresh: bool) {
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
            if let Some(last_tap) = self.last_tap {
                draw_tap_attempt_feedback(display, tx, ty, last_tap);
            }
        }

        let footer = if self.hint.is_empty() {
            "Follow the target and gesture prompts."
        } else {
            self.hint
        };
        draw_centered_text(display, &META_FONT, footer, height - 42);

        if full_refresh {
            let _ = display.display_bw(false);
        } else {
            let _ = display.display_bw_partial(false);
        }
    }

    pub(crate) fn handle_event(
        &mut self,
        display: &mut InkplateDriver,
        event: TouchEvent,
    ) -> WizardDispatch {
        if !self.is_active() {
            return WizardDispatch::Inactive;
        }

        let width = display.width() as i32;
        let height = display.height() as i32;
        let prev_phase = self.phase;
        let mut changed = false;
        match event.kind {
            TouchEventKind::Down => {
                // Some panels report stable Down sooner than Tap/Up at the intro step.
                if matches!(self.phase, WizardPhase::Intro) {
                    changed = self.on_tap(event.x, event.y, width, height);
                }
            }
            TouchEventKind::Tap => {
                changed = self.on_tap(event.x, event.y, width, height);
            }
            TouchEventKind::Up => {
                if matches!(self.phase, WizardPhase::SwipeRight) {
                    changed = self.on_swipe_release(event);
                } else {
                    changed = self.on_tap(event.x, event.y, width, height);
                }
            }
            TouchEventKind::LongPress => {
                // Fallback for panels where Tap classification is timing-sensitive.
                changed = self.on_tap(event.x, event.y, width, height);
            }
            TouchEventKind::Swipe(direction) => {
                changed = self.on_swipe(direction);
            }
            TouchEventKind::Cancel => {
                self.hint = "Touch canceled. Retry current step.";
                self.last_tap = None;
                changed = true;
            }
            _ => {}
        }

        let finished = matches!(self.phase, WizardPhase::Closed);
        if finished {
            return WizardDispatch::Finished;
        }

        if changed {
            if matches!(prev_phase, WizardPhase::Intro) {
                self.render_full(display);
            } else {
                self.render_partial(display);
            }
        }
        WizardDispatch::Consumed
    }

    fn on_tap(&mut self, x: u16, y: u16, width: i32, height: i32) -> bool {
        let px = x as i32;
        let py = y as i32;
        let prev_phase = self.phase;
        let prev_hint = self.hint;
        let prev_last_tap = self.last_tap;

        match self.phase {
            WizardPhase::Intro => {
                self.phase = WizardPhase::TapCenter;
                self.hint = "Step 1 started. Tap center target.";
                self.last_tap = None;
            }
            WizardPhase::TapCenter => {
                let hit = self.tap_hits_target(px, py, width, height);
                if hit {
                    self.phase = WizardPhase::TapTopLeft;
                    self.hint = "Center accepted.";
                    self.last_tap = None;
                } else {
                    self.hint = "Missed center target. See marker.";
                    self.last_tap = Some(TapAttempt { x: px, y: py, hit });
                }
            }
            WizardPhase::TapTopLeft => {
                let hit = self.tap_hits_target(px, py, width, height);
                if hit {
                    self.phase = WizardPhase::TapBottomRight;
                    self.hint = "Top-left accepted.";
                    self.last_tap = None;
                } else {
                    self.hint = "Missed top-left target. See marker.";
                    self.last_tap = Some(TapAttempt { x: px, y: py, hit });
                }
            }
            WizardPhase::TapBottomRight => {
                let hit = self.tap_hits_target(px, py, width, height);
                if hit {
                    self.phase = WizardPhase::SwipeRight;
                    self.hint = "Tap targets complete.";
                    self.last_tap = None;
                } else {
                    self.hint = "Missed bottom-right target. See marker.";
                    self.last_tap = Some(TapAttempt { x: px, y: py, hit });
                }
            }
            WizardPhase::SwipeRight => {
                self.hint = "Swipe right to complete.";
                self.last_tap = None;
            }
            WizardPhase::Complete => {
                self.phase = WizardPhase::Closed;
                self.last_tap = None;
            }
            WizardPhase::Closed => {}
        }

        self.phase != prev_phase || self.hint != prev_hint || self.last_tap != prev_last_tap
    }

    fn on_swipe(&mut self, direction: TouchSwipeDirection) -> bool {
        let prev_phase = self.phase;
        let prev_hint = self.hint;
        let prev_last_tap = self.last_tap;
        match self.phase {
            WizardPhase::SwipeRight if matches!(direction, TouchSwipeDirection::Right) => {
                self.phase = WizardPhase::Complete;
                self.hint = "Calibration complete. Tap to continue.";
                self.last_tap = None;
            }
            WizardPhase::SwipeRight => {
                self.hint = "Wrong direction. Swipe right.";
            }
            _ => {}
        }
        self.phase != prev_phase || self.hint != prev_hint || self.last_tap != prev_last_tap
    }

    fn on_swipe_release(&mut self, event: TouchEvent) -> bool {
        let prev_phase = self.phase;
        let prev_hint = self.hint;
        let prev_last_tap = self.last_tap;

        if matches!(self.phase, WizardPhase::SwipeRight) {
            if self.release_matches_right_swipe(event) {
                self.phase = WizardPhase::Complete;
                self.hint = "Calibration complete. Tap to continue.";
                self.last_tap = None;
            } else {
                self.hint = "Swipe right farther (mostly horizontal).";
            }
        }

        self.phase != prev_phase || self.hint != prev_hint || self.last_tap != prev_last_tap
    }

    fn release_matches_right_swipe(&self, event: TouchEvent) -> bool {
        let dx = event.x as i32 - event.start_x as i32;
        let dy = event.y as i32 - event.start_y as i32;
        let abs_dx = dx.abs();
        let abs_dy = dy.abs();

        if event.duration_ms > WIZARD_SWIPE_RELEASE_MAX_DURATION_MS {
            return false;
        }
        if dx < WIZARD_SWIPE_RELEASE_MIN_DX_PX {
            return false;
        }
        if abs_dy > WIZARD_SWIPE_RELEASE_MAX_ABS_DY_PX {
            return false;
        }
        if abs_dx * 100 < abs_dy * WIZARD_SWIPE_RELEASE_DOMINANCE_X100 {
            return false;
        }

        true
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

pub(crate) fn render_touch_wizard_waiting_screen(display: &mut InkplateDriver) {
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

    let _ = display.display_bw(false);
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

fn squared_distance_i32(ax: i32, ay: i32, bx: i32, by: i32) -> i32 {
    let dx = ax.saturating_sub(bx);
    let dy = ay.saturating_sub(by);
    dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
}
