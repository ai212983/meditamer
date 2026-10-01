use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::watch::Watch;
use observation::periodic::DemandControl;

use super::requests::RequestIngress;
use super::types::{EnvironmentFields, EnvironmentStateSnapshot, FIELDS};

/// Five-minute freshness and delivery, matching the plan's Phase 4 bullet
/// and the cadence `AppEvent::BatteryTick` already sampled BME688 at
/// (`crate::firmware::config::BATTERY_INTERVAL_SECONDS`) -- unchanged, just
/// no longer tied to that shared tick.
pub(crate) const ENVIRONMENT_SAMPLE_INTERVAL_S: u32 =
    crate::firmware::config::BATTERY_INTERVAL_SECONDS;

/// A failed initialization or sample retries starting from this floor,
/// doubling per `observation::runtime`'s backoff -- matching the IMU
/// acquisition task's `IMU_INIT_RETRY_MS` order of magnitude for "how soon
/// is it reasonable to try again", not the sensor's own timing.
pub(crate) const ENVIRONMENT_RETRY_FLOOR_MS: u32 = 2_000;

/// One live consumer and one bounded fixture consumer per provider. Ingress
/// credits bound queued plus provider-pending requests together, not separately.
pub(crate) const SUBSCRIPTION_CAPACITY: usize = 2;
pub(crate) const REQUEST_CAPACITY: usize = 2;
const STATE_RECEIVERS: usize = 1;

pub(crate) static ENVIRONMENT_DEMAND: DemandControl<
    CriticalSectionRawMutex,
    EnvironmentFields,
    FIELDS,
> = DemandControl::new();

pub(crate) static ENVIRONMENT_REQUESTS: RequestIngress<
    CriticalSectionRawMutex,
    EnvironmentFields,
    REQUEST_CAPACITY,
> = RequestIngress::new();

pub(crate) static ENVIRONMENT_STATE: Watch<
    CriticalSectionRawMutex,
    EnvironmentStateSnapshot,
    STATE_RECEIVERS,
> = Watch::new();
