//! Controller-independent Medinote rotation input.
//!
//! The target translates physical KEY edges and BLE button edges into
//! [`RotationInput`] values. This module owns the product mapping and the
//! strict timestamp adapter into Hourglass [`RotationCommand`] values; it
//! has no Embassy, GPIO, or BLE-host dependency.

use hourglass::fixed::Fx;
use hourglass::rotation::{RotationCommand, RotationCommandKind, TimestampUs};

/// CheerTok's captured keyboard usages (HID Usage Tables, Keyboard page).
const KEYBOARD_PAGE_UP: u8 = 0x4b;
const KEYBOARD_PAGE_DOWN: u8 = 0x4e;

/// Product-neutral button indices published for the two CheerTok controls.
pub const CLOCKWISE_BUTTON: u8 = 0;
pub const COUNTERCLOCKWISE_BUTTON: u8 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyboardReportError {
    Truncated,
}

/// Convert the captured eight-byte boot-keyboard-shaped report into the
/// button mask consumed by the shared BLE [`InputPublisher`]. Modifier and
/// reserved bytes are ignored; all six key slots are considered so rollover
/// and simultaneous Page Up/Page Down remain complete state, not deltas.
pub fn cheertok_keyboard_buttons(report: &[u8]) -> Result<u16, KeyboardReportError> {
    let key_slots = report.get(2..8).ok_or(KeyboardReportError::Truncated)?;
    let mut buttons = 0u16;
    for usage in key_slots {
        match *usage {
            KEYBOARD_PAGE_DOWN => buttons |= 1 << CLOCKWISE_BUTTON,
            KEYBOARD_PAGE_UP => buttons |= 1 << COUNTERCLOCKWISE_BUTTON,
            _ => {}
        }
    }
    Ok(buttons)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RotationControl {
    Clockwise,
    Counterclockwise,
}

/// One physical control edge on the target's absolute monotonic clock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RotationInput {
    pub control: RotationControl,
    pub pressed: bool,
    pub ticks_us: u64,
}

/// Map one shared BLE button edge into Medinote's product controls. Unknown
/// buttons are consumed but intentionally produce no product input.
pub const fn rotation_input_from_button(
    button: u8,
    pressed: bool,
    ticks_us: u64,
) -> Option<RotationInput> {
    let control = match button {
        CLOCKWISE_BUTTON => RotationControl::Clockwise,
        COUNTERCLOCKWISE_BUTTON => RotationControl::Counterclockwise,
        _ => return None,
    };
    Some(RotationInput {
        control,
        pressed,
        ticks_us,
    })
}

/// Add the local KEY edge to the remote input batch and order both sources by
/// the target's shared monotonic clock. The bounded vector remains owned by
/// the caller so this helper cannot alter its capacity or overflow policy.
pub fn merge_rotation_inputs<const CAPACITY: usize>(
    inputs: &mut heapless::Vec<RotationInput, CAPACITY>,
    local: Option<RotationInput>,
) {
    if let Some(local) = local {
        let _ = inputs.push(local);
    }
    inputs.sort_unstable_by_key(|input| input.ticks_us);
}

/// Session-local adapter that guarantees every emitted command has a
/// strictly increasing timestamp. Callers sort concurrently collected KEY
/// and BLE inputs by `ticks_us` first; equal-clock ties are advanced by one
/// microsecond so the Hourglass model never rejects one as a duplicate.
pub struct OrderedRotationCommands {
    session_started_us: u64,
    last_at: Option<TimestampUs>,
}

impl OrderedRotationCommands {
    pub const fn new(session_started_us: u64) -> Self {
        Self {
            session_started_us,
            last_at: None,
        }
    }

    pub fn reset(&mut self, session_started_us: u64) {
        self.session_started_us = session_started_us;
        self.last_at = None;
    }

    /// Releases are part of the complete input stream but do not rotate a
    /// step-based V1 glass. A future continuous-control adapter can consume
    /// the same edge without changing the target/BLE boundary.
    /// An edge predating the session is stale and is discarded.
    pub fn command(&mut self, input: RotationInput) -> Option<RotationCommand> {
        if !input.pressed || input.ticks_us < self.session_started_us {
            return None;
        }
        let relative = input.ticks_us - self.session_started_us;
        let strictly_ordered = self
            .last_at
            .map_or(relative, |last| relative.max(last.0.saturating_add(1)));
        let at = TimestampUs(strictly_ordered);
        self.last_at = Some(at);
        let delta = match input.control {
            RotationControl::Clockwise => Fx::ratio(1, 4),
            RotationControl::Counterclockwise => -Fx::ratio(1, 4),
        };
        Some(RotationCommand {
            kind: RotationCommandKind::RotateBy(delta),
            at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_page_keys_publish_complete_button_state() {
        assert_eq!(
            cheertok_keyboard_buttons(&[0, 0, KEYBOARD_PAGE_DOWN, 0, 0, 0, 0, 0]),
            Ok(1 << CLOCKWISE_BUTTON)
        );
        assert_eq!(
            cheertok_keyboard_buttons(&[0, 0, KEYBOARD_PAGE_UP, KEYBOARD_PAGE_DOWN, 0, 0, 0, 0,]),
            Ok((1 << CLOCKWISE_BUTTON) | (1 << COUNTERCLOCKWISE_BUTTON))
        );
        assert_eq!(cheertok_keyboard_buttons(&[0; 8]), Ok(0));
    }

    #[test]
    fn malformed_keyboard_report_fails_closed() {
        assert_eq!(
            cheertok_keyboard_buttons(&[0; 7]),
            Err(KeyboardReportError::Truncated)
        );
    }

    #[test]
    fn page_down_is_clockwise_and_page_up_is_counterclockwise() {
        let mut commands = OrderedRotationCommands::new(1_000);
        let clockwise = commands
            .command(RotationInput {
                control: RotationControl::Clockwise,
                pressed: true,
                ticks_us: 1_100,
            })
            .unwrap();
        let counterclockwise = commands
            .command(RotationInput {
                control: RotationControl::Counterclockwise,
                pressed: true,
                ticks_us: 1_200,
            })
            .unwrap();
        assert_eq!(
            clockwise.kind,
            RotationCommandKind::RotateBy(Fx::ratio(1, 4))
        );
        assert_eq!(
            counterclockwise.kind,
            RotationCommandKind::RotateBy(-Fx::ratio(1, 4))
        );
    }

    #[test]
    fn releases_do_not_emit_step_commands() {
        let mut commands = OrderedRotationCommands::new(0);
        assert!(commands
            .command(RotationInput {
                control: RotationControl::Clockwise,
                pressed: false,
                ticks_us: 10,
            })
            .is_none());
    }

    #[test]
    fn input_predating_session_start_is_rejected_not_clamped_to_zero() {
        let mut commands = OrderedRotationCommands::new(10_000);
        assert!(commands
            .command(RotationInput {
                control: RotationControl::Clockwise,
                pressed: true,
                ticks_us: 9_999,
            })
            .is_none());
        assert!(commands
            .command(RotationInput {
                control: RotationControl::Clockwise,
                pressed: true,
                ticks_us: 10_000,
            })
            .is_some());
    }

    #[test]
    fn simultaneous_key_and_ble_presses_remain_strictly_ordered() {
        let mut commands = OrderedRotationCommands::new(10_000);
        let first = commands
            .command(RotationInput {
                control: RotationControl::Clockwise,
                pressed: true,
                ticks_us: 10_100,
            })
            .unwrap();
        let second = commands
            .command(RotationInput {
                control: RotationControl::Counterclockwise,
                pressed: true,
                ticks_us: 10_100,
            })
            .unwrap();
        assert_eq!(first.at, TimestampUs(100));
        assert_eq!(second.at, TimestampUs(101));
    }

    #[test]
    fn merge_orders_remote_before_local_even_when_drained_later() {
        let mut inputs: heapless::Vec<RotationInput, 17> = heapless::Vec::new();
        inputs
            .push(RotationInput {
                control: RotationControl::Counterclockwise,
                pressed: true,
                ticks_us: 10,
            })
            .unwrap();

        merge_rotation_inputs(
            &mut inputs,
            Some(RotationInput {
                control: RotationControl::Clockwise,
                pressed: true,
                ticks_us: 30,
            }),
        );

        assert_eq!(inputs[0].ticks_us, 10);
        assert_eq!(inputs[1].ticks_us, 30);
    }

    #[test]
    fn merge_keeps_equal_timestamp_inputs_strictly_orderable() {
        let mut inputs: heapless::Vec<RotationInput, 17> = heapless::Vec::new();
        inputs
            .push(RotationInput {
                control: RotationControl::Counterclockwise,
                pressed: true,
                ticks_us: 10,
            })
            .unwrap();

        merge_rotation_inputs(
            &mut inputs,
            Some(RotationInput {
                control: RotationControl::Clockwise,
                pressed: true,
                ticks_us: 10,
            }),
        );

        let mut commands = OrderedRotationCommands::new(0);
        let timestamps: heapless::Vec<_, 2> = inputs
            .into_iter()
            .filter_map(|input| commands.command(input))
            .map(|command| command.at)
            .collect();
        assert_eq!(timestamps.as_slice(), &[TimestampUs(10), TimestampUs(11)]);
    }
}
