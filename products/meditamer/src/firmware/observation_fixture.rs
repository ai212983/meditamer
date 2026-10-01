//! Bounded external observation commands and terminal evidence vocabulary.
//! Serial converts validity to an absolute expiry before nonblocking enqueue.
//! The existing UI authority owns admission/results; no task or reply queue.

use crate::firmware::battery::{BatteryFields, BatteryStateSnapshot, BATTERY_PROVIDER_ID};
use crate::firmware::environment::{
    EnvironmentFields, EnvironmentStateSnapshot, ENVIRONMENT_PROVIDER_ID,
};
use core::{cell::RefCell, fmt};
use embassy_sync::blocking_mutex::{raw::RawMutex, Mutex};
use observation::field::FieldMask;
#[cfg(test)]
use observation::ids::{OwnerGeneration, ProviderGeneration, Revision};
use observation::ids::{OwnerId, ProviderId};
use observation::time::Instant;

#[path = "observation_fixture/panel_wait.rs"]
pub(crate) mod panel_wait;

#[cfg(target_os = "none")]
pub(crate) static PERIODIC_CHANGED: embassy_sync::signal::Signal<
    embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
    (),
> = embassy_sync::signal::Signal::new();

#[cfg(target_os = "none")]
pub(crate) fn notify_periodic_change() {
    PERIODIC_CHANGED.signal(());
}

#[cfg(target_os = "none")]
pub(crate) fn report_periodic<F: FieldMask, const N: usize>(
    event: observation::periodic::PeriodicEvent,
    provider: ProviderId,
    live: observation::demand::Demand<F, N>,
) {
    console::println!("{}", event.report(provider, live));
    notify_periodic_change();
}

pub(crate) const MAX_FIXTURE_VALIDITY_MS: u32 =
    crate::firmware::config::BATTERY_INTERVAL_SECONDS * 1_000;

/// Product-local provider identity, distinct from Medinote's numbering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FixtureProvider {
    Battery,
    Bme688,
}

impl FixtureProvider {
    pub(crate) const fn id(self) -> ProviderId {
        match self {
            Self::Battery => BATTERY_PROVIDER_ID,
            Self::Bme688 => ENVIRONMENT_PROVIDER_ID,
        }
    }
}

pub(crate) use observation::fixture::{
    FixtureRequest, FixtureResult, FixtureSample, FixtureStatus, Optional, SampleMetadata,
};
impl FixtureSample for BatteryStateSnapshot {
    type Fields = BatteryFields;
    const PROVIDER: ProviderId = FixtureProvider::Battery.id();
    const OWNER: OwnerId = OwnerId(1);
    const MAX_VALIDITY_MS: u32 = MAX_FIXTURE_VALIDITY_MS;
    fn fields() -> Self::Fields {
        BatteryFields::LEVEL
    }
    fn metadata(self) -> SampleMetadata {
        SampleMetadata {
            provider: self.provider,
            generation: self.generation,
            revision: self.revision,
            health: self.health,
            last_attempt_at: self.last_attempt_at,
            last_sample_at: self.last_sample_at,
            last_sample_revision: self.last_sample_revision,
        }
    }
    fn write_values(state: Option<Self>, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            " percent={}",
            Optional(state.and_then(|state| state.last_sample_at.map(|_| state.snapshot.percent)))
        )
    }
}

impl FixtureSample for EnvironmentStateSnapshot {
    type Fields = EnvironmentFields;
    const PROVIDER: ProviderId = FixtureProvider::Bme688.id();
    const OWNER: OwnerId = OwnerId(1);
    const MAX_VALIDITY_MS: u32 = MAX_FIXTURE_VALIDITY_MS;
    fn fields() -> Self::Fields {
        EnvironmentFields::TEMPERATURE.union(EnvironmentFields::HUMIDITY)
    }
    fn metadata(self) -> SampleMetadata {
        SampleMetadata {
            provider: self.provider,
            generation: self.generation,
            revision: self.revision,
            health: self.health,
            last_attempt_at: self.last_attempt_at,
            last_sample_at: self.last_sample_at,
            last_sample_revision: self.last_sample_revision,
        }
    }
    fn write_values(state: Option<Self>, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sample = state.filter(|state| state.last_sample_at.is_some());
        write!(
            f,
            " temperature_centidegrees={} humidity_millipercent={} sht45_temperature_centidegrees={} sht45_humidity_millipercent={}",
            Optional(sample.map(|state| state.snapshot.onboard.temperature_centidegrees)),
            Optional(sample.map(|state| state.snapshot.onboard.humidity_millipercent)),
            Optional(sample.and_then(|state| state.snapshot.external.map(|reading| reading.temperature_centidegrees))),
            Optional(sample.and_then(|state| state.snapshot.external.map(|reading| reading.humidity_millipercent)))
        )
    }
}

pub(crate) use observation::fixture::command::FixtureMode;
pub(crate) type ParsedCommand = observation::fixture::command::ParsedCommand<FixtureProvider>;
pub(crate) fn parse_command(line: &[u8]) -> Option<ParsedCommand> {
    observation::fixture::command::parse_command(
        line,
        |name| match name {
            "BATTERY" => Some(FixtureProvider::Battery),
            "BME688" => Some(FixtureProvider::Bme688),
            _ => None,
        },
        MAX_FIXTURE_VALIDITY_MS,
    )
}

/// The UI owns periodic admission and repaint as one operation. The ordinary
/// periodic grammar stays shared; only Inkplate accepts this trailing action.
pub(crate) fn parse_panel_command(line: &[u8]) -> Option<ParsedCommand> {
    let line = core::str::from_utf8(line).ok()?.trim_ascii();
    let prefix = line.strip_suffix(" REPAINT")?;
    let command = parse_command(prefix.as_bytes())?;
    matches!(command.mode, FixtureMode::Periodic { .. }).then_some(command)
}

pub(crate) fn parse_repaint_request(line: &[u8]) -> Option<Option<u64>> {
    let mut tokens = core::str::from_utf8(line).ok()?.split_ascii_whitespace();
    if !matches!(tokens.next()?, "REPAINT" | "REFRESH") {
        return None;
    }
    let Some(token) = tokens.next() else {
        return Some(None);
    };
    if !token.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let id = token.parse::<u64>().ok()?;
    (id != 0 && tokens.next().is_none()).then_some(Some(id))
}

/// One deferred command, separate from the provider's two observation credits.
/// The UI polls this mailbox; no async channel, task or waker is needed. Every
/// valid new ID is consumed before Busy rejection, so a terminal input result
/// cannot later alias an admitted request with the same ID.
pub(crate) struct CommandMailbox<M: RawMutex> {
    state: Mutex<M, RefCell<MailboxState>>,
}

struct MailboxState {
    last_request_id: u64,
    pending: Option<(FixtureProvider, FixtureRequest, FixtureMode, bool)>,
}

impl<M: RawMutex> CommandMailbox<M> {
    pub(crate) const fn new() -> Self {
        Self {
            state: Mutex::new(RefCell::new(MailboxState {
                last_request_id: 0,
                pending: None,
            })),
        }
    }

    pub(crate) fn try_enqueue_mode(
        &self,
        provider: FixtureProvider,
        request: FixtureRequest,
        mode: FixtureMode,
    ) -> Result<(), FixtureStatus> {
        self.enqueue_mode(provider, request, mode, false)
    }

    fn enqueue_mode(
        &self,
        provider: FixtureProvider,
        request: FixtureRequest,
        mode: FixtureMode,
        panel: bool,
    ) -> Result<(), FixtureStatus> {
        self.state.lock(|cell| {
            let mut state = cell.borrow_mut();
            if request.id == 0 {
                return Err(FixtureStatus::Invalid);
            }
            if mode != FixtureMode::Cancel && request.id <= state.last_request_id {
                return Err(FixtureStatus::StaleId);
            }
            if mode != FixtureMode::Cancel {
                state.last_request_id = request.id;
            }
            if state.pending.is_some() {
                return Err(FixtureStatus::Busy);
            }
            state.pending = Some((provider, request, mode, panel));
            Ok(())
        })
    }

    #[cfg(test)]
    fn try_enqueue(
        &self,
        provider: FixtureProvider,
        request: FixtureRequest,
    ) -> Result<(), FixtureStatus> {
        self.try_enqueue_mode(provider, request, FixtureMode::Once)
    }
    #[cfg(test)]
    fn try_receive(&self, provider: FixtureProvider) -> Option<FixtureRequest> {
        self.try_receive_mode(provider).map(|(request, _)| request)
    }
    /// Transfer the original absolute deadline while retaining the ID watermark.
    pub(crate) fn try_receive_mode(
        &self,
        provider: FixtureProvider,
    ) -> Option<(FixtureRequest, FixtureMode)> {
        self.state.lock(|cell| {
            let mut state = cell.borrow_mut();
            if state
                .pending
                .is_some_and(|(pending, _, _, panel)| pending == provider && !panel)
            {
                state
                    .pending
                    .take()
                    .map(|(_, request, mode, _)| (request, mode))
            } else {
                None
            }
        })
    }

    fn receive_panel(&self) -> Option<(FixtureProvider, FixtureRequest, FixtureMode)> {
        self.state.lock(|cell| {
            let mut state = cell.borrow_mut();
            if state.pending.is_some_and(|(_, _, _, panel)| panel) {
                state
                    .pending
                    .take()
                    .map(|(provider, request, mode, _)| (provider, request, mode))
            } else {
                None
            }
        })
    }
}

#[cfg(target_os = "none")]
static COMMANDS: CommandMailbox<embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex> =
    CommandMailbox::new();

#[cfg(target_os = "none")]
pub(crate) fn try_receive(provider: FixtureProvider) -> Option<(FixtureRequest, FixtureMode)> {
    COMMANDS.try_receive_mode(provider)
}

#[cfg(target_os = "none")]
pub(crate) fn try_receive_panel() -> Option<(FixtureProvider, FixtureRequest, FixtureMode)> {
    COMMANDS.receive_panel()
}

#[cfg(target_os = "none")]
pub(crate) fn enqueue_panel(command: ParsedCommand) {
    let request = command.at(Instant(embassy_time::Instant::now().as_millis()));
    let result = COMMANDS
        .enqueue_mode(command.provider, request, command.mode, true)
        .and_then(|()| {
            if crate::firmware::config::APP_EVENTS
                .try_send(crate::firmware::types::AppEvent::ObservationPanelCycle)
                .is_ok()
            {
                Ok(())
            } else {
                COMMANDS.receive_panel();
                Err(FixtureStatus::Busy)
            }
        });
    match result {
        Ok(()) => console::println!(
            "OBSPER QUEUED id={} provider={}",
            request.id,
            command.provider.id().0
        ),
        Err(status) => console::println!("PANEL_FIXTURE END id={} status={:?}", request.id, status),
    }
}

#[cfg(target_os = "none")]
pub(crate) fn log_result<S: FixtureSample>(result: FixtureResult<S>) {
    console::println!("{}", result);
}

#[cfg(target_os = "none")]
pub(crate) fn enqueue(command: ParsedCommand) {
    let request = command.at(Instant(embassy_time::Instant::now().as_millis()));
    let queued = COMMANDS.try_enqueue_mode(command.provider, request, command.mode);
    if queued.is_ok() {
        crate::firmware::display::wake();
    }
    match queued {
        Ok(()) => console::println!(
            "{} QUEUED id={} provider={}",
            command.mode.prefix(),
            request.id,
            command.provider.id().0
        ),
        Err(status) => console::println!(
            "{} RESULT id={} provider={} status={:?}",
            command.mode.prefix(),
            request.id,
            command.provider.id().0,
            status
        ),
    }
}

#[cfg(target_os = "none")]
pub(crate) fn route_command<S: FixtureSample, const N: usize>(
    command: (FixtureRequest, FixtureMode),
    control: &observation::periodic::DemandControl<
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        S::Fields,
        N,
    >,
    now: Instant,
    epoch: u32,
    once_pending: bool,
) -> Option<FixtureRequest> {
    let (request, mode) = command;
    match control.command(mode, request, S::fields(), now, epoch, once_pending) {
        Ok(request) => request,
        Err(status) => {
            console::println!(
                "{} RESULT id={} provider={} status={:?}",
                mode.prefix(),
                request.id,
                S::PROVIDER.0,
                status
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::firmware::battery::BatterySnapshot;
    type BatteryResult = super::FixtureResult<BatteryStateSnapshot>;
    use super::*;
    use embassy_sync::blocking_mutex::raw::NoopRawMutex;
    use observation::policy::Health;

    #[test]
    fn panel_command_keeps_periodic_bounds_and_uses_the_reserved_mailbox() {
        let command = parse_panel_command(b"OBSPER BME688 42 60000 150000 REPAINT").unwrap();
        assert_eq!(command.id, 42);
        for line in [
            "OBSFIX BME688 42 1000 REPAINT",
            "OBSPER BATTERY 42 CANCEL REPAINT",
            "OBSPER BME688 42 59999 150000 REPAINT",
            "OBSPER BME688 42 60000 150000 REPAINT extra",
        ] {
            assert!(parse_panel_command(line.as_bytes()).is_none());
        }
        let mailbox: CommandMailbox<NoopRawMutex> = CommandMailbox::new();
        let request = command.at(Instant(10));
        mailbox
            .enqueue_mode(command.provider, request, command.mode, true)
            .unwrap();
        assert!(mailbox.try_receive_mode(command.provider).is_none());
        assert_eq!(
            mailbox.try_enqueue(
                FixtureProvider::Battery,
                FixtureRequest { id: 43, ..request }
            ),
            Err(FixtureStatus::Busy)
        );
        assert_eq!(
            mailbox.receive_panel(),
            Some((command.provider, request, command.mode))
        );
        assert!(mailbox.receive_panel().is_none());
        assert_eq!(
            mailbox.enqueue_mode(command.provider, request, command.mode, true),
            Err(FixtureStatus::StaleId)
        );
    }

    #[test]
    fn provider_routing_shares_one_mailbox_and_id_watermark() {
        let mailbox: CommandMailbox<NoopRawMutex> = CommandMailbox::new();
        let parsed = parse_command(b"OBSFIX BME688 1 100").unwrap();
        assert_eq!(parsed.provider, FixtureProvider::Bme688);
        let request = parsed.at(Instant(10));
        mailbox.try_enqueue(parsed.provider, request).unwrap();
        assert!(mailbox.try_receive(FixtureProvider::Battery).is_none());
        assert_eq!(
            mailbox.try_enqueue(
                FixtureProvider::Battery,
                FixtureRequest { id: 2, ..request }
            ),
            Err(FixtureStatus::Busy)
        );
        assert_eq!(mailbox.try_receive(FixtureProvider::Bme688), Some(request));
        assert_eq!(
            mailbox.try_enqueue(
                FixtureProvider::Battery,
                FixtureRequest { id: 2, ..request }
            ),
            Err(FixtureStatus::StaleId)
        );
    }

    #[test]
    fn parse_accepts_decimal_boundaries_and_rejects_invalid_commands() {
        for (line, id, validity_ms) in [
            (b"OBSFIX BATTERY 1 1".as_slice(), 1, 1),
            (
                b"  OBSFIX\tBATTERY 18446744073709551615 300000\r\n".as_slice(),
                u64::MAX,
                MAX_FIXTURE_VALIDITY_MS,
            ),
        ] {
            assert_eq!(
                parse_command(line),
                Some(ParsedCommand {
                    mode: FixtureMode::Once,
                    provider: FixtureProvider::Battery,
                    id,
                    validity_ms
                })
            );
        }
        for line in [
            b"OBSFIX BATTERY 0 100".as_slice(),
            b"OBSFIX BATTERY 1 0",
            b"OBSFIX BATTERY 1 300001",
            b"OBSFIX BATTERY 18446744073709551616 1",
            b"OBSFIX BATTERY 1 4294967296",
            b"OBSFIX BATTERY +1 5",
            b"OBSFIX BATTERY 1 +5",
            b"OBSFIX BATTERY -1 5",
            b"OBSFIX BATTERY 1 5 extra",
            b"OBSFIX BATTERY 1",
            b"OBSFIX SHTC3 1 5",
            b"obsfix BATTERY 1 5",
            b"OBSFIX BATTERY 1 5\xff",
        ] {
            assert_eq!(parse_command(line), None, "{line:?}");
        }
    }

    #[test]
    fn mailbox_rejects_full_without_replacing_absolute_expiry() {
        let mailbox = CommandMailbox::<NoopRawMutex>::new();
        let parsed = parse_command(b"OBSFIX BATTERY 7 100").unwrap();
        let request = parsed.at(Instant(20));
        assert_eq!(request.expires_at, Instant(120));
        mailbox
            .try_enqueue(FixtureProvider::Battery, request)
            .unwrap();
        let second = FixtureRequest {
            id: 8,
            expires_at: Instant(600),
        };
        assert_eq!(
            mailbox.try_enqueue(FixtureProvider::Battery, second),
            Err(FixtureStatus::Busy)
        );
        assert_eq!(mailbox.try_receive(FixtureProvider::Battery), Some(request));
        assert_eq!(mailbox.try_receive(FixtureProvider::Battery), None);
        assert_eq!(
            mailbox.try_enqueue(FixtureProvider::Battery, second),
            Err(FixtureStatus::StaleId)
        );
        let third = FixtureRequest { id: 9, ..second };
        mailbox
            .try_enqueue(FixtureProvider::Battery, third)
            .unwrap();
        assert_eq!(mailbox.try_receive(FixtureProvider::Battery), Some(third));
        assert_eq!(
            parsed.at(Instant(u64::MAX - 1)).expires_at,
            Instant(u64::MAX)
        );
    }

    #[test]
    fn mailbox_consumes_rejected_ids_and_handles_zero_and_max_without_wrap() {
        let mailbox = CommandMailbox::<NoopRawMutex>::new();
        let request = |id| FixtureRequest {
            id,
            expires_at: Instant(500),
        };
        assert_eq!(
            mailbox.try_enqueue(FixtureProvider::Battery, request(0)),
            Err(FixtureStatus::Invalid)
        );
        mailbox
            .try_enqueue(FixtureProvider::Battery, request(100))
            .unwrap();
        assert_eq!(
            mailbox.try_enqueue(FixtureProvider::Battery, request(200)),
            Err(FixtureStatus::Busy)
        );
        assert_eq!(
            mailbox.try_receive(FixtureProvider::Battery),
            Some(request(100))
        );
        assert_eq!(
            mailbox.try_enqueue(FixtureProvider::Battery, request(200)),
            Err(FixtureStatus::StaleId)
        );
        mailbox
            .try_enqueue(FixtureProvider::Battery, request(201))
            .unwrap();
        assert_eq!(
            mailbox.try_receive(FixtureProvider::Battery),
            Some(request(201))
        );
        mailbox
            .try_enqueue(FixtureProvider::Battery, request(u64::MAX))
            .unwrap();
        assert_eq!(
            mailbox.try_receive(FixtureProvider::Battery),
            Some(request(u64::MAX))
        );
        for id in [1, 200, u64::MAX] {
            assert_eq!(
                mailbox.try_enqueue(FixtureProvider::Battery, request(id)),
                Err(FixtureStatus::StaleId)
            );
        }
        assert_eq!(
            mailbox.try_enqueue(FixtureProvider::Battery, request(0)),
            Err(FixtureStatus::Invalid)
        );
    }

    #[test]
    fn result_wire_fields_preserve_sample_identity_and_plain_optional_values() {
        let result = BatteryResult {
            id: 42,
            owner_generation: OwnerGeneration(3),
            admitted_at: Some(Instant(10)),
            expires_at: Instant(100),
            status: FixtureStatus::Sampled,
            baseline: Some((ProviderGeneration(0), Revision(8))),
            state: Some(BatteryStateSnapshot {
                provider: BATTERY_PROVIDER_ID,
                generation: ProviderGeneration(0),
                revision: Revision(9),
                health: Health::Ok,
                last_attempt_at: Some(Instant(15)),
                last_sample_at: Some(Instant(15)),
                last_sample_revision: Some(Revision(9)),
                snapshot: BatterySnapshot { percent: 84 },
            }),
        };
        assert_eq!(format!("{result}"), "OBSFIX RESULT id=42 provider=2 fields=1 status=Sampled owner_generation=3 admitted_at_ms=10 expires_at_ms=100 generation=0 revision=9 health=Ok last_attempt_at_ms=15 last_sample_at_ms=15 last_sample_revision=9 percent=84 baseline_generation=0 baseline_sample_revision=8");
        let missing = BatteryResult {
            status: FixtureStatus::Busy,
            admitted_at: None,
            state: None,
            baseline: None,
            ..result
        };
        assert_eq!(format!("{missing}"), "OBSFIX RESULT id=42 provider=2 fields=1 status=Busy owner_generation=3 admitted_at_ms=none expires_at_ms=100 generation=none revision=none health=none last_attempt_at_ms=none last_sample_at_ms=none last_sample_revision=none percent=none baseline_generation=none baseline_sample_revision=none");
        let no_sample = BatteryResult {
            state: Some(BatteryStateSnapshot {
                last_sample_at: None,
                last_sample_revision: None,
                ..result.state.unwrap()
            }),
            ..result
        };
        assert!(format!("{no_sample}").contains("last_sample_revision=none percent=none"));
    }

    #[test]
    fn terminal_status_spelling_is_stable() {
        for (status, expected) in [
            (FixtureStatus::Sampled, "Sampled"),
            (FixtureStatus::Expired, "Expired"),
            (FixtureStatus::Cancelled, "Cancelled"),
            (FixtureStatus::Restarted, "Restarted"),
            (FixtureStatus::Busy, "Busy"),
            (FixtureStatus::Invalid, "Invalid"),
            (FixtureStatus::StaleId, "StaleId"),
            (FixtureStatus::GenerationExhausted, "GenerationExhausted"),
            (FixtureStatus::Full, "Full"),
            (FixtureStatus::Closed, "Closed"),
            (FixtureStatus::Unavailable, "Unavailable"),
        ] {
            assert_eq!(format!("{status:?}"), expected);
        }
    }
    #[test]
    fn cancellation_preserves_global_new_id_watermark() {
        let mailbox = CommandMailbox::<NoopRawMutex>::new();
        let command = parse_command(b"OBSPER BATTERY 42 60000 150000").unwrap();
        assert_eq!(
            mailbox.try_enqueue_mode(command.provider, command.at(Instant(0)), command.mode),
            Ok(())
        );
        assert!(mailbox.try_receive_mode(command.provider).is_some());
        let newer = parse_command(b"OBSFIX BME688 43 100").unwrap();
        assert_eq!(
            mailbox.try_enqueue_mode(newer.provider, newer.at(Instant(0)), newer.mode),
            Ok(())
        );
        assert!(mailbox.try_receive_mode(newer.provider).is_some());
        assert_eq!(
            mailbox.try_enqueue_mode(
                command.provider,
                command.at(Instant(0)),
                FixtureMode::Cancel
            ),
            Ok(())
        );
        assert_eq!(
            mailbox.try_receive_mode(command.provider).unwrap().1,
            FixtureMode::Cancel
        );
        assert_eq!(
            mailbox.try_enqueue_mode(newer.provider, newer.at(Instant(0)), newer.mode),
            Err(FixtureStatus::StaleId)
        );
    }
    #[test]
    fn repaint_optional_identity_is_exact_and_nonzero() {
        assert_eq!(parse_repaint_request(b"REPAINT"), Some(None));
        assert_eq!(
            parse_repaint_request(b"REFRESH 18446744073709551615"),
            Some(Some(u64::MAX))
        );
        for input in [
            "REPAINT 0",
            "REPAINT +1",
            "REPAINT 1 trailing",
            "REPAINT 18446744073709551616",
            "REPAINTX 1",
        ] {
            assert!(parse_repaint_request(input.as_bytes()).is_none());
        }
    }
}
