use crate::firmware::app_state::AppStateCommand;
use crate::firmware::{input::gpio36::Gpio36Action, touch::types::TouchStatus};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiCycleTarget {
    Next,
    AnalogClock,
    AmbientView,
}

#[derive(Clone, Copy)]
pub enum AppEvent {
    TouchStatus(TouchStatus),
    Gpio36Action(Gpio36Action),
    ImuActionsReady,
    ObservationPanelCycle,
    ForceRepaint {
        request_id: Option<u64>,
    },
    UiCycleStep {
        ack_request_id: u16,
        target: UiCycleTarget,
    },
    #[cfg(feature = "ui-provider-fixture")]
    UiProviderFixtureStep {
        ack_request_id: u16,
    },
    ApplyAppStateCommand {
        command: AppStateCommand,
        ack_request_id: Option<u16>,
    },
}
