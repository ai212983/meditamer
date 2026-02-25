use core::fmt::Write;

use embedded_graphics::{pixelcolor::BinaryColor, prelude::DrawTarget};
use heapless::String;

use super::super::super::super::{
    config::{META_FONT, TITLE_FONT},
    types::InkplateDriver,
};
use super::draw::{
    draw_centered_text, draw_continue_button, draw_frame, draw_swipe_case_target, draw_swipe_debug,
    draw_swipe_mark_button, draw_tap_attempt_feedback, draw_target, swipe_dir_label,
    swipe_speed_label,
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
