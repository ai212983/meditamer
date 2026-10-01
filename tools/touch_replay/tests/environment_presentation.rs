//! Actual product delivery decisions and committed shell owner identities.
//! Rendering remains target-owned; this does not claim physical batching proof.
#[path = "../../../products/meditamer/src/firmware/display/battery_delivery.rs"]
mod battery_delivery;
#[path = "../../../products/meditamer/src/firmware/battery/types.rs"]
mod battery_types;
#[path = "../../../products/meditamer/src/firmware/display/environment_delivery.rs"]
mod environment_delivery;
#[path = "../../../products/meditamer/src/firmware/environment/requests.rs"]
mod environment_requests;
#[path = "../../../products/meditamer/src/firmware/environment/types.rs"]
mod environment_types;
#[path = "../../../products/meditamer/src/firmware/observation_fixture.rs"]
mod fixture_protocol;
use observation::fixture as observation_fixture;

mod config {
    pub const ENVIRONMENT_SAMPLE_INTERVAL_S: u32 = 300;
}
mod firmware {
    pub(crate) use crate::fixture_protocol as observation_fixture;
    pub mod config {
        pub(crate) const BATTERY_INTERVAL_SECONDS: u32 = 300;
    }
    pub mod battery {
        pub(crate) use crate::battery_types::*;
    }
    pub mod environment {
        pub(crate) use crate::environment_requests::RequestAdmissionError;
        pub(crate) use crate::environment_types::*;
        pub(crate) const SUBSCRIPTION_CAPACITY: usize = 2;
    }
}

use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use environment_delivery::{EntryAdmissionEvent, EnvironmentDeliveryState};
use environment_requests::{RequestAdmissionError, RequestIngress};
use environment_types::{EnvironmentSnapshot, EnvironmentStateSnapshot};
use inkplate_tempera::environment::EnvironmentReading;
use observation::{ids::ProviderGeneration, ids::Revision, policy::Health, time::Instant};
use shell::{
    model::DefaultShellModel,
    types::{
        NavIntent, OverlayInput, OverlayLifetime, ProviderId, RefreshHint, SurfaceCapabilities,
        SurfaceId, SurfaceInstanceToken, SurfaceRef, SurfaceRole, SurfaceSpec,
    },
};

fn shell() -> DefaultShellModel {
    let mut shell = DefaultShellModel::new(
        ProviderId(1),
        &[
            SurfaceSpec::new(
                1,
                SurfaceRole::Ambient,
                SurfaceCapabilities::AMBIENT,
                RefreshHint::Content,
            ),
            SurfaceSpec::new(
                2,
                SurfaceRole::Launcher,
                SurfaceCapabilities::NONE,
                RefreshHint::Content,
            ),
            SurfaceSpec::new(
                7,
                SurfaceRole::SystemRoot,
                SurfaceCapabilities::LAUNCHABLE,
                RefreshHint::Content,
            ),
            SurfaceSpec::new(
                8,
                SurfaceRole::Overlay,
                SurfaceCapabilities::OVERLAY,
                RefreshHint::Content,
            ),
        ],
        SurfaceId(1),
    )
    .unwrap();
    enter_ambient(&mut shell);
    shell
}

fn enter_ambient(shell: &mut DefaultShellModel) {
    let provider = shell.active_instance().surface.owner;
    for intent in [
        NavIntent::OpenLauncher(SurfaceRef::new(provider, 2)),
        NavIntent::Launch(SurfaceRef::new(provider, 7)),
    ] {
        let prepared = shell.prepare_intent(intent).unwrap();
        shell.commit_navigation(prepared).unwrap();
    }
}

fn consumer(owner: SurfaceInstanceToken) -> EnvironmentDeliveryState {
    let mut consumer = EnvironmentDeliveryState::default();
    assert!(!consumer.needs_service_retry());
    consumer.reconcile(Some(owner), Instant(0)).unwrap();
    consumer
}

fn sample(revision: u32, at: u64, temperature: i16, humidity: u32) -> EnvironmentStateSnapshot {
    EnvironmentStateSnapshot {
        revision: Revision(revision),
        health: Health::Ok,
        last_sample_at: Some(Instant(at)),
        last_attempt_at: Some(Instant(at)),
        last_sample_revision: Some(Revision(revision)),
        snapshot: EnvironmentSnapshot {
            onboard: EnvironmentReading {
                temperature_centidegrees: temperature,
                humidity_millipercent: humidity,
            },
            external: None,
        },
        ..Default::default()
    }
}

#[test]
fn unavailable_samples_route_health_without_projecting_zero_readings() {
    let owner = shell().active_instance();
    let mut consumer = consumer(owner);
    assert!(consumer.take_delivery(owner, None, Instant(0)).is_none());
    let unknown = consumer
        .take_delivery(owner, Some(EnvironmentStateSnapshot::default()), Instant(1))
        .unwrap();
    assert!(unknown.health_changed);
    assert!(!unknown.deliver_values);
    let failed = EnvironmentStateSnapshot {
        health: Health::Failed,
        ..Default::default()
    };
    let failure = consumer
        .take_delivery(owner, Some(failed), Instant(2))
        .unwrap();
    assert!(failure.health_changed);
    assert!(!failure.deliver_values);
    assert!(consumer
        .take_delivery(owner, Some(failed), Instant(3))
        .is_none());
    let ready = consumer
        .take_delivery(owner, Some(sample(1, 4, 2200, 45000)), Instant(4))
        .unwrap();
    assert!(ready.deliver_values);
    assert!(ready.health_changed);
    assert_eq!(ready.state.snapshot.onboard.temperature_centidegrees, 2200);
    assert_eq!(ready.state.snapshot.onboard.humidity_millipercent, 45000);
}

#[test]
fn combined_reading_is_delivered_once_then_later_revision_obeys_interval() {
    let owner = shell().active_instance();
    let mut consumer = consumer(owner);
    let first = sample(1, 10, 2350, 43210);
    let delivered = consumer
        .take_delivery(owner, Some(first), Instant(10))
        .unwrap();
    assert!(delivered.deliver_values);
    assert_eq!(delivered.state.revision, Revision(1));
    assert_eq!(delivered.state.generation, first.generation);
    assert_eq!(delivered.state.health, Health::Ok);
    assert!(consumer
        .take_delivery(owner, Some(first), Instant(11))
        .is_none());
    let next = sample(2, 20, 2410, 44000);
    assert!(consumer
        .take_delivery(owner, Some(next), Instant(20))
        .is_none());
    let delivered = consumer
        .take_delivery(owner, Some(next), Instant(300010))
        .unwrap();
    assert_eq!(delivered.state.revision, Revision(2));
    assert_eq!(
        delivered.state.snapshot.onboard.temperature_centidegrees,
        2410
    );
    assert_eq!(
        delivered.state.snapshot.onboard.humidity_millipercent,
        44000
    );
}

#[test]
fn optional_sht45_sample_is_delivered_alongside_the_bme688_sample() {
    let owner = shell().active_instance();
    let mut consumer = consumer(owner);
    let mut state = sample(1, 10, 2750, 48_000);
    state.snapshot.external = Some(EnvironmentReading {
        temperature_centidegrees: 2360,
        humidity_millipercent: 46_500,
    });

    let delivered = consumer
        .take_delivery(owner, Some(state), Instant(10))
        .unwrap();
    assert!(delivered.deliver_values);
    assert_eq!(
        delivered.state.snapshot.onboard.temperature_centidegrees,
        2750
    );
    assert_eq!(
        delivered.state.snapshot.external,
        Some(EnvironmentReading {
            temperature_centidegrees: 2360,
            humidity_millipercent: 46_500,
        })
    );
}

#[test]
fn environment_unchanged_cache_does_not_spend_the_next_delivery_interval() {
    let owner = shell().active_instance();
    let mut consumer = consumer(owner);
    let first = sample(1, 10050, 3194, 41360);
    assert!(
        consumer
            .take_delivery(owner, Some(first), Instant(10050))
            .unwrap()
            .deliver_values
    );
    // The UI polls at the age deadline before the conversion completes.
    assert!(consumer
        .take_delivery(owner, Some(first), Instant(310050))
        .is_none());
    let fresh = sample(2, 310125, 3194, 41360);
    assert!(
        consumer
            .take_delivery(owner, Some(fresh), Instant(310125))
            .unwrap()
            .deliver_values
    );
    assert!(consumer
        .take_delivery(owner, Some(sample(3, 310200, 3195, 41370)), Instant(310200))
        .is_none());
}

#[test]
fn awaited_fresh_result_bypasses_interval_after_stale_entry() {
    let owner = shell().active_instance();
    let mut consumer = EnvironmentDeliveryState::default();
    consumer.reconcile(Some(owner), Instant(300000)).unwrap();
    let ingress = Ingress::<2>::new();
    assert_eq!(
        admit(
            &mut consumer,
            &ingress,
            Some(sample(1, 0, 2000, 40000)),
            300000
        ),
        Some(EntryAdmissionEvent::Admitted)
    );
    consumer
        .take_delivery(owner, Some(sample(1, 0, 2000, 40000)), Instant(300000))
        .unwrap();
    let fresh = sample(2, 300001, 2100, 41000);
    let delivered = consumer
        .take_delivery(owner, Some(fresh), Instant(300001))
        .unwrap();
    assert!(delivered.deliver_values);
    assert_eq!(delivered.state.revision, Revision(2));
    assert!(consumer
        .take_delivery(owner, Some(fresh), Instant(300002))
        .is_none());
}

#[test]
fn unchanged_committed_owner_survives_overlay_and_uncommitted_navigation() {
    let mut shell = shell();
    let owner = shell.active_instance();
    let mut consumer = consumer(owner);
    let cached = sample(1, 0, 2200, 45000);
    consumer
        .take_delivery(owner, Some(cached), Instant(0))
        .unwrap();
    // Preparing a transition does not commit its candidate, including rollback
    // before commit. The subscription still belongs to the origin instance.
    let _candidate = shell.prepare_intent(NavIntent::Home).unwrap();
    consumer
        .reconcile(Some(shell.active_instance()), Instant(1))
        .unwrap();
    assert!(consumer
        .take_delivery(owner, Some(cached), Instant(1))
        .is_none());
    let overlay = shell
        .prepare_overlay_request(
            SurfaceRef::new(owner.surface.owner, 8),
            OverlayInput::Modal,
            OverlayLifetime::Transient,
            0,
        )
        .unwrap();
    shell.commit_overlay_request(overlay).unwrap();
    assert_eq!(shell.active_instance(), owner);
    consumer
        .reconcile(Some(shell.active_instance()), Instant(1))
        .unwrap();
    assert!(consumer
        .take_delivery(owner, Some(cached), Instant(2))
        .is_none());
}

#[test]
fn removal_and_reentry_reject_old_owner_and_reset_delivery_and_health() {
    let mut shell = shell();
    let old = shell.active_instance();
    let mut consumer = consumer(old);
    let cached = sample(1, 0, 2200, 45000);
    consumer
        .take_delivery(old, Some(cached), Instant(0))
        .unwrap();
    let leave = shell.prepare_intent(NavIntent::Home).unwrap();
    shell.commit_navigation(leave).unwrap();
    consumer.reconcile(None, Instant(1)).unwrap();
    assert!(consumer
        .take_delivery(old, Some(cached), Instant(1))
        .is_none());
    enter_ambient(&mut shell);
    let new = shell.active_instance();
    assert_eq!(old.surface, new.surface);
    assert_ne!(old.generation, new.generation);
    consumer.reconcile(Some(new), Instant(2)).unwrap();
    assert!(consumer
        .take_delivery(old, Some(cached), Instant(2))
        .is_none());
    let entry = consumer
        .take_delivery(new, Some(cached), Instant(2))
        .unwrap();
    assert!(entry.deliver_values);
    assert!(entry.health_changed);
    assert!(consumer
        .take_delivery(new, Some(cached), Instant(3))
        .is_none());
}

#[test]
fn same_surface_replacement_between_polls_still_resets_owner() {
    let mut shell = shell();
    let old = shell.active_instance();
    let mut consumer = consumer(old);
    let cached = sample(1, 0, 2200, 45000);
    consumer
        .take_delivery(old, Some(cached), Instant(0))
        .unwrap();
    let leave = shell.prepare_intent(NavIntent::Home).unwrap();
    shell.commit_navigation(leave).unwrap();
    enter_ambient(&mut shell);
    let new = shell.active_instance();
    consumer.reconcile(Some(new), Instant(2)).unwrap();
    assert!(consumer
        .take_delivery(old, Some(cached), Instant(1))
        .is_none());
    assert!(
        consumer
            .take_delivery(new, Some(cached), Instant(1))
            .unwrap()
            .deliver_values
    );
}

#[test]
fn health_changes_and_provider_restart_bypass_interval_but_old_generation_is_rejected() {
    let owner = shell().active_instance();
    let mut consumer = consumer(owner);
    let cached = sample(9, 0, 2200, 45000);
    consumer
        .take_delivery(owner, Some(cached), Instant(0))
        .unwrap();
    let degraded = EnvironmentStateSnapshot {
        health: Health::Degraded,
        ..cached
    };
    let health = consumer
        .take_delivery(owner, Some(degraded), Instant(1))
        .unwrap();
    assert!(health.health_changed);
    assert!(health.deliver_values);
    assert_eq!(health.state.snapshot.onboard.temperature_centidegrees, 2200);
    assert!(consumer
        .take_delivery(owner, Some(degraded), Instant(2))
        .is_none());
    let restarted = EnvironmentStateSnapshot {
        generation: ProviderGeneration(1),
        ..sample(1, 3, 2300, 46000)
    };
    let next = consumer
        .take_delivery(owner, Some(restarted), Instant(3))
        .unwrap();
    assert!(next.deliver_values);
    assert!(next.health_changed);
    assert!(consumer
        .take_delivery(owner, Some(cached), Instant(4))
        .is_none());
}

type Ingress<const N: usize> =
    RequestIngress<NoopRawMutex, environment_types::EnvironmentFields, N>;

fn admit<const N: usize>(
    consumer: &mut EnvironmentDeliveryState,
    ingress: &Ingress<N>,
    sample: Option<EnvironmentStateSnapshot>,
    at: u64,
) -> Option<EntryAdmissionEvent> {
    consumer.admit_entry(
        Instant(at),
        sample,
        ingress.close_generation(),
        |request, now| ingress.try_admit(request, now),
    )
}

#[test]
fn fresh_entry_cache_and_failure_revision_do_not_consume_fresh_result_bypass() {
    let owner = shell().active_instance();
    let mut consumer = consumer(owner);
    let ingress = Ingress::<2>::new();
    let cached = sample(1, 10, 2200, 45000);
    assert_eq!(
        admit(&mut consumer, &ingress, Some(cached), 20),
        Some(EntryAdmissionEvent::Admitted)
    );
    assert!(
        consumer
            .take_delivery(owner, Some(cached), Instant(20))
            .unwrap()
            .deliver_values
    );
    assert!(consumer
        .take_delivery(owner, Some(cached), Instant(21))
        .is_none());
    let failed = EnvironmentStateSnapshot {
        revision: Revision(2),
        health: Health::Degraded,
        ..cached
    };
    let health = consumer
        .take_delivery(owner, Some(failed), Instant(22))
        .unwrap();
    assert!(health.health_changed);
    assert!(consumer
        .take_delivery(
            owner,
            Some(EnvironmentStateSnapshot {
                revision: Revision(3),
                ..failed
            }),
            Instant(23)
        )
        .is_none());
    // Keep health unchanged to prove the request bypass, not a health bypass.
    let recovered = EnvironmentStateSnapshot {
        health: Health::Degraded,
        ..sample(4, 24, 2300, 46000)
    };
    assert!(
        consumer
            .take_delivery(owner, Some(recovered), Instant(24))
            .unwrap()
            .deliver_values
    );
    assert!(consumer
        .take_delivery(
            owner,
            Some(EnvironmentStateSnapshot {
                health: Health::Degraded,
                ..sample(5, 25, 2400, 47000)
            }),
            Instant(25)
        )
        .is_none());
    assert_eq!(admit(&mut consumer, &ingress, Some(recovered), 26), None);
}

#[test]
fn demand_is_exact_owner_scoped_and_admitted_old_entry_keeps_its_provider_credit() {
    let old = shell().active_instance();
    let mut consumer = consumer(old);
    let ingress = Ingress::<2>::new();
    assert!(!consumer.environment_demand(Instant(0)).is_empty());
    assert_eq!(
        admit(&mut consumer, &ingress, None, 0),
        Some(EntryAdmissionEvent::Admitted)
    );
    let old_request = ingress.try_receive().unwrap();
    assert!(!consumer.reconcile(Some(old), Instant(1)).unwrap());
    assert_eq!(admit(&mut consumer, &ingress, None, 1), None);
    assert!(consumer.reconcile(None, Instant(2)).unwrap());
    assert!(consumer.environment_demand(Instant(2)).is_empty());
    assert_eq!(ingress.outstanding(), 1);
    let mut replacement = old;
    replacement.surface.owner.id = ProviderId(99);
    // Shell surface and shell generation coincide across this replacement.
    assert!(consumer.reconcile(Some(replacement), Instant(3)).unwrap());
    assert_eq!(
        admit(&mut consumer, &ingress, None, 3),
        Some(EntryAdmissionEvent::Admitted)
    );
    let new_request = ingress.try_receive().unwrap();
    assert_ne!(old_request.owner_generation, new_request.owner_generation);
    assert_eq!(old_request.owner, new_request.owner);
    assert!(consumer
        .take_delivery(old, Some(sample(1, 4, 2200, 45000)), Instant(4))
        .is_none());
    assert!(!consumer.environment_demand(Instant(4)).is_empty());
}

#[test]
fn rejected_entry_retries_report_transitions_and_preserve_original_expiry() {
    let owner = shell().active_instance();
    let mut blocker = consumer(owner);
    let mut consumer = consumer(owner);
    let ingress = Ingress::<1>::new();
    assert_eq!(
        admit(&mut blocker, &ingress, None, 0),
        Some(EntryAdmissionEvent::Admitted)
    );
    assert_eq!(
        admit(&mut consumer, &ingress, None, 1),
        Some(EntryAdmissionEvent::Rejected(RequestAdmissionError::Full))
    );
    assert_eq!(admit(&mut consumer, &ingress, None, 2), None);
    ingress.close();
    assert_eq!(
        admit(&mut consumer, &ingress, None, 3),
        Some(EntryAdmissionEvent::Rejected(RequestAdmissionError::Closed))
    );
    assert_eq!(admit(&mut consumer, &ingress, None, 4), None);
    assert!(ingress.open());
    assert_eq!(
        admit(&mut consumer, &ingress, None, 299999),
        Some(EntryAdmissionEvent::Admitted)
    );
    let request = ingress.try_receive().unwrap();
    assert_eq!(request.admitted_at, Instant(299999));
    assert_eq!(request.expires_at, Instant(300000));
    assert_eq!(
        admit(&mut consumer, &ingress, None, 300000),
        Some(EntryAdmissionEvent::Expired)
    );
    assert_eq!(admit(&mut consumer, &ingress, None, 300001), None);
}

#[test]
fn unadmitted_entry_expires_once_without_post_deadline_admission() {
    let owner = shell().active_instance();
    let mut consumer = consumer(owner);
    let ingress = Ingress::<1>::new();
    ingress.close();
    assert_eq!(
        admit(&mut consumer, &ingress, None, 1),
        Some(EntryAdmissionEvent::Rejected(RequestAdmissionError::Closed))
    );
    assert_eq!(
        admit(&mut consumer, &ingress, None, 300000),
        Some(EntryAdmissionEvent::Expired)
    );
    assert!(ingress.open());
    assert_eq!(admit(&mut consumer, &ingress, None, 300001), None);
    assert_eq!(ingress.outstanding(), 0);
    assert!(!consumer.environment_demand(Instant(300001)).is_empty());
}

#[test]
fn suspension_cancellation_retries_same_entry_with_new_baseline_and_old_deadline() {
    let owner = shell().active_instance();
    let mut consumer = consumer(owner);
    let ingress = Ingress::<2>::new();
    let old_cache = sample(1, 10, 2200, 45000);
    assert_eq!(
        admit(&mut consumer, &ingress, Some(old_cache), 20),
        Some(EntryAdmissionEvent::Admitted)
    );
    consumer
        .take_delivery(owner, Some(old_cache), Instant(20))
        .unwrap();
    assert_eq!(ingress.close(), 1);
    assert_eq!(
        admit(&mut consumer, &ingress, Some(old_cache), 30),
        Some(EntryAdmissionEvent::Rejected(RequestAdmissionError::Closed))
    );
    assert_eq!(admit(&mut consumer, &ingress, Some(old_cache), 31), None);
    assert!(ingress.open());
    let baseline = sample(2, 32, 2300, 46000);
    assert_eq!(
        admit(&mut consumer, &ingress, Some(baseline), 33),
        Some(EntryAdmissionEvent::Admitted)
    );
    let request = ingress.try_receive().unwrap();
    assert_eq!(request.admitted_at, Instant(33));
    assert_eq!(request.expires_at, Instant(300000));
    assert!(consumer
        .take_delivery(owner, Some(baseline), Instant(33))
        .is_none());
    assert!(
        consumer
            .take_delivery(owner, Some(sample(3, 34, 2400, 47000)), Instant(34))
            .unwrap()
            .deliver_values
    );
}

#[test]
fn completion_before_admission_or_at_expiry_cannot_satisfy_entry() {
    let owner = shell().active_instance();
    let mut consumer = consumer(owner);
    let ingress = Ingress::<2>::new();
    let cached = sample(1, 1, 2200, 45000);
    assert_eq!(
        admit(&mut consumer, &ingress, Some(cached), 20),
        Some(EntryAdmissionEvent::Admitted)
    );
    consumer
        .take_delivery(owner, Some(cached), Instant(20))
        .unwrap();
    assert!(consumer
        .take_delivery(owner, Some(sample(2, 19, 2300, 46000)), Instant(21))
        .is_none());
    assert_eq!(
        admit(
            &mut consumer,
            &ingress,
            Some(sample(3, 300000, 2400, 47000)),
            300000
        ),
        Some(EntryAdmissionEvent::Expired)
    );
}

#[test]
fn success_before_expiry_can_be_delivered_when_ui_polls_after_expiry() {
    let owner = shell().active_instance();
    let mut consumer = consumer(owner);
    let ingress = Ingress::<2>::new();
    assert_eq!(
        admit(&mut consumer, &ingress, None, 20),
        Some(EntryAdmissionEvent::Admitted)
    );
    let completed = sample(1, 299999, 2200, 45000);
    assert_eq!(
        admit(&mut consumer, &ingress, Some(completed), 300001),
        None
    );
    assert!(
        consumer
            .take_delivery(owner, Some(completed), Instant(300001))
            .unwrap()
            .deliver_values
    );
}

#[test]
fn missed_suspension_epoch_renews_entry_before_accepting_new_cache() {
    let owner = shell().active_instance();
    let mut consumer = consumer(owner);
    let ingress = Ingress::<2>::new();
    let cached = sample(1, 10, 2200, 45000);
    assert_eq!(
        admit(&mut consumer, &ingress, Some(cached), 20),
        Some(EntryAdmissionEvent::Admitted)
    );
    consumer
        .take_delivery(owner, Some(cached), Instant(20))
        .unwrap();
    ingress.close();
    assert!(ingress.open());
    let resumed_cache = sample(2, 30, 2300, 46000);
    assert_eq!(
        admit(&mut consumer, &ingress, Some(resumed_cache), 31),
        Some(EntryAdmissionEvent::Admitted)
    );
    assert_eq!(ingress.try_receive().unwrap().admitted_at, Instant(31));
    assert!(consumer
        .take_delivery(owner, Some(resumed_cache), Instant(31))
        .is_none());
    assert!(
        consumer
            .take_delivery(owner, Some(sample(3, 32, 2400, 47000)), Instant(32))
            .unwrap()
            .deliver_values
    );
}

#[test]
fn generation_exhaustion_withdraws_owner_without_reusing_a_key() {
    use observation::ids::{OwnerGeneration, OwnerGenerationExhausted};

    let mut shell = shell();
    let owner = shell.active_instance();
    let mut authority =
        EnvironmentDeliveryState::with_generation_for_test(OwnerGeneration(u32::MAX - 1));
    authority.reconcile(Some(owner), Instant(0)).unwrap();
    assert!(!authority.environment_demand(Instant(0)).is_empty());
    let cached = sample(1, 0, 2200, 45000);
    assert!(authority
        .take_delivery(owner, Some(cached), Instant(0))
        .is_some());
    let leave = shell.prepare_intent(NavIntent::Home).unwrap();
    shell.commit_navigation(leave).unwrap();
    enter_ambient(&mut shell);
    let replacement = shell.active_instance();
    assert_ne!(owner, replacement);
    assert_eq!(
        authority.reconcile(Some(replacement), Instant(1)),
        Err(OwnerGenerationExhausted)
    );
    assert!(authority.environment_demand(Instant(1)).is_empty());
    assert!(authority
        .take_delivery(owner, Some(cached), Instant(1))
        .is_none());
    assert!(authority
        .take_delivery(replacement, Some(cached), Instant(1))
        .is_none());
    assert_eq!(
        authority.reconcile(Some(owner), Instant(2)),
        Err(OwnerGenerationExhausted)
    );
    assert!(authority.environment_demand(Instant(2)).is_empty());
    let ingress = Ingress::<2>::new();
    assert_eq!(admit(&mut authority, &ingress, Some(cached), 2), None);
    assert_eq!(ingress.outstanding(), 0);
}

#[path = "../../../products/meditamer/src/firmware/bounded_control.rs"]
#[allow(dead_code)]
mod ingress_control;

#[test]
fn full_ingress_cannot_block_control_and_suspend_reclaims_all_credits() {
    use core::{
        future::Future,
        task::{Context, Poll, Waker},
    };
    use environment_requests::RequestIngress;
    use environment_types::EnvironmentFields;
    use ingress_control::{Control, SuspendAck};
    use observation::field::FieldMask;
    use observation::{
        runtime::{AcquisitionDriver, ProviderLoop, SuspendOutcome},
        time::Duration,
    };

    struct Driver;
    impl AcquisitionDriver<EnvironmentFields, EnvironmentSnapshot> for Driver {
        type Error = ();
        async fn acquire(
            &mut self,
            fields: EnvironmentFields,
        ) -> Result<(EnvironmentFields, EnvironmentSnapshot), ()> {
            Ok((
                fields,
                EnvironmentSnapshot {
                    onboard: EnvironmentReading {
                        temperature_centidegrees: 2200,
                        humidity_millipercent: 40000,
                    },
                    external: None,
                },
            ))
        }
        async fn cancel(&mut self) -> Result<(), ()> {
            Ok(())
        }
        fn min_acquisition_interval(&self) -> Duration {
            Duration::ZERO
        }
    }
    fn immediately<F: Future>(future: F) -> F::Output {
        let mut future = core::pin::pin!(future);
        match future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            Poll::Ready(result) => result,
            Poll::Pending => panic!("unexpected wait"),
        }
    }
    let ingress = RequestIngress::<NoopRawMutex, EnvironmentFields, 2>::new();
    let control = Control::<NoopRawMutex>::new();
    let mut provider = ProviderLoop::<_, _, _, 2, 2>::new(
        Driver,
        environment_types::ENVIRONMENT_PROVIDER_ID,
        ProviderGeneration::INITIAL,
        EnvironmentSnapshot {
            onboard: EnvironmentReading {
                temperature_centidegrees: 0,
                humidity_millipercent: 0,
            },
            external: None,
        },
    );
    let request = observation::observe_now::ObserveNowRequest {
        owner: observation::ids::OwnerId(1),
        owner_generation: observation::ids::OwnerGeneration(0),
        fields: EnvironmentFields::TEMPERATURE.union(EnvironmentFields::HUMIDITY),
        admitted_at: Instant(0),
        expires_at: Instant(100),
    };
    ingress.try_admit(request, Instant(0)).unwrap();
    provider
        .admit_observe_now_at(ingress.try_receive().unwrap(), Instant(0))
        .unwrap();
    ingress.try_admit(request, Instant(0)).unwrap();
    let suspend = control.request_suspend().unwrap();
    assert_eq!(ingress.close(), 1);
    assert_eq!(control.try_receive(), Some(suspend));
    let before = provider.pending_observe_now();
    assert_eq!(
        immediately(provider.suspend(Instant(1))),
        SuspendOutcome::Quiesced
    );
    ingress.complete(before - provider.pending_observe_now());
    assert_eq!(ingress.outstanding(), 0);
    let mut actor = core::pin::pin!(control.hold_suspended(suspend, true));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(actor.as_mut().poll(&mut cx).is_pending());
    assert_eq!(
        immediately(control.wait_suspended(suspend, core::future::ready(()))),
        SuspendAck::Quiesced
    );
    assert_eq!(
        ingress.try_admit(request, Instant(1)),
        Err(RequestAdmissionError::Closed)
    );
    control.request_resume(false);
    assert!(actor.as_mut().poll(&mut cx).is_ready());
    provider.resume();
    assert!(ingress.open());
    ingress.try_admit(request, Instant(2)).unwrap();
    ingress.try_admit(request, Instant(2)).unwrap();
    assert_eq!(ingress.outstanding(), 2);
}

use battery_delivery::{BatteryDeliveryState, EntryAdmissionEvent as BatteryEntryEvent};
use battery_types::{BatteryFields, BatterySnapshot, BatteryStateSnapshot, BATTERY_PROVIDER_ID};

type BatteryIngress<const N: usize> = RequestIngress<NoopRawMutex, BatteryFields, N>;

fn battery_sample(revision: u32, at: u64, percent: u8) -> BatteryStateSnapshot {
    BatteryStateSnapshot {
        provider: BATTERY_PROVIDER_ID,
        generation: ProviderGeneration::INITIAL,
        revision: Revision(revision),
        health: Health::Ok,
        last_attempt_at: Some(Instant(at)),
        last_sample_at: Some(Instant(at)),
        last_sample_revision: Some(Revision(revision)),
        snapshot: BatterySnapshot { percent },
    }
}

fn battery_consumer(at: u64) -> BatteryDeliveryState {
    let mut consumer = BatteryDeliveryState::default();
    assert!(!consumer.needs_service_retry());
    assert!(consumer.reconcile(true, Instant(at)).unwrap());
    consumer
}

fn admit_battery<const N: usize>(
    consumer: &mut BatteryDeliveryState,
    ingress: &BatteryIngress<N>,
    state: Option<BatteryStateSnapshot>,
    at: u64,
) -> Option<BatteryEntryEvent> {
    consumer.admit_entry(
        Instant(at),
        state,
        ingress.close_generation(),
        |request, now| ingress.try_admit(request, now),
    )
}

#[test]
fn battery_consumer_survives_navigation_without_owner_or_cadence_reset() {
    let mut consumer = battery_consumer(0);
    let key = consumer.key().unwrap();
    let demand = consumer.battery_demand(Instant(0));
    assert_eq!(demand.fields, BatteryFields::LEVEL);
    assert_eq!(
        demand.max_age_at(0),
        Some(observation::time::Duration::from_secs(300))
    );
    let cached = battery_sample(1, 10, 77);
    assert!(
        consumer
            .take_delivery(key, Some(cached), Instant(10))
            .unwrap()
            .deliver_values
    );
    let mut shell = shell();
    let leave = shell.prepare_intent(NavIntent::Home).unwrap();
    shell.commit_navigation(leave).unwrap();
    enter_ambient(&mut shell);
    assert!(!consumer.reconcile(true, Instant(11)).unwrap());
    assert_eq!(consumer.key(), Some(key));
    let newer = battery_sample(2, 12, 76);
    assert!(consumer
        .take_delivery(key, Some(newer), Instant(12))
        .is_none());
    assert_eq!(
        consumer
            .take_delivery(key, Some(newer), Instant(300010))
            .unwrap()
            .state
            .snapshot
            .percent,
        76
    );
    assert!(!consumer.battery_demand(Instant(300010)).is_empty());
}

#[test]
fn battery_unchanged_cache_does_not_spend_the_next_delivery_interval() {
    let mut consumer = battery_consumer(0);
    let key = consumer.key().unwrap();
    let first = battery_sample(1, 10022, 100);
    assert!(
        consumer
            .take_delivery(key, Some(first), Instant(10022))
            .unwrap()
            .deliver_values
    );
    assert!(consumer
        .take_delivery(key, Some(first), Instant(310022))
        .is_none());
    let fresh = battery_sample(2, 310029, 100);
    assert!(
        consumer
            .take_delivery(key, Some(fresh), Instant(310029))
            .unwrap()
            .deliver_values
    );
    assert!(consumer
        .take_delivery(key, Some(battery_sample(3, 310100, 99)), Instant(310100))
        .is_none());
}

#[test]
fn battery_health_never_fabricates_trace_level_and_failure_retains_last_good_sample() {
    let mut consumer = battery_consumer(0);
    let key = consumer.key().unwrap();
    let missing = BatteryStateSnapshot {
        health: Health::Unknown,
        last_sample_at: None,
        last_sample_revision: None,
        last_attempt_at: None,
        ..battery_sample(0, 0, 0)
    };
    assert!(consumer.take_delivery(key, None, Instant(0)).is_none());
    let unknown = consumer
        .take_delivery(key, Some(missing), Instant(1))
        .unwrap();
    assert!(unknown.health_changed);
    assert!(!unknown.deliver_values);
    let failure = BatteryStateSnapshot {
        health: Health::Failed,
        revision: Revision(1),
        last_attempt_at: Some(Instant(2)),
        ..missing
    };
    let failed = consumer
        .take_delivery(key, Some(failure), Instant(2))
        .unwrap();
    assert!(failed.health_changed);
    assert!(!failed.deliver_values);
    let valid = battery_sample(2, 3, 77);
    assert_eq!(
        consumer
            .take_delivery(key, Some(valid), Instant(3))
            .unwrap()
            .state
            .snapshot
            .percent,
        77
    );
    let failed = BatteryStateSnapshot {
        health: Health::Degraded,
        revision: Revision(3),
        last_attempt_at: Some(Instant(4)),
        ..valid
    };
    let retained = consumer
        .take_delivery(key, Some(failed), Instant(4))
        .unwrap();
    assert!(retained.health_changed);
    assert!(retained.deliver_values);
    assert_eq!(retained.state.snapshot.percent, 77);
    assert_eq!(retained.state.last_sample_at, Some(Instant(3)));
    assert_eq!(retained.state.last_attempt_at, Some(Instant(4)));
    assert!(consumer
        .take_delivery(
            key,
            Some(BatteryStateSnapshot {
                revision: Revision(4),
                ..failed
            }),
            Instant(5)
        )
        .is_none());
}

#[test]
fn battery_startup_uses_successful_identity_even_with_same_timestamp_and_unseen_failure() {
    let mut consumer = battery_consumer(100);
    let ingress = BatteryIngress::<2>::new();
    let key = consumer.key().unwrap();
    let cached = BatteryStateSnapshot {
        health: Health::Degraded,
        ..battery_sample(1, 100, 77)
    };
    assert_eq!(
        admit_battery(&mut consumer, &ingress, Some(cached), 100),
        Some(BatteryEntryEvent::Admitted)
    );
    assert!(
        consumer
            .take_delivery(key, Some(cached), Instant(100))
            .unwrap()
            .deliver_values
    );
    assert!(consumer
        .take_delivery(key, Some(cached), Instant(100))
        .is_none());
    let failure = BatteryStateSnapshot {
        revision: Revision(2),
        ..cached
    };
    assert!(consumer
        .take_delivery(key, Some(failure), Instant(101))
        .is_none());
    let success_then_failure = BatteryStateSnapshot {
        health: Health::Degraded,
        revision: Revision(4),
        ..battery_sample(3, 100, 76)
    };
    assert!(
        consumer
            .take_delivery(key, Some(success_then_failure), Instant(102))
            .unwrap()
            .deliver_values
    );
    let periodic = BatteryStateSnapshot {
        health: Health::Degraded,
        ..battery_sample(5, 103, 75)
    };
    assert!(consumer
        .take_delivery(key, Some(periodic), Instant(103))
        .is_none());
    assert_eq!(
        admit_battery(&mut consumer, &ingress, Some(periodic), 104),
        None
    );
}

#[test]
fn battery_rejection_transitions_and_missed_suspension_keep_original_startup_expiry() {
    use observation::{
        ids::{OwnerGeneration, OwnerId},
        observe_now::ObserveNowRequest,
    };
    let mut consumer = battery_consumer(0);
    let ingress = BatteryIngress::<1>::new();
    let cached = battery_sample(1, 1, 77);
    ingress
        .try_admit(
            ObserveNowRequest {
                owner: OwnerId(99),
                owner_generation: OwnerGeneration(0),
                fields: BatteryFields::LEVEL,
                admitted_at: Instant(0),
                expires_at: Instant(300000),
            },
            Instant(0),
        )
        .unwrap();
    assert_eq!(
        admit_battery(&mut consumer, &ingress, Some(cached), 1),
        Some(BatteryEntryEvent::Rejected(RequestAdmissionError::Full))
    );
    assert_eq!(
        admit_battery(&mut consumer, &ingress, Some(cached), 2),
        None
    );
    ingress.close();
    assert_eq!(
        admit_battery(&mut consumer, &ingress, Some(cached), 3),
        Some(BatteryEntryEvent::Rejected(RequestAdmissionError::Closed))
    );
    assert_eq!(
        admit_battery(&mut consumer, &ingress, Some(cached), 4),
        None
    );
    assert!(ingress.open());
    assert_eq!(
        admit_battery(&mut consumer, &ingress, Some(cached), 100),
        Some(BatteryEntryEvent::Admitted)
    );
    let key = consumer.key().unwrap();
    consumer
        .take_delivery(key, Some(cached), Instant(100))
        .unwrap();
    assert_eq!(ingress.close(), 1);
    assert!(ingress.open());
    let new_cache = battery_sample(2, 110, 76);
    assert_eq!(
        admit_battery(&mut consumer, &ingress, Some(new_cache), 111),
        Some(BatteryEntryEvent::Admitted)
    );
    let request = ingress.try_receive().unwrap();
    assert_eq!(request.admitted_at, Instant(111));
    assert_eq!(request.expires_at, Instant(300000));
    assert!(consumer
        .take_delivery(key, Some(new_cache), Instant(111))
        .is_none());
    assert_eq!(
        admit_battery(&mut consumer, &ingress, Some(new_cache), 300000),
        Some(BatteryEntryEvent::Expired)
    );
    assert_eq!(
        admit_battery(&mut consumer, &ingress, Some(new_cache), 300001),
        None
    );
    assert!(!consumer.battery_demand(Instant(300001)).is_empty());
}

#[test]
fn battery_owner_removal_drops_demand_and_delivery_without_revoking_admitted_request() {
    let mut consumer = battery_consumer(0);
    let old = consumer.key().unwrap();
    let ingress = BatteryIngress::<2>::new();
    assert_eq!(
        admit_battery(&mut consumer, &ingress, None, 0),
        Some(BatteryEntryEvent::Admitted)
    );
    let old_request = ingress.try_receive().unwrap();
    assert!(consumer.reconcile(false, Instant(1)).unwrap());
    assert!(consumer.battery_demand(Instant(1)).is_empty());
    assert!(consumer
        .take_delivery(old, Some(battery_sample(1, 2, 77)), Instant(2))
        .is_none());
    assert_eq!(admit_battery(&mut consumer, &ingress, None, 2), None);
    assert_eq!(ingress.outstanding(), 1);
    assert!(consumer.reconcile(true, Instant(3)).unwrap());
    let new = consumer.key().unwrap();
    assert_ne!(new.owner_generation, old.owner_generation);
    assert_eq!(
        admit_battery(&mut consumer, &ingress, None, 3),
        Some(BatteryEntryEvent::Admitted)
    );
    let new_request = ingress.try_receive().unwrap();
    assert_ne!(old_request.owner_generation, new_request.owner_generation);
    assert!(consumer
        .take_delivery(old, Some(battery_sample(1, 4, 77)), Instant(4))
        .is_none());
    assert!(
        consumer
            .take_delivery(new, Some(battery_sample(1, 4, 77)), Instant(4))
            .unwrap()
            .deliver_values
    );
}

#[test]
fn battery_rejects_foreign_provider_and_older_generation_before_changing_entry() {
    let mut consumer = battery_consumer(0);
    let ingress = BatteryIngress::<2>::new();
    let key = consumer.key().unwrap();
    let current = BatteryStateSnapshot {
        generation: ProviderGeneration(1),
        ..battery_sample(1, 100, 77)
    };
    assert_eq!(
        admit_battery(&mut consumer, &ingress, Some(current), 100),
        Some(BatteryEntryEvent::Admitted)
    );
    consumer
        .take_delivery(key, Some(current), Instant(100))
        .unwrap();
    let old = battery_sample(2, 101, 76);
    let foreign = BatteryStateSnapshot {
        provider: observation::ids::ProviderId(99),
        generation: ProviderGeneration(2),
        ..old
    };
    for rejected in [old, foreign] {
        assert_eq!(
            consumer.admit_entry(Instant(101), Some(rejected), 1, |_, _| panic!(
                "rejected state changed entry"
            )),
            None
        );
        assert!(consumer
            .take_delivery(key, Some(rejected), Instant(101))
            .is_none());
    }
    // Rejected state/epoch pairs must not turn the current admission into retry.
    assert_eq!(
        consumer.admit_entry(Instant(102), Some(current), 0, |_, _| panic!(
            "current admission was changed"
        )),
        None
    );
    let completed = BatteryStateSnapshot {
        generation: ProviderGeneration(1),
        ..battery_sample(2, 103, 76)
    };
    assert!(
        consumer
            .take_delivery(key, Some(completed), Instant(103))
            .unwrap()
            .deliver_values
    );
}

#[test]
fn battery_owner_generation_exhaustion_leaves_persistent_demand_closed() {
    use observation::ids::{OwnerGeneration, OwnerGenerationExhausted};
    let mut consumer =
        BatteryDeliveryState::with_generation_for_test(OwnerGeneration(u32::MAX - 1));
    consumer.reconcile(true, Instant(0)).unwrap();
    let old = consumer.key().unwrap();
    consumer.reconcile(false, Instant(1)).unwrap();
    for at in [2, 3] {
        assert_eq!(
            consumer.reconcile(true, Instant(at)),
            Err(OwnerGenerationExhausted)
        );
        assert!(consumer.key().is_none());
        assert!(consumer.battery_demand(Instant(at)).is_empty());
        assert!(consumer
            .take_delivery(old, Some(battery_sample(1, at, 77)), Instant(at))
            .is_none());
    }
}

#[test]
fn battery_late_ui_poll_accepts_in_window_success_but_not_expired_completion() {
    let ingress = BatteryIngress::<2>::new();
    let mut consumer = battery_consumer(0);
    let key = consumer.key().unwrap();
    assert_eq!(
        admit_battery(&mut consumer, &ingress, None, 1),
        Some(BatteryEntryEvent::Admitted)
    );
    let before_expiry = battery_sample(1, 299999, 77);
    assert_eq!(
        admit_battery(&mut consumer, &ingress, Some(before_expiry), 300001),
        None
    );
    assert!(
        consumer
            .take_delivery(key, Some(before_expiry), Instant(300001))
            .unwrap()
            .deliver_values
    );
    consumer.reconcile(false, Instant(400000)).unwrap();
    consumer.reconcile(true, Instant(400001)).unwrap();
    assert_eq!(
        admit_battery(&mut consumer, &ingress, Some(before_expiry), 400001),
        Some(BatteryEntryEvent::Admitted)
    );
    let at_expiry = battery_sample(2, 700001, 76);
    assert_eq!(
        admit_battery(&mut consumer, &ingress, Some(at_expiry), 700001),
        Some(BatteryEntryEvent::Expired)
    );
    assert_eq!(
        admit_battery(&mut consumer, &ingress, Some(at_expiry), 700002),
        None
    );
}

type BatteryFixtureState = observation_fixture::FixtureState<battery_types::BatteryStateSnapshot>;
use fixture_protocol::{
    FixtureRequest as BatteryFixtureRequest, FixtureStatus as BatteryFixtureStatus,
};
type BatteryFixtureResult = fixture_protocol::FixtureResult<battery_types::BatteryStateSnapshot>;

fn fixture_request(id: u64, expiry: u64) -> BatteryFixtureRequest {
    BatteryFixtureRequest {
        id,
        expires_at: Instant(expiry),
    }
}

fn fixture_status(
    result: Option<BatteryFixtureResult>,
    status: BatteryFixtureStatus,
) -> BatteryFixtureResult {
    let result = result.expect("terminal fixture result");
    assert_eq!(result.status, status);
    result
}

#[test]
fn fixture_admits_one_session_and_reports_busy_without_replacing_it() {
    let mut fixture = BatteryFixtureState::default();
    let cached = battery_sample(1, 100, 77);
    let first = fixture_request(1, 200);
    assert!(fixture
        .begin(first, Instant(100), Some(cached), 0, |request, now| {
            assert_eq!(request.admitted_at, now);
            assert_eq!(request.expires_at, first.expires_at);
            assert_eq!(request.fields, BatteryFields::LEVEL);
            Ok(())
        })
        .is_none());
    let busy = fixture_status(
        fixture.begin(
            fixture_request(2, 200),
            Instant(101),
            Some(cached),
            0,
            |_, _| panic!("busy session admitted"),
        ),
        BatteryFixtureStatus::Busy,
    );
    assert_eq!(busy.id, 2);
    assert_eq!(busy.owner_generation.0, 0);
    assert!(busy.admitted_at.is_none());
    let sampled = fixture_status(
        fixture.poll(Instant(102), Some(battery_sample(2, 102, 76)), 0),
        BatteryFixtureStatus::Sampled,
    );
    assert_eq!(sampled.id, 1);
    assert_eq!(sampled.owner_generation.0, 1);
    assert_eq!(sampled.admitted_at, Some(Instant(100)));
    assert_eq!(sampled.baseline, Some((ProviderGeneration(0), Revision(1))));
    assert!(fixture
        .poll(Instant(103), Some(battery_sample(3, 103, 75)), 0)
        .is_none());
    // A rejected correlation ID was consumed too; retry requires a new ID.
    fixture_status(
        fixture.begin(fixture_request(2, 200), Instant(104), None, 0, |_, _| {
            panic!("duplicate id")
        }),
        BatteryFixtureStatus::StaleId,
    );
    assert!(fixture
        .begin(
            fixture_request(3, 200),
            Instant(104),
            None,
            0,
            |_, _| Ok(())
        )
        .is_none());
}

fn unexpected_fixture_admission(
    _request: observation::observe_now::ObserveNowRequest<BatteryFields>,
    _now: Instant,
) -> Result<(), RequestAdmissionError> {
    panic!("rejected fixture must not reach request admission")
}

#[test]
fn fixture_validates_ids_and_original_expiry_before_any_admission() {
    let mut fixture = BatteryFixtureState::default();
    fixture_status(
        fixture.begin(
            fixture_request(0, 200),
            Instant(100),
            None,
            0,
            unexpected_fixture_admission,
        ),
        BatteryFixtureStatus::Invalid,
    );
    fixture_status(
        fixture.begin(
            fixture_request(1, 100),
            Instant(100),
            None,
            0,
            unexpected_fixture_admission,
        ),
        BatteryFixtureStatus::Expired,
    );
    fixture_status(
        fixture.begin(
            fixture_request(1, 200),
            Instant(100),
            None,
            0,
            unexpected_fixture_admission,
        ),
        BatteryFixtureStatus::StaleId,
    );
    fixture_status(
        fixture.begin(
            fixture_request(2, 300101),
            Instant(100),
            None,
            0,
            unexpected_fixture_admission,
        ),
        BatteryFixtureStatus::Invalid,
    );
    assert!(fixture
        .begin(
            fixture_request(3, 300100),
            Instant(100),
            None,
            0,
            |request, _| {
                assert_eq!(request.expires_at, Instant(300100));
                Ok(())
            }
        )
        .is_none());
    // Invalid and stale IDs cannot disturb the pending valid request.
    fixture_status(
        fixture.begin(
            fixture_request(0, 200),
            Instant(100),
            None,
            0,
            unexpected_fixture_admission,
        ),
        BatteryFixtureStatus::Invalid,
    );
    fixture_status(
        fixture.begin(
            fixture_request(2, 200),
            Instant(100),
            None,
            0,
            unexpected_fixture_admission,
        ),
        BatteryFixtureStatus::StaleId,
    );
    assert_eq!(
        fixture_status(
            fixture.poll(Instant(101), Some(battery_sample(1, 101, 77)), 0),
            BatteryFixtureStatus::Sampled
        )
        .id,
        3
    );
}

#[test]
fn fixture_ingress_rejections_are_terminal_and_do_not_allocate_owner_generations() {
    let mut fixture = BatteryFixtureState::default();
    for (id, error, status) in [
        (1, RequestAdmissionError::Full, BatteryFixtureStatus::Full),
        (
            2,
            RequestAdmissionError::Closed,
            BatteryFixtureStatus::Closed,
        ),
        (
            3,
            RequestAdmissionError::Expired,
            BatteryFixtureStatus::Expired,
        ),
        (
            4,
            RequestAdmissionError::Invalid,
            BatteryFixtureStatus::Invalid,
        ),
    ] {
        let result = fixture_status(
            fixture.begin(fixture_request(id, 200), Instant(100), None, 0, |_, _| {
                Err(error)
            }),
            status,
        );
        assert_eq!(result.owner_generation.0, 0);
        assert!(fixture
            .poll(Instant(101), Some(battery_sample(1, 101, 77)), 0)
            .is_none());
    }
    let foreign = BatteryStateSnapshot {
        provider: observation::ids::ProviderId(99),
        ..battery_sample(1, 100, 77)
    };
    fixture_status(
        fixture.begin(
            fixture_request(5, 200),
            Instant(100),
            Some(foreign),
            0,
            |_, _| panic!("foreign baseline"),
        ),
        BatteryFixtureStatus::Unavailable,
    );
    assert!(fixture
        .begin(
            fixture_request(6, 200),
            Instant(100),
            None,
            0,
            |request, _| {
                assert_eq!(request.owner_generation.0, 1);
                Ok(())
            }
        )
        .is_none());
}

#[test]
fn fixture_same_millisecond_success_survives_failure_without_accepting_failure_only_revision() {
    let mut fixture = BatteryFixtureState::default();
    let cached = battery_sample(1, 100, 77);
    assert!(fixture
        .begin(
            fixture_request(1, 200),
            Instant(100),
            Some(cached),
            0,
            |_, _| Ok(())
        )
        .is_none());
    assert!(fixture.poll(Instant(100), Some(cached), 0).is_none());
    let failure = BatteryStateSnapshot {
        health: Health::Degraded,
        revision: Revision(2),
        last_attempt_at: Some(Instant(101)),
        ..cached
    };
    assert!(fixture.poll(Instant(101), Some(failure), 0).is_none());
    let success_then_failure = BatteryStateSnapshot {
        health: Health::Degraded,
        revision: Revision(4),
        last_attempt_at: Some(Instant(102)),
        ..battery_sample(3, 100, 76)
    };
    let result = fixture_status(
        fixture.poll(Instant(102), Some(success_then_failure), 0),
        BatteryFixtureStatus::Sampled,
    );
    let state = result.state.unwrap();
    assert_eq!(state.last_sample_at, Some(Instant(100)));
    assert_eq!(state.last_sample_revision, Some(Revision(3)));
    assert_eq!(state.revision, Revision(4));
    assert_eq!(state.snapshot.percent, 76);
}

#[test]
fn fixture_ignores_missing_or_inconsistent_success_metadata() {
    let mut fixture = BatteryFixtureState::default();
    let cached = battery_sample(3, 100, 77);
    assert!(fixture
        .begin(
            fixture_request(1, 200),
            Instant(100),
            Some(cached),
            0,
            |_, _| Ok(())
        )
        .is_none());
    let valid = battery_sample(4, 101, 76);
    for invalid in [
        BatteryStateSnapshot {
            last_sample_at: None,
            ..valid
        },
        BatteryStateSnapshot {
            last_sample_revision: None,
            ..valid
        },
        BatteryStateSnapshot {
            last_sample_revision: Some(Revision(2)),
            ..valid
        },
        BatteryStateSnapshot {
            last_sample_revision: Some(Revision(5)),
            ..valid
        },
        BatteryStateSnapshot {
            health: Health::Unknown,
            ..valid
        },
        BatteryStateSnapshot {
            health: Health::Failed,
            ..valid
        },
    ] {
        assert!(fixture.poll(Instant(102), Some(invalid), 0).is_none());
    }
    fixture_status(
        fixture.poll(Instant(102), Some(valid), 0),
        BatteryFixtureStatus::Sampled,
    );
}

#[test]
fn fixture_ignores_foreign_and_old_generations_but_reports_provider_restart() {
    let mut fixture = BatteryFixtureState::default();
    let current = BatteryStateSnapshot {
        generation: ProviderGeneration(2),
        ..battery_sample(3, 100, 77)
    };
    assert!(fixture
        .begin(
            fixture_request(1, 200),
            Instant(100),
            Some(current),
            0,
            |_, _| Ok(())
        )
        .is_none());
    let foreign = BatteryStateSnapshot {
        provider: observation::ids::ProviderId(99),
        generation: ProviderGeneration(3),
        ..battery_sample(4, 101, 76)
    };
    let old = BatteryStateSnapshot {
        generation: ProviderGeneration(1),
        ..battery_sample(4, 101, 76)
    };
    for state in [foreign, old] {
        assert!(fixture.poll(Instant(101), Some(state), 0).is_none());
    }
    let restarted = BatteryStateSnapshot {
        generation: ProviderGeneration(3),
        ..battery_sample(1, 102, 76)
    };
    let result = fixture_status(
        fixture.poll(Instant(102), Some(restarted), 0),
        BatteryFixtureStatus::Restarted,
    );
    assert_eq!(result.state.unwrap().generation, ProviderGeneration(3));
    assert!(fixture.poll(Instant(103), Some(restarted), 0).is_none());
}

#[test]
fn fixture_learns_provider_generation_without_treating_missing_cache_as_success() {
    let mut fixture = BatteryFixtureState::default();
    assert!(fixture
        .begin(
            fixture_request(1, 200),
            Instant(100),
            None,
            0,
            |_, _| Ok(())
        )
        .is_none());
    let unknown = BatteryStateSnapshot {
        generation: ProviderGeneration(2),
        health: Health::Unknown,
        last_sample_at: None,
        last_sample_revision: None,
        ..battery_sample(0, 100, 0)
    };
    assert!(fixture.poll(Instant(101), Some(unknown), 0).is_none());
    let restarted = BatteryStateSnapshot {
        generation: ProviderGeneration(3),
        ..battery_sample(1, 102, 77)
    };
    fixture_status(
        fixture.poll(Instant(102), Some(restarted), 0),
        BatteryFixtureStatus::Restarted,
    );
}

#[test]
fn fixture_closure_cancels_before_sample_or_restart_and_never_retries() {
    let mut fixture = BatteryFixtureState::default();
    let cached = battery_sample(1, 100, 77);
    assert!(fixture
        .begin(
            fixture_request(1, 200),
            Instant(100),
            Some(cached),
            0,
            |_, _| Ok(())
        )
        .is_none());
    let restarted = BatteryStateSnapshot {
        generation: ProviderGeneration(1),
        ..battery_sample(2, 101, 76)
    };
    let result = fixture_status(
        fixture.poll(Instant(102), Some(restarted), 1),
        BatteryFixtureStatus::Cancelled,
    );
    assert_eq!(result.admitted_at, Some(Instant(100)));
    assert!(result.state.is_none());
    assert!(fixture.poll(Instant(103), Some(restarted), 1).is_none());
    assert!(fixture
        .begin(
            fixture_request(2, 200),
            Instant(104),
            Some(restarted),
            1,
            |request, _| {
                assert_eq!(request.owner_generation.0, 2);
                Ok(())
            }
        )
        .is_none());
}

#[test]
fn fixture_expiry_uses_completion_time_and_keeps_original_command_deadline() {
    let cached = battery_sample(1, 99, 77);
    let mut fixture = BatteryFixtureState::default();
    assert!(fixture
        .begin(
            fixture_request(1, 200),
            Instant(100),
            Some(cached),
            0,
            |_, _| Ok(())
        )
        .is_none());
    assert!(fixture
        .poll(Instant(101), Some(battery_sample(2, 99, 76)), 0)
        .is_none());
    fixture_status(
        fixture.poll(Instant(200), Some(battery_sample(2, 200, 76)), 0),
        BatteryFixtureStatus::Expired,
    );
    assert!(fixture
        .poll(Instant(201), Some(battery_sample(3, 201, 75)), 0)
        .is_none());
    assert!(fixture
        .begin(
            fixture_request(2, 300),
            Instant(210),
            Some(cached),
            0,
            |_, _| Ok(())
        )
        .is_none());
    fixture_status(
        fixture.poll(Instant(310), Some(battery_sample(2, 299, 76)), 0),
        BatteryFixtureStatus::Sampled,
    );
}

#[test]
fn authority_fixture_preserves_live_demand_key_interval_and_provider_owned_credits() {
    let mut authority = battery_consumer(0);
    let key = authority.key().unwrap();
    let ingress = BatteryIngress::<2>::new();
    let cached = battery_sample(1, 10, 77);
    assert_eq!(
        admit_battery(&mut authority, &ingress, Some(cached), 10),
        Some(BatteryEntryEvent::Admitted)
    );
    authority
        .take_delivery(key, Some(cached), Instant(10))
        .unwrap();
    let live_success = battery_sample(2, 11, 76);
    authority
        .take_delivery(key, Some(live_success), Instant(11))
        .unwrap();
    let live_request = ingress.try_receive().unwrap();
    ingress.complete(1); // Provider-owned completion, not the authority.
    assert!(authority
        .begin_fixture(
            fixture_request(1, 200),
            Instant(12),
            Some(live_success),
            0,
            |request, now| {
                assert_ne!(request.owner, live_request.owner);
                ingress.try_admit(request, now)
            }
        )
        .is_none());
    assert_eq!(authority.key(), Some(key));
    assert!(authority.fixture_pending());
    let demand = authority.battery_demand(Instant(12));
    assert_eq!(demand.fields, BatteryFields::LEVEL);
    assert_eq!(
        demand.max_age_at(0),
        Some(observation::time::Duration::from_secs(300))
    );
    let fixture_success = battery_sample(3, 13, 75);
    fixture_status(
        authority.poll_fixture(Instant(13), Some(fixture_success), 0),
        BatteryFixtureStatus::Sampled,
    );
    assert!(authority
        .take_delivery(key, Some(fixture_success), Instant(13))
        .is_none());
    assert!(!authority.fixture_pending());
    assert_eq!(ingress.outstanding(), 1); // UI terminal result cannot free credit.
    assert_eq!(authority.key(), Some(key));
    assert_eq!(
        authority.battery_demand(Instant(300000)).max_age_at(0),
        demand.max_age_at(0)
    );
    assert!(authority
        .poll_fixture(Instant(300000), Some(fixture_success), 0)
        .is_none());
}

#[test]
fn fixture_can_run_without_a_live_trace_subscription() {
    let mut authority = BatteryDeliveryState::default();
    assert!(authority.key().is_none());
    assert!(authority
        .begin_fixture(
            fixture_request(1, 200),
            Instant(100),
            None,
            0,
            |_, _| Ok(())
        )
        .is_none());
    fixture_status(
        authority.poll_fixture(Instant(101), Some(battery_sample(1, 101, 77)), 0),
        BatteryFixtureStatus::Sampled,
    );
    assert!(authority.key().is_none());
    assert!(authority.battery_demand(Instant(101)).is_empty());
}

#[test]
fn provider_expires_fixture_and_returns_credit_without_any_ui_poll() {
    use core::{
        cell::Cell,
        future::Future,
        task::{Context, Poll, Waker},
    };
    use observation::{
        runtime::{AcquisitionDriver, ProviderLoop, StepOutcome},
        time::Duration,
    };

    struct Driver<'a>(&'a Cell<u32>);
    impl AcquisitionDriver<BatteryFields, BatterySnapshot> for Driver<'_> {
        type Error = ();
        async fn acquire(
            &mut self,
            fields: BatteryFields,
        ) -> Result<(BatteryFields, BatterySnapshot), ()> {
            self.0.set(self.0.get() + 1);
            Ok((fields, BatterySnapshot { percent: 77 }))
        }
        async fn cancel(&mut self) -> Result<(), ()> {
            Ok(())
        }
        fn min_acquisition_interval(&self) -> Duration {
            Duration::from_millis(1000)
        }
    }
    fn immediately<F: Future>(future: F) -> F::Output {
        let mut future = core::pin::pin!(future);
        match future
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            Poll::Ready(result) => result,
            Poll::Pending => panic!("unexpected driver wait"),
        }
    }
    let calls = Cell::new(0);
    let ingress = BatteryIngress::<2>::new();
    let mut authority = battery_consumer(0);
    let key = authority.key().unwrap();
    let demand = authority.battery_demand(Instant(100));
    let mut provider = ProviderLoop::<_, _, _, 2, 1>::new(
        Driver(&calls),
        BATTERY_PROVIDER_ID,
        ProviderGeneration::INITIAL,
        BatterySnapshot::default(),
    );
    assert!(matches!(
        immediately(provider.step(&demand, || Instant(100))),
        StepOutcome::Acquired { .. }
    ));
    let cached = battery_sample(1, 100, 77);
    assert!(authority
        .begin_fixture(
            fixture_request(1, 200),
            Instant(101),
            Some(cached),
            0,
            |request, now| ingress.try_admit(request, now)
        )
        .is_none());
    provider
        .admit_observe_now_at(ingress.try_receive().unwrap(), Instant(101))
        .unwrap();
    assert!(matches!(
        immediately(provider.step(&demand, || Instant(101))),
        StepOutcome::Idle {
            next_wake: Some(Instant(200))
        }
    ));
    // UI is not polled again until after expiry. Provider timer ownership still
    // removes the one-shot, frees transport credit and keeps normal demand.
    let pending = provider.pending_observe_now();
    assert!(matches!(
        immediately(provider.step(&demand, || Instant(200))),
        StepOutcome::Idle {
            next_wake: Some(Instant(300100))
        }
    ));
    ingress.complete(pending - provider.pending_observe_now());
    assert_eq!(ingress.outstanding(), 0);
    assert_eq!(calls.get(), 1);
    assert_eq!(authority.key(), Some(key));
    assert_eq!(
        authority.battery_demand(Instant(201)).max_age_at(0),
        demand.max_age_at(0)
    );
    fixture_status(
        authority.poll_fixture(Instant(201), Some(cached), 0),
        BatteryFixtureStatus::Expired,
    );
    assert!(authority
        .poll_fixture(Instant(202), Some(cached), 0)
        .is_none());
}

#[test]
fn bme_fixture_without_home_preserves_demand_and_accepts_retained_in_window_success() {
    use observation::field::FieldMask;
    let mut authority = EnvironmentDeliveryState::default();
    let cached = sample(1, 100, 2300, 41000);
    let request = fixture_request(1, 200);
    assert!(authority
        .begin_fixture(request, Instant(100), Some(cached), 0, |observe, _| {
            assert_eq!(observe.fields.bits(), 3);
            assert_eq!(observe.expires_at, request.expires_at);
            Ok(())
        })
        .is_none());
    assert!(authority.environment_demand(Instant(100)).fields.is_empty());
    assert!(authority.fixture_pending());
    // Failure-only revisions cannot qualify the cached sample.
    let failed = EnvironmentStateSnapshot {
        revision: Revision(2),
        health: Health::Degraded,
        last_attempt_at: Some(Instant(101)),
        ..cached
    };
    assert!(authority
        .poll_fixture(Instant(101), Some(failed), 0)
        .is_none());
    // A second conversion can complete in the same millisecond. A subsequent
    // failure retains its successful identity, even if UI polling is delayed.
    let retained = EnvironmentStateSnapshot {
        revision: Revision(4),
        health: Health::Degraded,
        last_attempt_at: Some(Instant(110)),
        ..sample(3, 100, -250, 40000)
    };
    let result = authority
        .poll_fixture(Instant(250), Some(retained), 0)
        .unwrap();
    assert!(!authority.fixture_pending());
    assert_eq!(result.status, BatteryFixtureStatus::Sampled);
    assert_eq!(
        result.state.unwrap().last_sample_revision,
        Some(Revision(3))
    );
    let wire = result.to_string();
    assert!(wire.contains("provider=1 fields=3 status=Sampled"));
    assert!(wire.contains("temperature_centidegrees=-250 humidity_millipercent=40000"));
    assert!(!wire.contains(" percent="));
    assert!(authority
        .poll_fixture(Instant(251), Some(retained), 0)
        .is_none());
    assert!(authority.environment_demand(Instant(251)).fields.is_empty());
}

#[test]
fn bme_fixture_rejection_expiry_restart_and_close_leave_live_demand_intact() {
    use observation::field::FieldMask;
    let owner = shell().active_instance();
    for (new_generation, close_generation, now, expected) in [
        (0, 0, 200, BatteryFixtureStatus::Expired),
        (1, 0, 101, BatteryFixtureStatus::Restarted),
        (0, 1, 101, BatteryFixtureStatus::Cancelled),
    ] {
        let mut authority = consumer(owner);
        let cached = sample(1, 100, 2300, 41000);
        for (id, error, expected) in [
            (1, RequestAdmissionError::Full, BatteryFixtureStatus::Full),
            (
                2,
                RequestAdmissionError::Closed,
                BatteryFixtureStatus::Closed,
            ),
        ] {
            let rejection = authority
                .begin_fixture(
                    fixture_request(id, 200),
                    Instant(100),
                    Some(cached),
                    0,
                    |_, _| Err(error),
                )
                .unwrap();
            assert_eq!(rejection.status, expected);
        }
        assert!(authority
            .begin_fixture(
                fixture_request(3, 200),
                Instant(100),
                Some(cached),
                0,
                |_, _| Ok(())
            )
            .is_none());
        let state = EnvironmentStateSnapshot {
            generation: ProviderGeneration(new_generation),
            ..cached
        };
        assert_eq!(
            authority
                .poll_fixture(Instant(now), Some(state), close_generation)
                .unwrap()
                .status,
            expected
        );
        let demand = authority.environment_demand(Instant(now));
        assert_eq!(demand.fields.bits(), 3);
        assert_eq!(
            demand.max_age_at(0),
            Some(observation::time::Duration::from_secs(300))
        );
    }
}

#[test]
fn bme_projection_tracks_attempts_without_fabricating_success() {
    use observation::field::FieldMask;
    let mut state = observation::state::ObservationState::<_, 2>::new(
        environment_types::ENVIRONMENT_PROVIDER_ID,
        ProviderGeneration(0),
        sample(0, 0, 0, 0).snapshot,
    );
    state.apply_failure(Instant(10)).unwrap();
    let failed = EnvironmentStateSnapshot::from_observation(&state, None);
    assert_eq!(failed.provider, environment_types::ENVIRONMENT_PROVIDER_ID);
    assert_eq!(failed.last_attempt_at, Some(Instant(10)));
    assert_eq!(failed.sample_identity(), None);
    let fields = environment_types::EnvironmentFields::TEMPERATURE
        .union(environment_types::EnvironmentFields::HUMIDITY);
    state
        .apply_success(fields, Instant(20), |value| {
            *value = sample(0, 0, 2300, 40000).snapshot
        })
        .unwrap();
    let success = state.revision;
    state.apply_failure(Instant(30)).unwrap();
    let projected = EnvironmentStateSnapshot::from_observation(&state, Some(success));
    assert_eq!(
        projected.sample_identity(),
        Some((ProviderGeneration(0), Revision(2)))
    );
    assert_eq!(projected.last_sample_at, Some(Instant(20)));
    assert_eq!(projected.last_attempt_at, Some(Instant(30)));
    assert_eq!(projected.health, Health::Degraded);
}

#[test]
fn bme_live_entry_uses_success_identity_for_equal_timestamp_conversion() {
    let owner = shell().active_instance();
    let mut authority = consumer(owner);
    let cached = sample(1, 100, 2200, 40000);
    authority
        .take_delivery(owner, Some(cached), Instant(100))
        .unwrap();
    authority.admit_entry(Instant(100), Some(cached), 0, |_, _| Ok(()));
    let fresh = sample(2, 100, 2300, 41000);
    assert!(
        authority
            .take_delivery(owner, Some(fresh), Instant(101))
            .unwrap()
            .deliver_values
    );
    assert!(authority
        .take_delivery(owner, Some(fresh), Instant(102))
        .is_none());
}
