//! Logical CPU sensor. A persistent product diagnostics subscription keeps
//! five-second windows available across navigation, including serial-only use.
//! The common provider loop owns acquisition, freshness, revisions and health;
//! UI delivery is independently limited to fifteen seconds. No one-shot ingress
//! is allocated because this provider currently exposes only cached diagnostics.
use cpu_load::{History, Snapshot};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, watch::Watch};
use embassy_time::{Instant as Clock, Timer};
use observation::{
    demand::aggregate_demand,
    field::FieldMask,
    ids::{OwnerGeneration, OwnerId, ProviderGeneration, ProviderId, Revision, SubscriptionSlot},
    policy::{Health, InitialPolicy},
    runtime::{AcquisitionDriver, ProviderLoop, StepOutcome},
    schedule::FieldTimestamps,
    subscription::{ObservationSubscription, SubscriptionBook, SubscriptionKey},
    time::{Duration, Instant},
};

pub(crate) const PROVIDER_ID: ProviderId = ProviderId(3);
const INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fields(u32);
impl Fields {
    pub(crate) const BOTH: Self = Self(3);
}
impl FieldMask for Fields {
    const EMPTY: Self = Self(0);
    fn bits(self) -> u32 {
        self.0
    }
    fn from_bits_truncate(bits: u32) -> Self {
        Self(bits & 3)
    }
}

/// Both core counters are latched together under the measurement lock.
#[derive(Clone, Copy)]
pub(crate) struct State {
    pub provider: ProviderId,
    pub generation: ProviderGeneration,
    pub revision: Revision,
    pub health: Health,
    pub last_attempt_at: Option<Instant>,
    pub timestamps: FieldTimestamps<2>,
    pub snapshot: Snapshot,
}

static STATE: Watch<CriticalSectionRawMutex, State, 0> = Watch::new();
pub(crate) fn latest() -> Option<State> {
    STATE.try_get()
}

pub(crate) fn diagnostics_subscription() -> ObservationSubscription<Fields> {
    ObservationSubscription {
        key: SubscriptionKey {
            provider: PROVIDER_ID,
            owner: OwnerId(0), // Product lifetime, independent of shell surfaces.
            owner_generation: OwnerGeneration(0),
            slot: SubscriptionSlot(0),
        },
        fields: Fields::BOTH,
        max_age: INTERVAL,
        initial_policy: InitialPolicy::RefreshIfStale,
        min_delivery_interval: Duration::from_secs(15),
        expires_at: None,
    }
}

struct Driver {
    history: History,
    started: bool,
}
impl AcquisitionDriver<Fields, Snapshot> for Driver {
    type Error = ();
    async fn acquire(&mut self, _: Fields) -> Result<(Fields, Snapshot), ()> {
        if !self.started {
            // Discard boot's partial window. Only this driver samples counters.
            let _ = cpu_load::sample();
            self.started = true;
            Timer::after_millis(5_000).await;
        }
        let readings = cpu_load::sample();
        if readings.iter().any(|r| r.is_none_or(|r| r.elapsed_us == 0)) {
            return Err(());
        }
        self.history
            .record(Clock::now().as_millis() as u32, readings);
        Ok((Fields::BOTH, self.history.snapshot))
    }
    async fn cancel(&mut self) -> Result<(), ()> {
        self.started = false;
        self.history = History::new();
        Ok(())
    }
    fn min_acquisition_interval(&self) -> Duration {
        INTERVAL
    }
}

fn now() -> Instant {
    Instant(Clock::now().as_millis())
}

#[embassy_executor::task]
pub async fn acquisition_task() {
    let mut subscriptions = SubscriptionBook::<Fields, 1>::new();
    subscriptions
        .upsert(diagnostics_subscription())
        .expect("CPU diagnostics slot");
    let mut provider = ProviderLoop::<Fields, Snapshot, Driver, 0, 2>::new(
        Driver {
            history: History::new(),
            started: false,
        },
        PROVIDER_ID,
        ProviderGeneration::INITIAL,
        History::new().snapshot,
    );
    loop {
        let demand = aggregate_demand(&subscriptions, PROVIDER_ID, now());
        let outcome = provider.step(&demand, now).await;
        let state = provider.state();
        STATE.sender().send(State {
            provider: state.provider,
            generation: state.generation,
            revision: state.revision,
            health: state.health,
            last_attempt_at: state.last_attempt_at,
            timestamps: state.field_timestamps,
            snapshot: state.snapshot,
        });
        crate::firmware::display::wake();
        match outcome {
            StepOutcome::Idle {
                next_wake: Some(at),
            } => {
                Timer::at(Clock::from_millis(at.0)).await;
            }
            StepOutcome::RevisionExhausted => {
                console::println!("CPU_OBSERVATION_STOP reason=RevisionExhausted");
                core::future::pending::<()>().await;
            }
            StepOutcome::Idle { next_wake: None } | StepOutcome::Suspended => {
                core::future::pending::<()>().await;
            }
            _ => {} // Re-evaluate scheduling/backoff using the common loop.
        }
    }
}
