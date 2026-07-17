use core::fmt::Write;

use embedded_graphics::{pixelcolor::BinaryColor, prelude::DrawTarget};
use heapless::String;

use super::super::super::super::{
    config::{META_FONT, TITLE_FONT},
    types::InkplateDriver,
};
use super::draw::{
    draw_calibration_targets, draw_centered_text, draw_continue_button, draw_frame,
    draw_precision_menu_buttons, draw_return_button, draw_swipe_case_target, draw_swipe_debug,
    draw_swipe_mark_button, draw_test_toggle, draw_test_touch, swipe_dir_label, swipe_speed_label,
};

use super::*;

impl TouchCalibrationWizard {
    pub(crate) fn new(enabled: bool) -> Self {
        Self {
            phase: if enabled {
                WizardPhase::PrecisionMenu
            } else {
                WizardPhase::Closed
            },
            hint: if enabled {
                "Calibrate precision before testing swipes."
            } else {
                ""
            },
            calibration_observations: [None; CALIBRATION_CORNER_COUNT],
            calibration: None,
            calibration_pending_return: false,
            test_mode: TestCoordinateMode::Calibrated,
            last_test_touch: None,
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
        match self.phase {
            WizardPhase::PrecisionMenu => {
                draw_centered_text(display, &TITLE_FONT, "TOUCH PRECISION", 54);
                let status = if self.calibration.is_some() {
                    "Calibration ready"
                } else {
                    "Not calibrated"
                };
                draw_centered_text(display, &META_FONT, status, 112);
                draw_precision_menu_buttons(display, width, height);
            }
            WizardPhase::Calibrate => {
                draw_centered_text(display, &TITLE_FONT, "CALIBRATE TOUCH", 40);
                draw_centered_text(display, &META_FONT, "Touch all four corner dots.", 92);
                let mut progress: String<48> = String::new();
                let _ = write!(
                    &mut progress,
                    "Corners {}/{}",
                    self.calibration_count(),
                    CALIBRATION_CORNER_COUNT,
                );
                draw_centered_text(display, &META_FONT, &progress, 122);
                draw_calibration_targets(
                    display,
                    Self::calibration_targets(width, height),
                    self.calibration_observations,
                );
            }
            WizardPhase::PrecisionTest => {
                draw_centered_text(display, &TITLE_FONT, "TEST TOUCH PRECISION", 54);
                draw_centered_text(
                    display,
                    &META_FONT,
                    "Tap the center toggle to change coordinates.",
                    104,
                );
                draw_test_toggle(display, width, height, self.test_mode);
                if let Some(touch) = self.last_test_touch {
                    draw_test_touch(display, touch, self.test_mode);
                }
                draw_return_button(display, width, height);
            }
            WizardPhase::SwipeRight => {
                draw_centered_text(display, &TITLE_FONT, "TOUCH INPUT TEST", 40);
                draw_centered_text(display, &META_FONT, self.step_progress_text(), 74);
                draw_centered_text(display, &META_FONT, self.primary_instruction(), 120);
                draw_centered_text(display, &META_FONT, self.secondary_instruction(), 154);
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
                draw_swipe_debug(
                    display,
                    self.swipe_trace,
                    self.last_swipe,
                    self.swipe_debug,
                    self.swipe_case_passed,
                    self.swipe_case_attempts,
                    self.manual_swipe_marks,
                );
                draw_continue_button(display, width, height, self.continue_button_label());
                draw_swipe_mark_button(display, width, height);
            }
            WizardPhase::Complete => {
                draw_centered_text(display, &TITLE_FONT, "TOUCH INPUT TEST", 40);
                draw_centered_text(display, &META_FONT, "Done", 74);
                draw_centered_text(display, &META_FONT, "Swipe test complete.", 120);
                draw_centered_text(display, &META_FONT, "Exit with the EXIT button.", 154);
                draw_continue_button(display, width, height, self.continue_button_label());
            }
            WizardPhase::Closed => {}
        }

        let footer = if self.hint.is_empty() {
            "Choose an action."
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
    draw_centered_text(display, &TITLE_FONT, "TOUCH INPUT TEST", 40);
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
