//! Read-only evidence emitted by the UI's correlated repaint operation.
//! No fixture admission, provider control, or panel policy is owned here.
use crate::firmware::observation_fixture::{FixtureSample, Optional};
use crate::firmware::{battery, environment};
use observation::{demand::Demand, field::FieldMask};

pub(super) fn begin(id: u64) {
    console::println!(
        "PANEL_FIXTURE BEGIN id={} at_ms={}",
        id,
        embassy_time::Instant::now().as_millis()
    );
}

pub(super) fn control(id: u64, phase: &str, all_ok: bool) {
    let (environment, environment_ack) = environment::control_snapshot();
    let (battery, battery_ack) = battery::control_snapshot();
    console::println!("PANEL_FIXTURE CONTROL id={} phase={} all_ok={} environment_id={} environment_ack={:?} battery_id={} battery_ack={:?}",
        id, phase, all_ok, environment.id(), environment_ack, battery.id(), battery_ack);
}

fn live<S: FixtureSample, const N: usize>(
    id: u64,
    demand: Demand<S::Fields, N>,
    pending: bool,
    state: Option<S>,
) {
    let metadata = state.map(|s| s.metadata());
    console::println!("PANEL_FIXTURE LIVE id={} provider={} fields={} age0_ms={} age1_ms={} periodic_pending={} generation={} revision={} sampled_at_ms={}",
        id, S::PROVIDER.0, demand.fields.bits(), Optional(demand.max_age_at(0).map(|d| d.0)),
        Optional(demand.max_age_at(1).map(|d| d.0)), pending,
        Optional(metadata.map(|s| s.generation.0)), Optional(metadata.and_then(|s| s.last_sample_revision.map(|r| r.0))),
        Optional(metadata.and_then(|s| s.last_sample_at.map(|t| t.0))));
}

pub(super) fn end(id: u64, refreshed: bool, resumed: bool) {
    live(
        id,
        battery::BATTERY_DEMAND.live(),
        battery::BATTERY_DEMAND.is_pending(),
        battery::BATTERY_STATE.try_get(),
    );
    live(
        id,
        environment::ENVIRONMENT_DEMAND.live(),
        environment::ENVIRONMENT_DEMAND.is_pending(),
        environment::ENVIRONMENT_STATE.try_get(),
    );
    console::println!(
        "PANEL_FIXTURE END id={} status={} at_ms={}",
        id,
        if refreshed && resumed {
            "Completed"
        } else {
            "Failed"
        },
        embassy_time::Instant::now().as_millis()
    );
}
