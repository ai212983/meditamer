use super::super::{input::gpio36::Gpio36Action, types::DisplayContext};
use super::{
    frontlight::{open_calibration, trigger_backlight_cycle},
    frontlight_policy::{arm_press, classify_release, PressArm, ReleaseOutcome, WakePress},
    state::DisplayLoopState,
};

pub(super) async fn handle_gpio36_action(
    action: Gpio36Action,
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
) {
    match action {
        Gpio36Action::Touch => {}
        Gpio36Action::WakeButtonPressed { t_ms, generation } => {
            // Press arms only: the short action fires on release and the
            // calibration opens at the hold deadline, so a long press never
            // flashes max.
            let snapshot = super::super::input::gpio36::load_wake_snapshot();
            match arm_press(t_ms, generation, snapshot.generation, snapshot.active) {
                PressArm::IgnoredStale => {
                    console::println!(
                        "input: gpio36 source=wake_button state=pressed_ignored reason=stale"
                    );
                }
                PressArm::Armed(press) => {
                    console::println!("input: gpio36 source=wake_button state=pressed");
                    state.wake_press = Some(press);
                }
            }
        }
        Gpio36Action::WakeButtonReleased { t_ms, generation } => {
            console::println!("input: gpio36 source=wake_button state=released");
            let press = state.wake_press.take();
            let Some(press) = press else { return };
            handle_wake_release(context, state, press, t_ms, generation).await;
        }
    }
}

async fn handle_wake_release(
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
    press: WakePress,
    released_ms: u64,
    release_generation: u32,
) {
    let snapshot = super::super::input::gpio36::load_wake_snapshot();
    match classify_release(
        press,
        released_ms,
        release_generation,
        snapshot.generation,
        snapshot.active,
    ) {
        ReleaseOutcome::Ignored => {
            console::println!("input: gpio36 wake_hold state=release_ignored reason=stale");
        }
        ReleaseOutcome::ConsumedLong => {
            console::println!("input: gpio36 wake_hold state=release_consumed");
        }
        ReleaseOutcome::ShortFlash => {
            trigger_backlight_cycle(
                &mut context.inkplate,
                &mut state.backlight_cycle_start,
                &mut state.backlight_level,
                &mut state.backlight_retry_at_ms,
            )
            .await;
        }
        ReleaseOutcome::LateLong => {
            // Delayed servicing: the release arrived at/after the deadline
            // without the deadline service having fired. Open calibration
            // exactly as a deadline-fired long press.
            let opened = open_calibration(context, state).await;
            console::println!(
                "input: gpio36 wake_hold state=long_late calibration={}",
                if opened { "opened" } else { "rejected" },
            );
        }
    }
}
