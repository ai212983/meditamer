//! Persistent diagnostics delivery; screen construction never owns sampling.
use crate::firmware::cpu_observation;
use observation::{
    delivery::{decide_delivery, DeliveryAction, DeliveryInput},
    ids::ProviderGeneration,
    policy::Health,
    subscription::DeliveryTracking,
    time::Instant,
};

#[derive(Default)]
pub(super) struct CpuDeliveryState {
    tracking: DeliveryTracking,
    last_health: Option<(ProviderGeneration, Health)>,
}

pub(super) fn poll_cpu(state: &mut super::state::DisplayLoopState) {
    let Some(reading) = cpu_observation::latest() else {
        return;
    };
    let delivery = &mut state.cpu_delivery;
    let health = (reading.generation, reading.health);
    let health_changed = delivery.last_health != Some(health);
    if delivery
        .last_health
        .is_some_and(|previous| previous.0 != reading.generation)
    {
        delivery.tracking = Default::default();
    }
    if !health_changed && delivery.tracking.last_delivered_revision == Some(reading.revision) {
        return;
    }
    let action = decide_delivery(
        &cpu_observation::diagnostics_subscription(),
        &mut delivery.tracking,
        &reading.timestamps,
        DeliveryInput {
            now: Instant(embassy_time::Instant::now().as_millis()),
            health_changed,
            revision: reading.revision,
        },
    );
    delivery.last_health = Some(health);
    if matches!(
        action,
        DeliveryAction::DeliverCache(_) | DeliveryAction::DeliverCacheAndRequestRefresh { .. }
    ) || health_changed
    {
        let snapshot = (reading.health == Health::Ok).then_some(reading.snapshot);
        super::presentation::update_cpu_load(&mut state.presentation, snapshot);
    }
}
