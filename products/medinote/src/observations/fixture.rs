//! Headless product consumers. Runtime UI owns these alongside Home, but Home
//! navigation never changes their sessions or the provider-owned request credits.

use super::{
    BatteryFields, BatteryStateSnapshot, EnvironmentFields, EnvironmentStateSnapshot,
    StateSnapshot, BATTERY_PROVIDER_ID, ENVIRONMENT_PROVIDER_ID,
};
use core::fmt;
use observation::field::FieldMask;
use observation::fixture::{FixtureSample, FixtureStatus, Optional, SampleMetadata};
use observation::ids::{OwnerId, ProviderId};

/// The existing OBSFIX wire contract allows at most five minutes of eligibility.
/// This is a session bound, independent of Home's one-minute sampling cadence.
pub const MAX_FIXTURE_VALIDITY_MS: u32 = 300_000;
/// Home uses its surface ID (1). Fixture identity is product-local and separate.
const FIXTURE_OWNER: OwnerId = OwnerId(0);
/// Network-control targets accept the complete NETCFG policy payload; the
/// default fixture-only image keeps the smaller command frame.
#[cfg(feature = "network-controls")]
pub const COMMAND_CAPACITY: usize = 1024;
#[cfg(not(feature = "network-controls"))]
pub const COMMAND_CAPACITY: usize = 320;
pub const RX_BYTES_PER_POLL: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixtureProvider {
    Shtc3,
    Adc,
}
impl FixtureProvider {
    pub const fn id(self) -> ProviderId {
        match self {
            Self::Shtc3 => ENVIRONMENT_PROVIDER_ID,
            Self::Adc => BATTERY_PROVIDER_ID,
        }
    }
}

pub use observation::fixture::command::FixtureMode;
pub type ParsedCommand = observation::fixture::command::ParsedCommand<FixtureProvider>;
pub fn parse_command(line: &[u8]) -> Option<ParsedCommand> {
    observation::fixture::command::parse_command(
        line,
        |name| match name {
            "SHTC3" => Some(FixtureProvider::Shtc3),
            "ADC" => Some(FixtureProvider::Adc),
            _ => None,
        },
        MAX_FIXTURE_VALIDITY_MS,
    )
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConsoleCommand {
    Ping,
    Sleep(super::sleep_fixture::SleepRequest),
    SleepStale(u64),
    Fixture(ParsedCommand),
    Rejected(ParsedCommand, FixtureStatus),
    /// Requests the Settings overlay -- Medinote has no touchscreen, so
    /// unlike Meditamer's own on-screen button, this is invoked
    /// programmatically over the same console byte stream every other
    /// command here already uses.
    UiSettings,
    /// Dismisses the active modal overlay (Settings today; any future one
    /// later), the `UiSettings` counterpart.
    UiClose,
    Invalid,
}

/// Incremental framing retains partial lines across ticks and discards an entire
/// oversized line. One global ID watermark includes terminal Busy/Full outcomes.
pub struct ConsoleInput {
    bytes: [u8; COMMAND_CAPACITY],
    used: usize,
    overflow: bool,
    last_id: u64,
}
impl Default for ConsoleInput {
    fn default() -> Self {
        Self {
            bytes: [0; COMMAND_CAPACITY],
            used: 0,
            overflow: false,
            last_id: 0,
        }
    }
}
impl ConsoleInput {
    pub fn push(&mut self, byte: u8) -> Option<ConsoleCommand> {
        self.push_with_handler(byte, |_| false)
    }

    /// Give a target its complete line without introducing another RX owner.
    pub fn push_with_handler(
        &mut self,
        byte: u8,
        handler: impl FnOnce(&[u8]) -> bool,
    ) -> Option<ConsoleCommand> {
        if byte != b'\r' && byte != b'\n' {
            if self.used < self.bytes.len() {
                self.bytes[self.used] = byte;
                self.used += 1;
            } else {
                self.overflow = true;
            }
            return None;
        }
        let used = core::mem::take(&mut self.used);
        if core::mem::take(&mut self.overflow) {
            return Some(ConsoleCommand::Invalid);
        }
        if used == 0 {
            return None;
        }
        let line = &self.bytes[..used];
        if line == b"PING" {
            return Some(ConsoleCommand::Ping);
        }
        if line == b"UISETTINGS" {
            return Some(ConsoleCommand::UiSettings);
        }
        if line == b"UICLOSE" {
            return Some(ConsoleCommand::UiClose);
        }
        if handler(line) {
            return None;
        }
        if let Some(request) = super::sleep_fixture::parse(line) {
            if request.id <= self.last_id {
                return Some(ConsoleCommand::SleepStale(request.id));
            }
            self.last_id = request.id;
            return Some(ConsoleCommand::Sleep(request));
        }
        let Some(command) = parse_command(line) else {
            return Some(ConsoleCommand::Invalid);
        };
        if command.mode == FixtureMode::Cancel {
            return Some(ConsoleCommand::Fixture(command));
        }
        if command.id <= self.last_id {
            return Some(ConsoleCommand::Rejected(command, FixtureStatus::StaleId));
        }
        self.last_id = command.id;
        Some(ConsoleCommand::Fixture(command))
    }
}

impl<S> StateSnapshot<S> {
    fn fixture_metadata(&self) -> SampleMetadata {
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
}
impl FixtureSample for EnvironmentStateSnapshot {
    type Fields = EnvironmentFields;
    const PROVIDER: ProviderId = ENVIRONMENT_PROVIDER_ID;
    const OWNER: OwnerId = FIXTURE_OWNER;
    const MAX_VALIDITY_MS: u32 = MAX_FIXTURE_VALIDITY_MS;
    fn fields() -> EnvironmentFields {
        EnvironmentFields::TEMPERATURE.union(EnvironmentFields::HUMIDITY)
    }
    fn metadata(self) -> SampleMetadata {
        self.fixture_metadata()
    }
    fn write_values(state: Option<Self>, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sample = state.filter(|state| state.last_sample_at.is_some());
        write!(
            f,
            " temperature_millicelsius={} humidity_millipercent={}",
            Optional(sample.map(|s| s.snapshot.temperature_millicelsius)),
            Optional(sample.map(|s| s.snapshot.humidity_millipercent))
        )
    }
}
impl FixtureSample for BatteryStateSnapshot {
    type Fields = BatteryFields;
    const PROVIDER: ProviderId = BATTERY_PROVIDER_ID;
    const OWNER: OwnerId = FIXTURE_OWNER;
    const MAX_VALIDITY_MS: u32 = MAX_FIXTURE_VALIDITY_MS;
    fn fields() -> BatteryFields {
        BatteryFields::VOLTAGE.union(BatteryFields::LEVEL)
    }
    fn metadata(self) -> SampleMetadata {
        self.fixture_metadata()
    }
    fn write_values(state: Option<Self>, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sample = state.filter(|state| state.last_sample_at.is_some());
        write!(
            f,
            " percent={} millivolts={}",
            Optional(sample.map(|s| s.snapshot.percent)),
            Optional(sample.map(|s| s.snapshot.millivolts))
        )
    }
}
