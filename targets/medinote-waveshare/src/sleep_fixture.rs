//! The UI owns admission and the normal sleep action. No second power owner.

use crate::{battery, environment};
use medinote::observations::sleep_fixture::SleepRequest;

#[derive(Clone, Copy)]
pub(crate) struct PendingSleep {
    pub request: SleepRequest,
    expires_at_ms: u64,
}

impl PendingSleep {
    pub(crate) fn new(request: SleepRequest) -> Self {
        Self {
            request,
            expires_at_ms: now().saturating_add(u64::from(request.validity_ms)),
        }
    }

    pub(crate) fn eligible(self) -> bool {
        now() < self.expires_at_ms
    }

    pub(crate) fn begin(self) -> Option<Self> {
        if !self.eligible() {
            self.end("Expired");
            return None;
        }
        console::println!(
            "OBSSLEEP BEGIN id={} mode={} at_ms={} expires_at_ms={}",
            self.request.id,
            self.request.mode.wire(),
            now(),
            self.expires_at_ms
        );
        if let Some(s) = environment::ENVIRONMENT_STATE.try_get() {
            console::println!("OBSSLEEP BASE id={} provider=ENVIRONMENT generation={} revision={} sampled_at_ms={:?}", self.request.id, s.generation.0, s.revision.0, s.last_sample_at.map(|t| t.0));
        }
        if let Some(s) = battery::BATTERY_STATE.try_get() {
            console::println!(
                "OBSSLEEP BASE id={} provider=BATTERY generation={} revision={} sampled_at_ms={:?}",
                self.request.id,
                s.generation.0,
                s.revision.0,
                s.last_sample_at.map(|t| t.0)
            );
        }
        Some(self)
    }

    pub(crate) fn end(self, status: &str) {
        let (environment, _) = environment::ENVIRONMENT_CONTROL.snapshot();
        let (battery, _) = battery::BATTERY_CONTROL.snapshot();
        console::println!(
            "OBSSLEEP END id={} status={} at_ms={} environment_id={} battery_id={}",
            self.request.id,
            status,
            now(),
            environment.id(),
            battery.id()
        );
        trace_demand(
            self.request.id,
            medinote::observations::ENVIRONMENT_PROVIDER_ID,
            &environment::ENVIRONMENT_DEMAND,
        );
        trace_demand(
            self.request.id,
            medinote::observations::BATTERY_PROVIDER_ID,
            &battery::BATTERY_DEMAND,
        );
    }

    pub(crate) fn entering(self) {
        console::println!("OBSSLEEP ENTER id={} at_ms={}", self.request.id, now());
    }
}

/// Read-only evidence after the UI has published both resumes and current Home
/// demand. Only provider actors can close/apply periodic overrides.
fn trace_demand<F: observation::field::FieldMask, const N: usize>(
    id: u64,
    provider: observation::ids::ProviderId,
    control: &observation::periodic::DemandControl<
        embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex,
        F,
        N,
    >,
) {
    use observation::fixture::Optional;
    let live = control.live();
    console::println!(
        "OBSSLEEP DEMAND id={} provider={} pending={} live_fields={} live_age0_ms={} live_age1_ms={}",
        id, provider.0, control.is_pending(), live.fields.bits(),
        Optional(live.max_age_at(0).map(|age| age.0)),
        Optional(live.max_age_at(1).map(|age| age.0))
    );
}

fn now() -> u64 {
    embassy_time::Instant::now().as_millis()
}
