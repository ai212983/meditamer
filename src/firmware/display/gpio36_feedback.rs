use super::super::{input::gpio36::Gpio36Action, types::DisplayContext};
use super::{frontlight::trigger_backlight_cycle, state::DisplayLoopState};

pub(super) async fn handle_gpio36_action(
    action: Gpio36Action,
    context: &mut DisplayContext,
    state: &mut DisplayLoopState,
) {
    match action {
        Gpio36Action::Touch => {}
        Gpio36Action::WakeButtonPressed => {
            console::println!("input: gpio36 source=wake_button state=pressed");
            trigger_backlight_cycle(
                &mut context.inkplate,
                &mut state.backlight_cycle_start,
                &mut state.backlight_level,
            )
            .await;
        }
        Gpio36Action::WakeButtonReleased => {
            console::println!("input: gpio36 source=wake_button state=released");
        }
    }
}
