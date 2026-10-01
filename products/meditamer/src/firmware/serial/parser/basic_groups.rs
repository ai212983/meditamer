use super::super::commands::SerialCommand;
use super::basic;
use super::firmware;

pub(super) fn parse_fixture_command(line: &[u8]) -> Option<SerialCommand> {
    if let Some(command) = crate::firmware::observation_fixture::parse_panel_command(line) {
        return Some(SerialCommand::ObservationPanelCycle(command));
    }
    if let Some(command) = crate::firmware::observation_fixture::parse_command(line) {
        return Some(SerialCommand::ObservationFixture(command));
    }
    if let Some(command) = firmware::parse_firmware_command(line) {
        return Some(command);
    }
    #[cfg(feature = "ui-provider-fixture")]
    if basic::parse_ui_provider_fixture_step_command(line) {
        return Some(SerialCommand::UiProviderFixtureStep);
    }
    if let Some(target) = basic::parse_ui_cycle_step_command(line) {
        return Some(SerialCommand::UiCycleStep { target });
    }
    None
}

pub(super) fn parse_ble_command(line: &[u8]) -> Option<SerialCommand> {
    #[cfg(not(feature = "ble-foundation"))]
    let _ = line;
    #[cfg(feature = "ble-foundation")]
    if let Some((boot_generation, epoch)) = basic::parse_ble_phase1s_start_command(line) {
        return Some(SerialCommand::BlePhase1sStart {
            boot_generation,
            epoch,
        });
    }
    #[cfg(feature = "ble-foundation")]
    if basic::parse_ble_phase1s_status_command(line) {
        return Some(SerialCommand::BlePhase1sStatus);
    }
    #[cfg(feature = "ble-foundation")]
    if basic::parse_ble_probe_start_command(line) {
        return Some(SerialCommand::BleProbeStart);
    }
    #[cfg(feature = "ble-foundation")]
    if basic::parse_ble_probe_status_command(line) {
        return Some(SerialCommand::BleProbeStatus);
    }
    #[cfg(feature = "ble-foundation")]
    if let Some(command) = basic::parse_radio_handoff_command(line) {
        return Some(match command {
            basic::RadioHandoffCommand::Acquire {
                boot_generation,
                epoch,
            } => SerialCommand::RadioHandoffAcquire {
                boot_generation,
                epoch,
            },
            basic::RadioHandoffCommand::Release {
                boot_generation,
                epoch,
            } => SerialCommand::RadioHandoffRelease {
                boot_generation,
                epoch,
            },
            basic::RadioHandoffCommand::Status => SerialCommand::RadioHandoffStatus,
        });
    }
    None
}

pub(super) fn parse_repaint_and_trace_command(line: &[u8]) -> Option<SerialCommand> {
    if let Some(request_id) = basic::parse_repaint_command(line) {
        return Some(SerialCommand::Repaint { request_id });
    }
    if basic::parse_metrics_net_command(line) {
        return Some(SerialCommand::MetricsNet);
    }
    #[cfg(feature = "firmware-trace")]
    if let Some(operation) = match line {
        b"TRACE START" | b"UITRACE START" => Some(0),
        b"TRACE STOP" | b"UITRACE STOP" => Some(1),
        b"TRACE DUMP" | b"UITRACE DUMP" => Some(2),
        b"TRACE ARM" | b"UITRACE ARM" => Some(3),
        b"TRACE STATUS" | b"UITRACE STATUS" => Some(4),
        b"TRACE SELFTEST" => Some(5),
        _ => None,
    } {
        return Some(SerialCommand::Trace { operation });
    }
    if line == b"ACQWINDOW" {
        return Some(SerialCommand::AcquisitionWindow);
    }
    if basic::parse_touch_sched_reset_command(line) {
        return Some(SerialCommand::TouchSchedReset);
    }
    if let Some(operation) = basic::parse_scheduler_command(line) {
        return Some(SerialCommand::Scheduler { operation });
    }
    None
}

pub(super) fn parse_telemetry_command(line: &[u8]) -> Option<SerialCommand> {
    if basic::parse_telemetry_status_command(line) {
        return Some(SerialCommand::TelemetryStatus);
    }
    if let Some(operation) = basic::parse_telemetry_set_command(line) {
        return Some(SerialCommand::TelemetrySet { operation });
    }
    if basic::parse_metrics_command(line) {
        return Some(SerialCommand::Metrics);
    }
    if line == b"STACKSTATUS" {
        return Some(SerialCommand::StackStatus);
    }
    if line == b"CONSOLESTATS" {
        return Some(SerialCommand::ConsoleStats);
    }
    if line == b"MOUNTAINSTATUS" {
        return Some(SerialCommand::MountainStatus);
    }
    match line {
        b"DISPLAYPAUSE" => return Some(SerialCommand::DisplayPause { paused: None }),
        b"DISPLAYPAUSE ON" => return Some(SerialCommand::DisplayPause { paused: Some(true) }),
        b"DISPLAYPAUSE OFF" => {
            return Some(SerialCommand::DisplayPause {
                paused: Some(false),
            })
        }
        _ => {}
    }
    if basic::parse_ping_command(line) {
        return Some(SerialCommand::Ping);
    }
    if let Some(bytes) = basic::parse_allocator_alloc_probe_command(line) {
        return Some(SerialCommand::AllocatorAllocProbe { bytes });
    }
    None
}

pub(super) fn parse_state_time_command(line: &[u8]) -> Option<SerialCommand> {
    if basic::parse_diag_get_command(line) {
        return Some(SerialCommand::DiagGet);
    }
    if basic::parse_state_get_command(line) {
        return Some(SerialCommand::StateGet);
    }
    if let Some(operation) = basic::parse_state_set_command(line) {
        return Some(SerialCommand::StateSet { operation });
    }
    if let Some((kind, targets)) = basic::parse_state_diag_command(line) {
        return Some(SerialCommand::StateDiag { kind, targets });
    }
    if let Some(reply) = basic::parse_time_reply_command(line) {
        return Some(SerialCommand::TimeReply {
            version: reply.version,
            session: reply.session,
            nonce: reply.nonce,
            utc_epoch_seconds: reply.utc_epoch_seconds,
            offset_minutes: reply.offset_minutes,
        });
    }
    if basic::parse_timesync_command(line) {
        return Some(SerialCommand::TimeSync);
    }
    if basic::parse_timeget_command(line) {
        return Some(SerialCommand::TimeGet);
    }
    None
}
