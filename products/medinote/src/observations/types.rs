//! Product observation values shared by target providers and Home delivery.

use observation::field::FieldMask;
use observation::ids::{ProviderGeneration, ProviderId, Revision};
use observation::policy::Health;
use observation::time::Instant;

pub const ENVIRONMENT_PROVIDER_ID: ProviderId = ProviderId(1);
pub const BATTERY_PROVIDER_ID: ProviderId = ProviderId(2);
/// Both providers expose two fields from one combined acquisition.
pub const FIELDS: usize = 2;
/// One live Home subscription and one reserved fixture subscription per provider.
pub const SUBSCRIPTION_CAPACITY: usize = 2;

/// SHTC3's two fields. A combined conversion always measures both together,
/// so every acquisition this driver performs reports both bits regardless of
/// which were actually due.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvironmentFields(u32);

impl EnvironmentFields {
    pub const TEMPERATURE: EnvironmentFields = EnvironmentFields(1 << 0);
    pub const HUMIDITY: EnvironmentFields = EnvironmentFields(1 << 1);
    const KNOWN: u32 = Self::TEMPERATURE.0 | Self::HUMIDITY.0;
}

impl FieldMask for EnvironmentFields {
    const EMPTY: Self = EnvironmentFields(0);

    fn bits(self) -> u32 {
        self.0
    }

    fn from_bits_truncate(bits: u32) -> Self {
        EnvironmentFields(bits & Self::KNOWN)
    }
}

/// Board self-heating-corrected temperature and raw humidity, in the same
/// fixed-point units `runtime_ui` has always displayed
/// (`shtc3::Measurement`'s millicelsius/millipercent).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EnvironmentSnapshot {
    pub temperature_millicelsius: i32,
    pub humidity_millipercent: i32,
}

/// The ADC yields one raw reading; voltage and percent are both derived from
/// it, so an acquisition always reports both bits together, the same
/// combined-conversion shape the environmental provider's fields have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatteryFields(u32);

impl BatteryFields {
    pub const VOLTAGE: BatteryFields = BatteryFields(1 << 0);
    pub const LEVEL: BatteryFields = BatteryFields(1 << 1);
    const KNOWN: u32 = Self::VOLTAGE.0 | Self::LEVEL.0;
}

impl FieldMask for BatteryFields {
    const EMPTY: Self = BatteryFields(0);

    fn bits(self) -> u32 {
        self.0
    }

    fn from_bits_truncate(bits: u32) -> Self {
        BatteryFields(bits & Self::KNOWN)
    }
}

/// Preserves the exact units and scaling `runtime_ui` always used: raw ADC
/// millivolts times the board's fixed 3x divider ratio, then
/// `medinote::presentation::battery_percent_from_mv`'s linear 3.0V-4.12V
/// curve.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BatterySnapshot {
    pub millivolts: u32,
    pub percent: u8,
}

/// Latest provider state; a missing timestamp means no usable sample exists.
#[derive(Clone, Copy, Debug)]
pub struct StateSnapshot<S> {
    pub provider: ProviderId,
    pub generation: ProviderGeneration,
    pub revision: Revision,
    pub health: Health,
    pub last_attempt_at: Option<Instant>,
    pub last_sample_at: Option<Instant>,
    /// Revision of the last successful acquisition, retained across failures.
    /// Unlike timestamps, this distinguishes conversions in the same millisecond.
    pub last_sample_revision: Option<Revision>,
    pub snapshot: S,
}

impl<S> StateSnapshot<S> {
    pub fn sample_identity(&self) -> Option<(ProviderGeneration, Revision)> {
        Some((self.generation, self.last_sample_revision?))
    }
}

impl<S: Copy> StateSnapshot<S> {
    /// Both fields are acquired together; failed attempts retain sample identity.
    pub fn from_observation(
        state: &observation::state::ObservationState<S, FIELDS>,
        last_sample_revision: Option<Revision>,
    ) -> Self {
        Self {
            provider: state.provider,
            generation: state.generation,
            revision: state.revision,
            health: state.health,
            last_attempt_at: state.last_attempt_at,
            last_sample_at: state.field_timestamps.get(0),
            last_sample_revision,
            snapshot: state.snapshot,
        }
    }
}

impl Default for StateSnapshot<EnvironmentSnapshot> {
    fn default() -> Self {
        Self::from_observation(
            &observation::state::ObservationState::new(
                ENVIRONMENT_PROVIDER_ID,
                ProviderGeneration::INITIAL,
                EnvironmentSnapshot::default(),
            ),
            None,
        )
    }
}

impl Default for StateSnapshot<BatterySnapshot> {
    fn default() -> Self {
        Self::from_observation(
            &observation::state::ObservationState::new(
                BATTERY_PROVIDER_ID,
                ProviderGeneration::INITIAL,
                BatterySnapshot::default(),
            ),
            None,
        )
    }
}

pub type EnvironmentStateSnapshot = StateSnapshot<EnvironmentSnapshot>;
pub type BatteryStateSnapshot = StateSnapshot<BatterySnapshot>;
