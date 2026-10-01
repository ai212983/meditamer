use embassy_time::Instant;

use super::super::types::DisplayContext;
use super::super::{
    config::{BACKLIGHT_FADE_MS, BACKLIGHT_HOLD_MS, BACKLIGHT_MAX_BRIGHTNESS},
    types::InkplateDriver,
};
use super::frontlight_policy::FrontlightCalibrationSession;
use super::frontlight_policy::{
    application_deadline, application_due, fade_deadline_ms, needs_level_apply,
    target_level_for_elapsed, FadeSpec, FRONTLIGHT_APPLY_RETRY_MS,
};
use super::presentation;
use super::state::DisplayLoopState;
use crate::firmware::ui::lvgl::FrontlightCalibrationEffect;

pub(super) fn fade_spec(baseline: u8) -> FadeSpec {
    FadeSpec {
        max: BACKLIGHT_MAX_BRIGHTNESS,
        hold_ms: BACKLIGHT_HOLD_MS,
        fade_ms: BACKLIGHT_FADE_MS,
        baseline,
    }
}

pub(crate) async fn trigger_backlight_cycle(
    display: &mut InkplateDriver,
    backlight_cycle_start: &mut Option<Instant>,
    backlight_level: &mut u8,
    retry_at_ms: &mut Option<u64>,
) {
    *backlight_cycle_start = Some(Instant::now());
    apply_backlight_level(
        display,
        backlight_level,
        BACKLIGHT_MAX_BRIGHTNESS,
        retry_at_ms,
    )
    .await;
}

/// Drives the active flash toward the baseline and settles exactly there.
/// When no cycle is active but the recorded level disagrees with the
/// baseline (a previous application failed), re-applies the baseline so a
/// failed steady-level change still converges via the retry deadline.
pub(crate) async fn run_backlight_timeline(
    display: &mut InkplateDriver,
    backlight_cycle_start: &mut Option<Instant>,
    backlight_level: &mut u8,
    baseline: u8,
    retry_at_ms: &mut Option<u64>,
) {
    let now_ms = Instant::now().as_millis();
    if !application_due(*retry_at_ms, now_ms) {
        return;
    }

    let Some(cycle_start) = *backlight_cycle_start else {
        if needs_level_apply(*backlight_level, baseline, retry_at_ms.is_some()) {
            apply_backlight_level(display, backlight_level, baseline, retry_at_ms).await;
        }
        return;
    };

    let elapsed_ms = Instant::now()
        .saturating_duration_since(cycle_start)
        .as_millis();
    let (target_level, cycle_done) = target_level_for_elapsed(fade_spec(baseline), elapsed_ms);
    if cycle_done {
        *backlight_cycle_start = None;
    }

    apply_backlight_level(display, backlight_level, target_level, retry_at_ms).await;
}

/// Records `next_level` only when the hardware confirms it. A failed
/// application keeps the old recorded level and arms a bounded retry
/// instead of silently reporting success the hardware never reached.
async fn apply_backlight_level(
    display: &mut InkplateDriver,
    current_level: &mut u8,
    next_level: u8,
    retry_at_ms: &mut Option<u64>,
) {
    if !needs_level_apply(*current_level, next_level, retry_at_ms.is_some()) {
        return;
    }

    let now_ms = Instant::now().as_millis();
    match display.set_brightness_checked(next_level).await {
        Ok(true) => {
            if next_level == 0 {
                if let Err(error) = display.frontlight_off().await {
                    console::println!(
                        "display: frontlight_off failed level={} error={:?}",
                        next_level,
                        error
                    );
                    *retry_at_ms = Some(now_ms.saturating_add(FRONTLIGHT_APPLY_RETRY_MS));
                    return;
                }
            }
            *current_level = next_level;
            *retry_at_ms = None;
        }
        Ok(false) => {
            console::println!(
                "display: frontlight_apply confirmed=false want={} current={}",
                next_level,
                *current_level
            );
            *retry_at_ms = Some(now_ms.saturating_add(FRONTLIGHT_APPLY_RETRY_MS));
        }
        Err(error) => {
            console::println!(
                "display: frontlight_apply failed want={} current={} error={:?}",
                next_level,
                *current_level,
                error
            );
            *retry_at_ms = Some(now_ms.saturating_add(FRONTLIGHT_APPLY_RETRY_MS));
        }
    }
}

/// Applies the committed baseline and clears any in-progress short-press
/// flash. A failed application converges through the retry deadline via
/// [`run_backlight_timeline`]'s idle re-apply.
pub(crate) async fn apply_baseline_level(
    state: &mut DisplayLoopState,
    display: &mut InkplateDriver,
) {
    state.backlight_cycle_start = None;
    apply_backlight_level(
        display,
        &mut state.backlight_level,
        state.frontlight_baseline,
        &mut state.backlight_retry_at_ms,
    )
    .await;
}

async fn apply_steady_level(state: &mut DisplayLoopState, display: &mut InkplateDriver, level: u8) {
    state.backlight_cycle_start = None;
    apply_backlight_level(
        display,
        &mut state.backlight_level,
        level,
        &mut state.backlight_retry_at_ms,
    )
    .await;
}

pub(crate) async fn open_calibration(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
) -> bool {
    if state.frontlight_calibration.is_some() {
        return false;
    }
    let session = FrontlightCalibrationSession::new(state.frontlight_baseline);
    if !presentation::show_frontlight_calibration(context, &mut state.presentation, session.preview)
        .await
    {
        return false;
    }
    state.frontlight_calibration = Some(session);
    apply_steady_level(state, &mut context.inkplate, session.preview).await;
    console::println!(
        "frontlight: calibration state=open original={} preview={}",
        session.original,
        session.preview,
    );
    true
}

pub(crate) async fn service_calibration(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
) -> bool {
    let effect = presentation::take_frontlight_calibration_effect(&mut state.presentation);
    if let Some(effect) = effect {
        match effect {
            FrontlightCalibrationEffect::Preview(level) => {
                let Some(session) = state.frontlight_calibration.as_mut() else {
                    return false;
                };
                session.preview = level;
                apply_steady_level(state, &mut context.inkplate, level).await;
                console::println!("frontlight: calibration state=preview level={}", level);
            }
            FrontlightCalibrationEffect::Commit(level) => {
                if state.frontlight_calibration.take().is_none() {
                    return false;
                }
                state.frontlight_baseline = level;
                apply_baseline_level(state, &mut context.inkplate).await;
                console::println!("frontlight: calibration state=committed baseline={}", level);
            }
            FrontlightCalibrationEffect::Cancel => {
                let Some(session) = state.frontlight_calibration.take() else {
                    return false;
                };
                apply_baseline_level(state, &mut context.inkplate).await;
                console::println!(
                    "frontlight: calibration state=cancelled restored={}",
                    session.original,
                );
            }
        }
        return true;
    }

    if state.frontlight_calibration.is_some()
        && !presentation::frontlight_calibration_active(&state.presentation)
    {
        let session = state
            .frontlight_calibration
            .take()
            .expect("session checked");
        apply_baseline_level(state, &mut context.inkplate).await;
        console::println!(
            "frontlight: calibration state=cancelled reason=overlay_closed restored={}",
            session.original,
        );
    }
    false
}

/// Earliest wakeup covering the flash fade steps and a pending apply retry.
/// `None` while idle at the baseline: no periodic work, no spin.
pub(super) fn next_deadline_ms(state: &DisplayLoopState, now_ms: u64) -> Option<u64> {
    let fade = fade_deadline_ms(
        fade_spec(state.frontlight_baseline),
        state.backlight_cycle_start.map(|start| start.as_millis()),
        state.backlight_level,
        now_ms,
    );
    application_deadline(fade, state.backlight_retry_at_ms)
}
