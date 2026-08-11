//! Serial command vocabulary: the parsed command model, its app-state mapping, and its event/response mapping.

mod app_state;
mod mapping;
mod types;

pub(super) use app_state::app_state_command_for_serial;
pub(super) use mapping::serial_command_event_and_responses;
pub(super) use types::{
    SchedulerOperation, SdWaitTarget, SerialCommand, StateSetOperation, TelemetryDomain,
    TelemetrySetOperation,
};
