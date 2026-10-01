use super::*;
use observation::ids::Revision;
use shell::model::{ShellModel, ShellNavigationError};
use shell::types::{
    NavIntent, RefreshHint, SurfaceCapabilities, SurfaceRef, SurfaceRole, SurfaceSpec,
};

type TestShell = ShellModel<2, 4, 4, 1, 1, 2>;
const SURFACES: &[SurfaceSpec] = &[
    SurfaceSpec::new(
        catalogue::HOME_SURFACE_ID.0,
        SurfaceRole::Ambient,
        SurfaceCapabilities::AMBIENT,
        RefreshHint::Content,
    ),
    SurfaceSpec::new(
        catalogue::LAUNCHER_SURFACE_ID.0,
        SurfaceRole::Launcher,
        SurfaceCapabilities::NONE,
        RefreshHint::Content,
    ),
];

fn setup() -> (TestShell, HomeObservations) {
    let shell = TestShell::new(
        catalogue::BASE_PROVIDER_ID,
        SURFACES,
        catalogue::HOME_SURFACE_ID,
    )
    .unwrap();
    let mut home = HomeObservations::new();
    home.committed(shell.active_instance(), Instant(0)).unwrap();
    (shell, home)
}

fn launcher(shell: &TestShell) -> SurfaceRef {
    SurfaceRef {
        owner: shell.active().surface.owner,
        id: catalogue::LAUNCHER_SURFACE_ID,
    }
}

fn navigate(shell: &mut TestShell, home: &mut HomeObservations, intent: NavIntent) {
    let prepared = shell.prepare_intent(intent).unwrap();
    shell.commit_navigation(prepared).unwrap();
    home.committed(shell.active_instance(), Instant(0)).unwrap();
}

fn environment() -> EnvironmentStateSnapshot {
    EnvironmentStateSnapshot {
        provider: ENVIRONMENT_PROVIDER_ID,
        last_attempt_at: Some(Instant(1)),
        generation: ProviderGeneration::INITIAL,
        revision: Revision(1),
        health: Health::Ok,
        last_sample_at: Some(Instant(1)),
        last_sample_revision: Some(Revision(1)),
        snapshot: EnvironmentSnapshot {
            temperature_millicelsius: 23_500,
            humidity_millipercent: 42_000,
        },
    }
}

fn battery() -> BatteryStateSnapshot {
    BatteryStateSnapshot {
        provider: BATTERY_PROVIDER_ID,
        last_attempt_at: Some(Instant(1)),
        generation: ProviderGeneration::INITIAL,
        revision: Revision(1),
        health: Health::Ok,
        last_sample_at: Some(Instant(1)),
        last_sample_revision: Some(Revision(1)),
        snapshot: BatterySnapshot {
            millivolts: 3800,
            percent: 71,
        },
    }
}

#[test]
fn initial_committed_home_installs_both_existing_slots() {
    let (_, home) = setup();
    let environment = home.environment_demand(Instant(1));
    let battery = home.battery_demand(Instant(1));
    assert_eq!(environment.fields.bits(), 3);
    assert_eq!(battery.fields.bits(), 3);
    for index in 0..FIELDS {
        assert_eq!(
            environment.max_age_at(index),
            Some(Duration::from_secs(SAMPLE_INTERVAL_S as u32))
        );
        assert_eq!(battery.max_age_at(index), environment.max_age_at(index));
    }
    assert_eq!(
        home.environment_key().unwrap().owner_generation,
        OwnerGeneration(1)
    );
    assert_eq!(
        home.battery_key().unwrap().owner_generation,
        OwnerGeneration(1)
    );
}

#[test]
fn unchanged_commit_preserves_identity_and_delivery_cadence() {
    let (shell, mut home) = setup();
    let key = home.environment_key().unwrap();
    assert_eq!(
        home.poll_environment(key, environment(), Instant(1))
            .unwrap()
            .sample,
        Some(environment().snapshot)
    );
    home.committed(shell.active_instance(), Instant(0)).unwrap();
    assert_eq!(home.environment_key(), Some(key));
    assert!(home
        .poll_environment(key, environment(), Instant(2))
        .is_none());
}

#[test]
fn committed_departure_removes_demand_and_reentry_renews_owner() {
    let (mut shell, mut home) = setup();
    let old = home.environment_key().unwrap();
    let old_battery = home.battery_key().unwrap();
    let launcher = launcher(&shell);
    navigate(&mut shell, &mut home, NavIntent::OpenLauncher(launcher));
    assert!(home.environment_demand(Instant(2)).is_empty());
    assert!(home.battery_demand(Instant(2)).is_empty());
    assert!(home
        .poll_environment(old, environment(), Instant(2))
        .is_none());
    assert!(home
        .poll_battery(old_battery, battery(), Instant(2))
        .is_none());
    navigate(&mut shell, &mut home, NavIntent::Back);
    let key = home.environment_key().unwrap();
    assert!(key.owner_generation > old.owner_generation);
    assert!(home
        .poll_environment(key, environment(), Instant(3))
        .unwrap()
        .sample
        .is_some());
}

#[test]
fn prepared_or_failed_candidate_does_not_change_committed_owner() {
    let (mut shell, mut home) = setup();
    let old = home.environment_key();
    let launcher = launcher(&shell);
    let candidate = shell
        .prepare_intent(NavIntent::OpenLauncher(launcher))
        .unwrap();
    assert_eq!(home.environment_key(), old);
    // A discarded candidate has no adapter call and consumes no observation epoch.
    drop(candidate);
    assert_eq!(home.environment_key(), old);
    let stale = shell
        .prepare_intent(NavIntent::OpenLauncher(launcher))
        .unwrap();
    navigate(&mut shell, &mut home, NavIntent::OpenLauncher(launcher));
    assert_eq!(
        shell.commit_navigation(stale),
        Err(ShellNavigationError::StalePlan)
    );
    assert!(home.environment_key().is_none());
    navigate(&mut shell, &mut home, NavIntent::Back);
    assert_eq!(
        home.environment_key().unwrap().owner_generation,
        OwnerGeneration(2)
    );
}

#[test]
fn retained_shell_wake_and_rejected_sleep_rebuild_renew_identity_and_projection() {
    let (shell, mut home) = setup();
    let retained = shell.active_instance();
    for _ in 0..2 {
        let old = home.environment_key().unwrap();
        home.deactivate();
        assert!(home.environment_demand(Instant(2)).is_empty());
        assert!(home.battery_demand(Instant(2)).is_empty());
        home.rebuilt(retained, Instant(2)).unwrap();
        let current = home.environment_key().unwrap();
        assert!(current.owner_generation > old.owner_generation);
        let delivery = home
            .poll_environment(current, environment(), Instant(2))
            .unwrap();
        assert_eq!(delivery.health, Some(Health::Ok));
        assert_eq!(delivery.sample, Some(environment().snapshot));
    }
}

#[test]
fn stale_key_cannot_change_replacement_tracking_or_health() {
    let (shell, mut home) = setup();
    let old_environment = home.environment_key().unwrap();
    let old_battery = home.battery_key().unwrap();
    home.rebuilt(shell.active_instance(), Instant(2)).unwrap();
    assert!(home
        .poll_environment(old_environment, environment(), Instant(2))
        .is_none());
    assert!(home
        .poll_battery(old_battery, battery(), Instant(2))
        .is_none());
    let current_environment = home.environment_key().unwrap();
    let current_battery = home.battery_key().unwrap();
    assert_eq!(
        home.poll_environment(current_environment, environment(), Instant(3))
            .unwrap()
            .health,
        Some(Health::Ok)
    );
    assert_eq!(
        home.poll_battery(current_battery, battery(), Instant(3))
            .unwrap()
            .health,
        Some(Health::Ok)
    );
}

#[test]
fn health_without_samples_never_projects_zero_readings() {
    let (_, mut home) = setup();
    let environment_key = home.environment_key().unwrap();
    let battery_key = home.battery_key().unwrap();
    for health in [Health::Unknown, Health::Failed] {
        let environment = EnvironmentStateSnapshot {
            health,
            ..Default::default()
        };
        let battery = BatteryStateSnapshot {
            health,
            ..Default::default()
        };
        assert_eq!(
            home.poll_environment(environment_key, environment, Instant(1)),
            Some(Delivery {
                health: Some(health),
                sample: None
            })
        );
        assert_eq!(
            home.poll_battery(battery_key, battery, Instant(1)),
            Some(Delivery {
                health: Some(health),
                sample: None
            })
        );
        assert!(home
            .poll_environment(environment_key, environment, Instant(2))
            .is_none());
    }
}

#[test]
fn health_transition_bypasses_cadence_and_retains_last_good_value() {
    let (_, mut home) = setup();
    let environment_key = home.environment_key().unwrap();
    let battery_key = home.battery_key().unwrap();
    home.poll_environment(environment_key, environment(), Instant(1))
        .unwrap();
    home.poll_battery(battery_key, battery(), Instant(1))
        .unwrap();
    let failed_environment = EnvironmentStateSnapshot {
        health: Health::Degraded,
        revision: Revision(2),
        ..environment()
    };
    let failed_battery = BatteryStateSnapshot {
        health: Health::Degraded,
        revision: Revision(2),
        ..battery()
    };
    assert_eq!(
        home.poll_environment(environment_key, failed_environment, Instant(2)),
        Some(Delivery {
            health: Some(Health::Degraded),
            sample: Some(environment().snapshot)
        })
    );
    assert_eq!(
        home.poll_battery(battery_key, failed_battery, Instant(2)),
        Some(Delivery {
            health: Some(Health::Degraded),
            sample: Some(battery().snapshot)
        })
    );
}

#[test]
fn provider_restart_reprojects_unchanged_health_and_rejects_older_generation() {
    let (_, mut home) = setup();
    let key = home.environment_key().unwrap();
    home.poll_environment(key, environment(), Instant(1))
        .unwrap();
    let restarted = EnvironmentStateSnapshot {
        provider: ENVIRONMENT_PROVIDER_ID,
        last_attempt_at: Some(Instant(1)),
        generation: ProviderGeneration(1),
        revision: Revision(0),
        ..environment()
    };
    assert_eq!(
        home.poll_environment(key, restarted, Instant(2))
            .unwrap()
            .health,
        Some(Health::Ok)
    );
    assert!(home
        .poll_environment(key, environment(), Instant(3))
        .is_none());
    assert!(home.poll_environment(key, restarted, Instant(3)).is_none());
}

#[test]
fn owner_generation_exhaustion_repeatedly_keeps_both_books_closed() {
    let (shell, mut home) = setup();
    let old = home.environment_key().unwrap();
    home.generation = OwnerGeneration(u32::MAX);
    for _ in 0..2 {
        assert_eq!(
            home.rebuilt(shell.active_instance(), Instant(2)),
            Err(OwnerGenerationExhausted)
        );
        assert!(home.environment_key().is_none());
        assert!(home.battery_key().is_none());
        assert!(home.environment_demand(Instant(2)).is_empty());
        assert!(home.battery_demand(Instant(2)).is_empty());
        assert!(home
            .poll_environment(old, environment(), Instant(2))
            .is_none());
    }
}

fn fresh_result_contract<F: FieldMask, S: Copy + core::fmt::Debug + PartialEq>(
    fields: F,
    cached: StateSnapshot<S>,
    entry_at: u64,
) {
    let mut consumer = Consumer::new();
    consumer.install(
        cached.provider,
        OwnerGeneration(1),
        fields,
        Instant(entry_at),
    );
    let key = consumer.key().unwrap();
    assert_eq!(
        consumer.admit(Instant(entry_at + 20), Some(cached), 0, |request, now| {
            assert_eq!(request.admitted_at, now);
            assert_eq!(request.expires_at, Instant(entry_at + 60_000));
            assert_eq!(request.fields.bits(), fields.bits());
            Ok(())
        }),
        Some(EntryAdmissionEvent::Admitted)
    );
    assert_eq!(
        consumer
            .poll(key, cached, Instant(entry_at + 20))
            .unwrap()
            .sample,
        Some(cached.snapshot)
    );
    // Fresh cache does not represent the requested new acquisition.
    assert!(consumer.poll(key, cached, Instant(entry_at + 21)).is_none());
    let failed = StateSnapshot {
        health: Health::Degraded,
        revision: Revision(2),
        ..cached
    };
    let health = consumer.poll(key, failed, Instant(entry_at + 22)).unwrap();
    assert_eq!(health.health, Some(Health::Degraded));
    assert_eq!(health.sample, Some(cached.snapshot));
    assert!(consumer
        .poll(
            key,
            StateSnapshot {
                revision: Revision(3),
                ..failed
            },
            Instant(entry_at + 23)
        )
        .is_none());
    // Keeping degraded health proves the requested result, not a health
    // transition, bypasses the ordinary one-minute delivery interval.
    let completed = StateSnapshot {
        revision: Revision(4),
        last_sample_revision: Some(Revision(4)),
        last_sample_at: Some(Instant(entry_at + 24)),
        ..failed
    };
    assert_eq!(
        consumer
            .poll(key, completed, Instant(entry_at + 24))
            .unwrap()
            .sample,
        Some(cached.snapshot)
    );
    let periodic = StateSnapshot {
        revision: Revision(5),
        last_sample_revision: Some(Revision(5)),
        last_sample_at: Some(Instant(entry_at + 25)),
        ..completed
    };
    assert!(consumer
        .poll(key, periodic, Instant(entry_at + 25))
        .is_none());
    assert_eq!(
        consumer.admit(Instant(entry_at + 26), Some(periodic), 0, |_, _| panic!(
            "entry already satisfied"
        )),
        None
    );
}

#[test]
fn both_providers_preserve_fresh_result_bypass_across_fresh_or_stale_cache_and_failure_revisions() {
    for entry_at in [0, 60_000] {
        fresh_result_contract(
            EnvironmentFields::TEMPERATURE.union(EnvironmentFields::HUMIDITY),
            environment(),
            entry_at,
        );
        fresh_result_contract(
            BatteryFields::VOLTAGE.union(BatteryFields::LEVEL),
            battery(),
            entry_at,
        );
    }
}

fn admission_bounds<F: FieldMask, S: Copy>(fields: F, cached: StateSnapshot<S>) {
    let mut consumer = Consumer::new();
    consumer.install(cached.provider, OwnerGeneration(1), fields, Instant(100));
    for (at, error, event) in [
        (
            101,
            RequestAdmissionError::Full,
            Some(EntryAdmissionEvent::Rejected(RequestAdmissionError::Full)),
        ),
        (102, RequestAdmissionError::Full, None),
        (
            103,
            RequestAdmissionError::Closed,
            Some(EntryAdmissionEvent::Rejected(RequestAdmissionError::Closed)),
        ),
        (104, RequestAdmissionError::Closed, None),
    ] {
        assert_eq!(
            consumer.admit(Instant(at), Some(cached), 0, |_, _| Err(error)),
            event
        );
    }
    assert_eq!(
        consumer.admit(Instant(60_099), Some(cached), 1, |request, now| {
            assert_eq!(request.admitted_at, now);
            assert_eq!(request.expires_at, Instant(60_100));
            Ok(())
        }),
        Some(EntryAdmissionEvent::Admitted)
    );
    assert_eq!(
        consumer.admit(Instant(60_100), Some(cached), 1, |_, _| panic!(
            "expired admission"
        )),
        Some(EntryAdmissionEvent::Expired)
    );
    assert_eq!(
        consumer.admit(Instant(60_101), Some(cached), 1, |_, _| panic!(
            "expiry is terminal"
        )),
        None
    );
    assert!(consumer.key().is_some());
    // An entry never admitted also expires once, with no eventual retry.
    consumer.install(cached.provider, OwnerGeneration(2), fields, Instant(70_000));
    assert_eq!(
        consumer.admit(Instant(70_001), Some(cached), 1, |_, _| Err(
            RequestAdmissionError::Closed
        )),
        Some(EntryAdmissionEvent::Rejected(RequestAdmissionError::Closed))
    );
    assert_eq!(
        consumer.admit(Instant(130_000), Some(cached), 1, |_, _| panic!(
            "expired pending entry"
        )),
        Some(EntryAdmissionEvent::Expired)
    );
    assert_eq!(
        consumer.admit(Instant(130_001), Some(cached), 1, |_, _| panic!(
            "no deadline renewal"
        )),
        None
    );
}

#[test]
fn both_providers_report_rejection_transitions_and_keep_original_entry_expiry() {
    admission_bounds(
        EnvironmentFields::TEMPERATURE.union(EnvironmentFields::HUMIDITY),
        environment(),
    );
    admission_bounds(
        BatteryFields::VOLTAGE.union(BatteryFields::LEVEL),
        battery(),
    );
}

fn suspension_epoch_contract<F: FieldMask, S: Copy>(fields: F, cached: StateSnapshot<S>) {
    let mut consumer = Consumer::new();
    consumer.install(cached.provider, OwnerGeneration(1), fields, Instant(0));
    let key = consumer.key().unwrap();
    assert_eq!(
        consumer.admit(Instant(20), Some(cached), 0, |_, _| Ok(())),
        Some(EntryAdmissionEvent::Admitted)
    );
    assert!(consumer
        .poll(key, cached, Instant(20))
        .unwrap()
        .sample
        .is_some());
    // The UI missed both closure and resume. The newer cached stamp cannot
    // satisfy the cancelled entry: re-admission establishes a new baseline.
    let resumed = StateSnapshot {
        revision: Revision(2),
        last_sample_revision: Some(Revision(2)),
        last_sample_at: Some(Instant(25)),
        ..cached
    };
    assert_eq!(
        consumer.admit(Instant(30), Some(resumed), 1, |request, now| {
            assert_eq!(request.admitted_at, now);
            assert_eq!(request.expires_at, Instant(60_000));
            Ok(())
        }),
        Some(EntryAdmissionEvent::Admitted)
    );
    assert!(consumer.poll(key, resumed, Instant(30)).is_none());
    let before_admission = StateSnapshot {
        revision: Revision(3),
        last_sample_revision: Some(Revision(3)),
        last_sample_at: Some(Instant(29)),
        ..resumed
    };
    assert!(consumer.poll(key, before_admission, Instant(31)).is_none());
    let completed = StateSnapshot {
        revision: Revision(4),
        last_sample_revision: Some(Revision(4)),
        last_sample_at: Some(Instant(32)),
        ..resumed
    };
    assert!(consumer
        .poll(key, completed, Instant(32))
        .unwrap()
        .sample
        .is_some());
    assert!(consumer.poll(key, completed, Instant(33)).is_none());
}

#[test]
fn both_providers_retry_missed_suspend_epoch_without_reusing_cache_or_extending_expiry() {
    suspension_epoch_contract(
        EnvironmentFields::TEMPERATURE.union(EnvironmentFields::HUMIDITY),
        environment(),
    );
    suspension_epoch_contract(
        BatteryFields::VOLTAGE.union(BatteryFields::LEVEL),
        battery(),
    );
}

#[test]
fn public_entry_apis_share_owner_but_admit_and_deliver_each_provider_independently() {
    let (shell, mut home) = setup();
    let environment_key = home.environment_key().unwrap();
    let battery_key = home.battery_key().unwrap();
    let mut admitted_environment = None;
    let mut admitted_battery = None;
    assert_eq!(
        home.admit_environment(Instant(20), Some(environment()), 0, |request, _| {
            admitted_environment = Some(request);
            Ok(())
        }),
        Some(EntryAdmissionEvent::Admitted)
    );
    assert_eq!(
        home.admit_battery(Instant(20), Some(battery()), 0, |request, _| {
            admitted_battery = Some(request);
            Ok(())
        }),
        Some(EntryAdmissionEvent::Admitted)
    );
    let admitted_environment = admitted_environment.unwrap();
    let admitted_battery = admitted_battery.unwrap();
    assert_eq!(
        admitted_environment.owner_generation,
        admitted_battery.owner_generation
    );
    assert_eq!(admitted_environment.owner, admitted_battery.owner);
    assert_eq!(admitted_environment.fields.bits(), 3);
    assert_eq!(admitted_battery.fields.bits(), 3);
    home.poll_environment(environment_key, environment(), Instant(20))
        .unwrap();
    home.poll_battery(battery_key, battery(), Instant(20))
        .unwrap();
    home.committed(shell.active_instance(), Instant(21))
        .unwrap();
    assert_eq!(
        home.admit_environment(Instant(21), Some(environment()), 0, |_, _| panic!(
            "unchanged commit readmits"
        )),
        None
    );
    assert_eq!(
        home.admit_battery(Instant(21), Some(battery()), 0, |_, _| panic!(
            "unchanged commit readmits"
        )),
        None
    );
    let environment_ready = EnvironmentStateSnapshot {
        revision: Revision(2),
        last_sample_revision: Some(Revision(2)),
        last_sample_at: Some(Instant(22)),
        ..environment()
    };
    assert!(home
        .poll_environment(environment_key, environment_ready, Instant(22))
        .unwrap()
        .sample
        .is_some());
    assert!(home
        .poll_battery(battery_key, battery(), Instant(22))
        .is_none());
    let battery_ready = BatteryStateSnapshot {
        revision: Revision(2),
        last_sample_revision: Some(Revision(2)),
        last_sample_at: Some(Instant(23)),
        ..battery()
    };
    assert!(home
        .poll_battery(battery_key, battery_ready, Instant(23))
        .unwrap()
        .sample
        .is_some());
}

#[test]
fn retained_wake_and_rejected_sleep_recreate_both_entry_intents_with_new_lifetimes() {
    let (shell, mut home) = setup();
    for at in [20, 40] {
        let old_environment = home.environment_key().unwrap();
        let old_battery = home.battery_key().unwrap();
        home.deactivate();
        assert_eq!(
            home.admit_environment(Instant(at), Some(environment()), 0, |_, _| panic!(
                "removed Home admits"
            )),
            None
        );
        assert_eq!(
            home.admit_battery(Instant(at), Some(battery()), 0, |_, _| panic!(
                "removed Home admits"
            )),
            None
        );
        home.rebuilt(shell.active_instance(), Instant(at)).unwrap();
        assert!(home
            .poll_environment(old_environment, environment(), Instant(at))
            .is_none());
        assert!(home
            .poll_battery(old_battery, battery(), Instant(at))
            .is_none());
        assert_eq!(
            home.admit_environment(Instant(at), Some(environment()), 1, |request, now| {
                assert_eq!(
                    request.expires_at,
                    now.saturating_add(Duration::from_secs(60))
                );
                assert!(request.owner_generation > old_environment.owner_generation);
                Ok(())
            }),
            Some(EntryAdmissionEvent::Admitted)
        );
        assert_eq!(
            home.admit_battery(Instant(at), Some(battery()), 1, |request, now| {
                assert_eq!(
                    request.expires_at,
                    now.saturating_add(Duration::from_secs(60))
                );
                assert!(request.owner_generation > old_battery.owner_generation);
                Ok(())
            }),
            Some(EntryAdmissionEvent::Admitted)
        );
    }
}

#[test]
fn both_providers_reject_completion_at_expiry_but_accept_before_expiry_when_observed_late() {
    fn check<F: FieldMask, S: Copy>(fields: F, cached: StateSnapshot<S>) {
        let mut consumer = Consumer::new();
        consumer.install(cached.provider, OwnerGeneration(1), fields, Instant(0));
        let key = consumer.key().unwrap();
        consumer.admit(Instant(20), Some(cached), 0, |_, _| Ok(()));
        let expired = StateSnapshot {
            revision: Revision(2),
            last_sample_revision: Some(Revision(2)),
            last_sample_at: Some(Instant(60_000)),
            ..cached
        };
        assert_eq!(
            consumer.admit(Instant(60_000), Some(expired), 0, |_, _| panic!(
                "late completion readmits"
            )),
            Some(EntryAdmissionEvent::Expired)
        );
        consumer.install(cached.provider, OwnerGeneration(1), fields, Instant(70_000));
        consumer.admit(Instant(70_001), Some(cached), 0, |_, _| Ok(()));
        let completed = StateSnapshot {
            revision: Revision(2),
            last_sample_revision: Some(Revision(2)),
            last_sample_at: Some(Instant(70_002)),
            ..cached
        };
        assert_eq!(
            consumer.admit(Instant(130_001), Some(completed), 0, |_, _| panic!(
                "success readmits"
            )),
            None
        );
        assert!(consumer
            .poll(key, completed, Instant(130_001))
            .unwrap()
            .sample
            .is_some());
    }
    check(
        EnvironmentFields::TEMPERATURE.union(EnvironmentFields::HUMIDITY),
        environment(),
    );
    check(
        BatteryFields::VOLTAGE.union(BatteryFields::LEVEL),
        battery(),
    );
}

#[test]
fn home_replacement_preserves_reserved_fixture_slot_and_does_not_route_it_as_home() {
    let (shell, mut home) = setup();
    let mut fixture = *home
        .environment
        .subscriptions
        .get(home.environment_key().unwrap())
        .unwrap();
    fixture.key.owner = OwnerId(99);
    fixture.initial_policy = InitialPolicy::UseCache;
    fixture.max_age = Duration::from_secs(10);
    home.environment.subscriptions.upsert(fixture).unwrap();
    assert!(home.environment.subscriptions.is_full());
    assert!(home
        .poll_environment(fixture.key, environment(), Instant(1))
        .is_none());
    home.deactivate();
    assert!(home.environment_key().is_none());
    assert_eq!(
        home.environment_demand(Instant(1)).max_age_at(0),
        Some(Duration::from_secs(10))
    );
    home.rebuilt(shell.active_instance(), Instant(2)).unwrap();
    assert!(home.environment.subscriptions.is_full());
    assert!(home.environment.subscriptions.get(fixture.key).is_some());
}

#[test]
fn restart_during_entry_keeps_missing_samples_empty_and_rejects_previous_generation_for_both_providers(
) {
    fn check<F: FieldMask, S: Copy>(fields: F, cached: StateSnapshot<S>) {
        let mut consumer = Consumer::new();
        consumer.install(cached.provider, OwnerGeneration(1), fields, Instant(0));
        let key = consumer.key().unwrap();
        consumer.admit(Instant(20), Some(cached), 0, |_, _| Ok(()));
        consumer.poll(key, cached, Instant(20)).unwrap();
        let restarted = StateSnapshot {
            generation: ProviderGeneration(1),
            revision: Revision(0),
            health: Health::Unknown,
            last_sample_at: None,
            last_sample_revision: None,
            ..cached
        };
        let health = consumer.poll(key, restarted, Instant(21)).unwrap();
        assert_eq!(health.health, Some(Health::Unknown));
        assert!(health.sample.is_none());
        assert!(consumer.poll(key, cached, Instant(22)).is_none());
        let completed = StateSnapshot {
            revision: Revision(1),
            last_sample_revision: Some(Revision(1)),
            last_sample_at: Some(Instant(23)),
            ..restarted
        };
        assert!(consumer
            .poll(key, completed, Instant(23))
            .unwrap()
            .sample
            .is_some());
        assert!(consumer.poll(key, completed, Instant(24)).is_none());
        assert_eq!(
            consumer.admit(Instant(24), Some(completed), 0, |_, _| panic!(
                "new result already consumed"
            )),
            None
        );
    }
    check(
        EnvironmentFields::TEMPERATURE.union(EnvironmentFields::HUMIDITY),
        environment(),
    );
    check(
        BatteryFields::VOLTAGE.union(BatteryFields::LEVEL),
        battery(),
    );
}

#[derive(Clone, Copy)]
enum SameMillisecondOutcome {
    Success,
    Failure,
    SuccessThenFailure,
}

fn same_millisecond_entry_contract<F: FieldMask, S: Copy>(
    fields: F,
    cached: StateSnapshot<S>,
    outcome: SameMillisecondOutcome,
) {
    let mut consumer = Consumer::new();
    consumer.install(cached.provider, OwnerGeneration(1), fields, Instant(100));
    let key = consumer.key().unwrap();
    let cached = StateSnapshot {
        last_sample_at: Some(Instant(100)),
        last_sample_revision: Some(Revision(1)),
        // Avoid a health transition masking the explicit result bypass.
        health: Health::Degraded,
        ..cached
    };
    assert_eq!(
        consumer.admit(Instant(100), Some(cached), 0, |_, _| Ok(())),
        Some(EntryAdmissionEvent::Admitted)
    );
    assert!(consumer
        .poll(key, cached, Instant(100))
        .unwrap()
        .sample
        .is_some());
    assert!(consumer.poll(key, cached, Instant(100)).is_none());
    let (revision, last_sample_revision, delivered) = match outcome {
        SameMillisecondOutcome::Success => (Revision(2), Revision(2), true),
        SameMillisecondOutcome::Failure => (Revision(2), Revision(1), false),
        SameMillisecondOutcome::SuccessThenFailure => (Revision(3), Revision(2), true),
    };
    let observed = StateSnapshot {
        revision,
        last_sample_revision: Some(last_sample_revision),
        ..cached
    };
    // Admission, the cache and the new conversion all share timestamp 100.
    assert_eq!(
        consumer.admit(Instant(101), Some(observed), 0, |_, _| panic!(
            "entry readmitted"
        )),
        None
    );
    assert_eq!(
        consumer.poll(key, observed, Instant(101)).is_some(),
        delivered
    );
    assert!(consumer.poll(key, observed, Instant(101)).is_none());
    let later = StateSnapshot {
        revision: Revision(4),
        last_sample_revision: Some(Revision(4)),
        last_sample_at: Some(Instant(102)),
        ..cached
    };
    // Failure-only leaves the one-shot bypass intact. Successful cases spent it.
    assert_eq!(
        consumer.poll(key, later, Instant(102)).is_some(),
        !delivered
    );
}

fn same_millisecond_for_both(outcome: SameMillisecondOutcome) {
    same_millisecond_entry_contract(
        EnvironmentFields::TEMPERATURE.union(EnvironmentFields::HUMIDITY),
        environment(),
        outcome,
    );
    same_millisecond_entry_contract(
        BatteryFields::VOLTAGE.union(BatteryFields::LEVEL),
        battery(),
        outcome,
    );
}

#[test]
fn successful_entry_conversion_in_same_millisecond_bypasses_interval_once() {
    same_millisecond_for_both(SameMillisecondOutcome::Success);
}

#[test]
fn failure_only_revision_in_same_millisecond_does_not_consume_entry_bypass() {
    same_millisecond_for_both(SameMillisecondOutcome::Failure);
}

#[test]
fn success_then_failure_before_ui_poll_still_delivers_the_requested_success() {
    same_millisecond_for_both(SameMillisecondOutcome::SuccessThenFailure);
}

#[test]
fn restarted_provider_success_identity_includes_provider_generation() {
    let (_, mut home) = setup();
    let key = home.battery_key().unwrap();
    let cached = battery();
    home.admit_battery(Instant(1), Some(cached), 0, |_, _| Ok(()));
    home.poll_battery(key, cached, Instant(1)).unwrap();
    let restarted = BatteryStateSnapshot {
        provider: BATTERY_PROVIDER_ID,
        last_attempt_at: Some(Instant(1)),
        generation: ProviderGeneration(1),
        ..cached
    };
    assert!(home
        .poll_battery(key, restarted, Instant(1))
        .unwrap()
        .sample
        .is_some());
    // Same successful revision and timestamp in the new provider generation
    // satisfied the entry; closing admission later cannot resurrect it.
    assert_eq!(
        home.admit_battery(Instant(2), Some(restarted), 1, |_, _| panic!(
            "completed entry readmitted"
        )),
        None
    );
}

#[test]
fn both_provider_projections_preserve_failed_attempt_and_success_identity() {
    fn check<S: Copy + Default, F: FieldMask>(provider: ProviderId, fields: F) {
        let mut state = observation::state::ObservationState::<S, FIELDS>::new(
            provider,
            ProviderGeneration(0),
            S::default(),
        );
        state.apply_failure(Instant(10)).unwrap();
        let first = StateSnapshot::from_observation(&state, None);
        assert_eq!(first.provider, provider);
        assert_eq!(first.last_attempt_at, Some(Instant(10)));
        assert_eq!(first.sample_identity(), None);
        state.apply_success(fields, Instant(20), |_| {}).unwrap();
        let success = state.revision;
        state.apply_failure(Instant(30)).unwrap();
        let retained = StateSnapshot::from_observation(&state, Some(success));
        assert_eq!(retained.last_attempt_at, Some(Instant(30)));
        assert_eq!(retained.last_sample_at, Some(Instant(20)));
        assert_eq!(
            retained.sample_identity(),
            Some((ProviderGeneration(0), Revision(2)))
        );
        state.restart(ProviderGeneration(1));
        let restarted = StateSnapshot::from_observation(&state, None);
        assert_eq!(restarted.generation, ProviderGeneration(1));
        assert_eq!(restarted.last_attempt_at, None);
        assert_eq!(restarted.sample_identity(), None);
    }
    check::<EnvironmentSnapshot, _>(
        ENVIRONMENT_PROVIDER_ID,
        EnvironmentFields::TEMPERATURE.union(EnvironmentFields::HUMIDITY),
    );
    check::<BatterySnapshot, _>(
        BATTERY_PROVIDER_ID,
        BatteryFields::VOLTAGE.union(BatteryFields::LEVEL),
    );
}

#[test]
fn wrong_provider_identity_cannot_change_home_delivery() {
    let (_, mut home) = setup();
    let key = home.environment_key().unwrap();
    let wrong = EnvironmentStateSnapshot {
        provider: BATTERY_PROVIDER_ID,
        ..environment()
    };
    assert!(home.poll_environment(key, wrong, Instant(1)).is_none());
    assert!(home
        .poll_environment(key, environment(), Instant(1))
        .unwrap()
        .sample
        .is_some());
}

fn fixture_request(id: u64, expires: u64) -> observation::fixture::FixtureRequest {
    observation::fixture::FixtureRequest {
        id,
        expires_at: Instant(expires),
    }
}

#[test]
fn fixtures_use_distinct_owner_and_preserve_both_live_home_demands() {
    let (_, mut home) = setup();
    let env_key = home.environment_key();
    let battery_key = home.battery_key();
    let environment_demand = home.environment_demand(Instant(10));
    let battery_demand = home.battery_demand(Instant(10));
    let env = environment();
    let bat = battery();
    assert!(home
        .environment_fixture()
        .begin(
            fixture_request(1, 100),
            Instant(10),
            Some(env),
            0,
            |request, at| {
                assert_ne!(request.owner, env_key.unwrap().owner);
                assert_eq!(request.fields.bits(), 3);
                assert_eq!(request.admitted_at, at);
                Ok(())
            }
        )
        .is_none());
    assert!(home
        .battery_fixture()
        .begin(
            fixture_request(2, 100),
            Instant(10),
            Some(bat),
            0,
            |request, _| {
                assert_ne!(request.owner, battery_key.unwrap().owner);
                assert_eq!(request.fields.bits(), 3);
                Ok(())
            }
        )
        .is_none());
    assert_eq!(home.environment_key(), env_key);
    assert_eq!(home.battery_key(), battery_key);
    assert_same_demand(home.environment_demand(Instant(10)), environment_demand);
    assert_same_demand(home.battery_demand(Instant(10)), battery_demand);
}

#[test]
fn both_fixtures_complete_without_home_and_do_not_fabricate_periodic_demand() {
    use observation::fixture::FixtureStatus;
    let mut home = HomeObservations::new();
    let mut env = environment();
    let mut bat = battery();
    let env_demand = home.environment_demand(Instant(10));
    let bat_demand = home.battery_demand(Instant(10));
    assert!(home
        .environment_fixture()
        .begin(
            fixture_request(1, 100),
            Instant(10),
            Some(env),
            0,
            |_, _| Ok(())
        )
        .is_none());
    assert!(home
        .battery_fixture()
        .begin(
            fixture_request(2, 100),
            Instant(10),
            Some(bat),
            0,
            |_, _| Ok(())
        )
        .is_none());
    assert!(home
        .environment_fixture()
        .poll(Instant(11), Some(env), 0)
        .is_none());
    assert!(home
        .battery_fixture()
        .poll(Instant(11), Some(bat), 0)
        .is_none());
    env.revision = Revision(2);
    env.last_sample_revision = Some(Revision(2));
    env.last_attempt_at = Some(Instant(12));
    env.last_sample_at = Some(Instant(12));
    bat.revision = Revision(2);
    bat.last_sample_revision = Some(Revision(2));
    bat.last_attempt_at = Some(Instant(12));
    bat.last_sample_at = Some(Instant(12));
    assert_eq!(
        home.environment_fixture()
            .poll(Instant(12), Some(env), 0)
            .unwrap()
            .status,
        FixtureStatus::Sampled
    );
    assert_eq!(
        home.battery_fixture()
            .poll(Instant(12), Some(bat), 0)
            .unwrap()
            .status,
        FixtureStatus::Sampled
    );
    assert!(home
        .environment_fixture()
        .poll(Instant(13), Some(env), 0)
        .is_none());
    assert!(home
        .battery_fixture()
        .poll(Instant(13), Some(bat), 0)
        .is_none());
    assert!(home.environment_key().is_none());
    assert!(home.battery_key().is_none());
    assert_same_demand(home.environment_demand(Instant(13)), env_demand);
    assert_same_demand(home.battery_demand(Instant(13)), bat_demand);
}

#[test]
fn home_rebuild_and_withdrawal_do_not_cancel_admitted_fixture() {
    use observation::fixture::FixtureStatus;
    let (shell, mut home) = setup();
    let mut env = environment();
    assert!(home
        .environment_fixture()
        .begin(
            fixture_request(1, 100),
            Instant(10),
            Some(env),
            7,
            |_, _| Ok(())
        )
        .is_none());
    home.deactivate();
    home.rebuilt(shell.active_instance(), Instant(11)).unwrap();
    home.deactivate();
    env.revision = Revision(2);
    env.last_sample_revision = Some(Revision(2));
    env.last_attempt_at = Some(Instant(12));
    env.last_sample_at = Some(Instant(12));
    assert_eq!(
        home.environment_fixture()
            .poll(Instant(12), Some(env), 7)
            .unwrap()
            .status,
        FixtureStatus::Sampled
    );
}

#[test]
fn medinote_fixture_preserves_terminal_admission_and_close_restart_fences() {
    use observation::fixture::FixtureStatus;
    for (error, status) in [
        (RequestAdmissionError::Full, FixtureStatus::Full),
        (RequestAdmissionError::Closed, FixtureStatus::Closed),
    ] {
        let mut home = HomeObservations::new();
        let result = home
            .battery_fixture()
            .begin(
                fixture_request(1, 100),
                Instant(10),
                Some(battery()),
                0,
                |_, _| Err(error),
            )
            .unwrap();
        assert_eq!(result.status, status);
        assert!(home
            .battery_fixture()
            .poll(Instant(20), Some(battery()), 0)
            .is_none());
    }
    for (close, generation, now, expected) in [
        (1, 0, 12, FixtureStatus::Cancelled),
        (0, 1, 12, FixtureStatus::Restarted),
        (0, 0, 100, FixtureStatus::Expired),
    ] {
        let mut home = HomeObservations::new();
        let mut env = environment();
        assert!(home
            .environment_fixture()
            .begin(
                fixture_request(1, 100),
                Instant(10),
                Some(env),
                0,
                |_, _| Ok(())
            )
            .is_none());
        env.generation = ProviderGeneration(generation);
        assert_eq!(
            home.environment_fixture()
                .poll(Instant(now), Some(env), close)
                .unwrap()
                .status,
            expected
        );
        assert!(home
            .environment_fixture()
            .poll(Instant(101), Some(env), close)
            .is_none());
    }
}

fn console_line(
    input: &mut fixture::ConsoleInput,
    bytes: &[u8],
) -> Option<fixture::ConsoleCommand> {
    let mut result = None;
    for byte in bytes {
        if let Some(command) = input.push(*byte) {
            assert!(result.is_none());
            result = Some(command);
        }
    }
    result
}

#[test]
fn fixture_console_retains_partial_frames_and_consumes_ids_across_providers() {
    use fixture::{ConsoleCommand, ConsoleInput, FixtureProvider};
    use observation::fixture::FixtureStatus;
    let mut input = ConsoleInput::default();
    assert!(console_line(&mut input, b"OBSFIX SH").is_none());
    let command = fixture::parse_command(b"OBSFIX SHTC3 42 300000").unwrap();
    assert_eq!(
        console_line(&mut input, b"TC3 42 300000\r\n"),
        Some(ConsoleCommand::Fixture(command))
    );
    assert_eq!(command.at(Instant(5)).expires_at, Instant(300005));
    let stale = fixture::parse_command(b"OBSFIX ADC 42 50").unwrap();
    assert_eq!(
        console_line(&mut input, b"OBSFIX ADC 42 50\n"),
        Some(ConsoleCommand::Rejected(stale, FixtureStatus::StaleId))
    );
    let ConsoleCommand::Fixture(next) = console_line(&mut input, b"OBSFIX ADC 43 50\n").unwrap()
    else {
        panic!("fresh command rejected")
    };
    assert_eq!(next.provider, FixtureProvider::Adc);
    assert_eq!(
        console_line(&mut input, b"PING\n"),
        Some(ConsoleCommand::Ping)
    );
}

#[test]
fn console_recognizes_settings_open_and_close_commands() {
    use fixture::{ConsoleCommand, ConsoleInput};
    let mut input = ConsoleInput::default();
    assert_eq!(
        console_line(&mut input, b"UISETTINGS\n"),
        Some(ConsoleCommand::UiSettings)
    );
    assert_eq!(
        console_line(&mut input, b"UICLOSE\n"),
        Some(ConsoleCommand::UiClose)
    );
    // A trailing \r\n frames the same as a bare \n, matching PING's own
    // line framing (both are stripped as line delimiters in `push`).
    assert_eq!(
        console_line(&mut input, b"UISETTINGS\r\n"),
        Some(ConsoleCommand::UiSettings)
    );
}

#[test]
fn fixture_console_discards_oversized_line_until_delimiter_and_recovers() {
    use fixture::{ConsoleCommand, ConsoleInput, COMMAND_CAPACITY};
    let mut input = ConsoleInput::default();
    for _ in 0..COMMAND_CAPACITY + 1 {
        assert!(input.push(b' ').is_none());
    }
    assert_eq!(
        console_line(&mut input, b"OBSFIX ADC 1 100\n"),
        Some(ConsoleCommand::Invalid)
    );
    assert!(matches!(
        console_line(&mut input, b"OBSFIX ADC 1 100\n"),
        Some(ConsoleCommand::Fixture(_))
    ));
    assert_eq!(
        console_line(&mut input, b"\xff\n"),
        Some(ConsoleCommand::Invalid)
    );
    assert_eq!(
        console_line(&mut input, b"PING\r\n"),
        Some(ConsoleCommand::Ping)
    );
}

#[test]
fn fixture_console_custom_handler_receives_long_line_once_and_overflow_recovers() {
    use core::cell::RefCell;
    use fixture::{ConsoleCommand, ConsoleInput, COMMAND_CAPACITY};
    use heapless::Vec;

    let mut input = ConsoleInput::default();
    let mut line = [0u8; 250];
    for (index, byte) in line.iter_mut().enumerate() {
        *byte = b'a' + (index % 26) as u8;
    }
    let seen = RefCell::new(Vec::<Vec<u8, 250>, 1>::new());
    for &byte in line.iter() {
        assert!(input
            .push_with_handler(byte, |completed| {
                let mut copy = Vec::new();
                copy.extend_from_slice(completed).unwrap();
                seen.borrow_mut().push(copy).unwrap();
                true
            })
            .is_none());
    }
    assert!(input
        .push_with_handler(b'\n', |completed| {
            let mut copy = Vec::new();
            copy.extend_from_slice(completed).unwrap();
            seen.borrow_mut().push(copy).unwrap();
            true
        })
        .is_none());
    assert_eq!(seen.borrow().len(), 1);
    assert_eq!(seen.borrow()[0].as_slice(), line.as_slice());

    let oversized = [b'x'; COMMAND_CAPACITY + 1];
    let calls = RefCell::new(0usize);
    let mut overflow_result = None;
    for byte in oversized {
        if let Some(command) = input.push_with_handler(byte, |_| {
            *calls.borrow_mut() += 1;
            true
        }) {
            overflow_result = Some(command);
        }
    }
    if let Some(command) = input.push_with_handler(b'\n', |_| {
        *calls.borrow_mut() += 1;
        true
    }) {
        overflow_result = Some(command);
    }
    assert_eq!(overflow_result, Some(ConsoleCommand::Invalid));
    assert_eq!(*calls.borrow(), 0);
    assert_eq!(
        console_line(&mut input, b"PING\n"),
        Some(ConsoleCommand::Ping)
    );
}

#[cfg(feature = "network-controls")]
#[test]
fn fixture_console_accepts_full_network_config_line_and_discards_overflow() {
    use core::cell::RefCell;
    use fixture::{ConsoleCommand, ConsoleInput, COMMAND_CAPACITY};
    use heapless::Vec;

    let line = br#"NETCFG SET {"ssid":"Meditamer network","password":"a secure password","connect_timeout_ms":30000,"dhcp_timeout_ms":20000,"pinned_dhcp_timeout_ms":45000,"listener_timeout_ms":25000,"scan_active_min_ms":600,"scan_active_max_ms":1500,"scan_passive_ms":1500,"retry_same_max":2,"rotate_candidate_max":2,"rotate_auth_max":5,"full_scan_reset_max":1,"driver_restart_max":1,"cooldown_ms":1200,"driver_restart_backoff_ms":2500}"#;
    assert!(line.len() > 320);
    let mut input = ConsoleInput::default();
    let seen = RefCell::new(None::<Vec<u8, 1024>>);
    for &byte in line {
        assert!(input
            .push_with_handler(byte, |completed| {
                assert!(completed.starts_with(b"NETCFG SET "));
                let mut copy = Vec::new();
                copy.extend_from_slice(completed).unwrap();
                seen.replace(Some(copy));
                true
            })
            .is_none());
    }
    assert!(input
        .push_with_handler(b'\n', |completed| {
            assert!(completed.starts_with(b"NETCFG SET "));
            let mut copy = Vec::new();
            copy.extend_from_slice(completed).unwrap();
            seen.replace(Some(copy));
            true
        })
        .is_none());
    assert_eq!(
        seen.borrow().as_ref().map(Vec::as_slice),
        Some(line.as_slice())
    );

    let oversized = [b'x'; COMMAND_CAPACITY + 1];
    let handler_calls = RefCell::new(0usize);
    for byte in oversized {
        assert!(input
            .push_with_handler(byte, |_| {
                *handler_calls.borrow_mut() += 1;
                true
            })
            .is_none());
    }
    assert_eq!(
        input.push_with_handler(b'\n', |_| {
            *handler_calls.borrow_mut() += 1;
            true
        }),
        Some(ConsoleCommand::Invalid)
    );
    assert_eq!(*handler_calls.borrow(), 0);
    assert_eq!(
        console_line(&mut input, b"PING\n"),
        Some(ConsoleCommand::Ping)
    );
}

#[test]
fn fixture_console_validates_provider_decimal_bounds_and_never_wraps_ids() {
    use fixture::{ConsoleCommand, ConsoleInput};
    for line in [
        b"OBSFIX BATTERY 1 10".as_slice(),
        b"OBSFIX BME688 1 10",
        b"OBSFIX ADC 0 10",
        b"OBSFIX ADC +1 10",
        b"OBSFIX ADC 1 0",
        b"OBSFIX ADC 1 300001",
        b"OBSFIX SHTC3 1 10 extra",
        b"OBSFIX ADC 18446744073709551616 10",
    ] {
        assert!(fixture::parse_command(line).is_none());
    }
    let mut input = ConsoleInput::default();
    assert!(matches!(
        console_line(&mut input, b"OBSFIX SHTC3 18446744073709551615 1\n"),
        Some(ConsoleCommand::Fixture(_))
    ));
    assert!(matches!(
        console_line(&mut input, b"OBSFIX ADC 1 100\n"),
        Some(ConsoleCommand::Rejected(
            _,
            observation::fixture::FixtureStatus::StaleId
        ))
    ));
}

#[test]
fn fixture_wire_preserves_medinote_units_and_missing_sample_values() {
    extern crate std;
    use observation::fixture::{FixtureResult, FixtureStatus};
    let env = FixtureResult {
        id: 1,
        owner_generation: OwnerGeneration(1),
        admitted_at: Some(Instant(0)),
        expires_at: Instant(100),
        status: FixtureStatus::Sampled,
        state: Some(environment()),
        baseline: None,
    };
    let bat = FixtureResult {
        id: 2,
        owner_generation: env.owner_generation,
        admitted_at: env.admitted_at,
        expires_at: env.expires_at,
        status: env.status,
        state: Some(battery()),
        baseline: None,
    };
    let env_line = std::format!("{env}");
    let bat_line = std::format!("{bat}");
    assert!(env_line.contains("provider=1 fields=3"));
    assert!(env_line.contains("temperature_millicelsius=23500 humidity_millipercent=42000"));
    assert!(!env_line.contains("temperature_centidegrees="));
    assert!(bat_line.contains("provider=2 fields=3"));
    assert!(bat_line.contains("percent="));
    assert!(bat_line.contains("millivolts="));
    let missing = FixtureResult {
        state: Some(BatteryStateSnapshot::default()),
        status: FixtureStatus::Expired,
        ..bat
    };
    assert!(std::format!("{missing}").contains("percent=none millivolts=none"));
}

fn assert_same_demand<F: FieldMask>(actual: Demand<F, FIELDS>, expected: Demand<F, FIELDS>) {
    assert_eq!(actual.fields.bits(), expected.fields.bits());
    for index in 0..FIELDS {
        assert_eq!(actual.max_age_at(index), expected.max_age_at(index));
    }
}

#[test]
fn periodic_cancel_uses_original_id_without_rewinding_console_watermark() {
    use fixture::{ConsoleCommand, ConsoleInput};
    use observation::fixture::FixtureStatus;
    let mut input = ConsoleInput::default();
    for line in [
        b"OBSPER SHTC3 42 60000 150000\n".as_slice(),
        b"OBSFIX ADC 43 100\n",
        b"OBSPER SHTC3 42 CANCEL\n",
    ] {
        assert!(matches!(
            console_line(&mut input, line),
            Some(ConsoleCommand::Fixture(_))
        ));
    }
    assert!(matches!(
        console_line(&mut input, b"OBSFIX ADC 43 100\n"),
        Some(ConsoleCommand::Rejected(_, FixtureStatus::StaleId))
    ));
}

#[test]
fn sleep_console_validates_bounds_and_shares_the_fixture_watermark() {
    use super::sleep_fixture::{parse, SleepMode};
    use fixture::{ConsoleCommand, ConsoleInput};
    let mut input = ConsoleInput::default();
    let request = parse(b"OBSSLEEP 42 30000 DEEP").unwrap();
    assert_eq!(request.mode, SleepMode::Deep);
    assert_eq!(
        console_line(&mut input, b"OBSSLEEP 42 30000 DEEP\n"),
        Some(ConsoleCommand::Sleep(request))
    );
    assert_eq!(
        console_line(&mut input, b"OBSSLEEP 42 30000 REJECT_ADC\n"),
        Some(ConsoleCommand::SleepStale(42))
    );
    assert!(matches!(
        console_line(&mut input, b"OBSFIX ADC 42 30000\n"),
        Some(ConsoleCommand::Rejected(
            _,
            observation::fixture::FixtureStatus::StaleId
        ))
    ));
    for line in [
        b"OBSSLEEP 0 30000 DEEP".as_slice(),
        b"OBSSLEEP 43 0 DEEP",
        b"OBSSLEEP 43 300001 DEEP",
        b"OBSSLEEP 43 30 UNKNOWN",
        b"OBSSLEEP 43 30 DEEP EXTRA",
    ] {
        assert!(parse(line).is_none());
    }
    for command in [
        b"OBSSLEEP 43 30 REJECT_ENV".as_slice(),
        b"OBSSLEEP 43 30 REJECT_ADC",
    ] {
        assert!(parse(command).is_some());
    }
}
