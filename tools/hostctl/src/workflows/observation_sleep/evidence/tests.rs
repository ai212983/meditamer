use super::*;

fn capture(mode: Mode) -> Vec<String> {
    let mut lines = vec![format!(
        "OBSSLEEP BEGIN id=42 mode={} at_ms=100 expires_at_ms=30000",
        mode.wire()
    )];
    for name in ["ENVIRONMENT", "BATTERY"] {
        lines.push(format!(
            "OBSSLEEP BASE id=42 provider={name} generation=0 revision=3 sampled_at_ms=Some(90)"
        ));
        let failed = (name == "ENVIRONMENT" && mode == Mode::RejectEnvironment)
            || (name == "BATTERY" && mode == Mode::RejectBattery);
        if failed {
            lines.push(format!("OBSERVATION_CLEANUP_INJECTED provider={name} id=5"));
        }
        lines.push(format!(
            "OBSERVATION_SUSPEND_ACK provider={name} id=5 outcome={} accepted=true",
            if failed { "CleanupFailed" } else { "Quiesced" }
        ));
    }
    lines.push(format!(
        "OBSSLEEP CONTROL id=42 environment_id=5 environment_ack={} battery_id=5 battery_ack={}",
        if mode == Mode::RejectEnvironment {
            "CleanupFailed"
        } else {
            "Quiesced"
        },
        if mode == Mode::RejectBattery {
            "CleanupFailed"
        } else {
            "Quiesced"
        }
    ));
    if mode == Mode::Deep {
        lines.extend(
            [
                "OBSSLEEP ENTER id=42 at_ms=200",
                "DEEP_SLEEP_ENTER wake=timer-only duration_s=15 destination=home",
                "RESET reason=Some(CoreDeepSleep) wake=Timer",
                "RUNTIME_READY app_state=ready display=ready",
            ]
            .map(str::to_owned),
        );
    } else {
        lines.push(
            "OBSSLEEP END id=42 status=Rejected at_ms=200 environment_id=6 battery_id=6".into(),
        );
    }
    lines.push("HOME_SAMPLE generation=0 revision=4 last_sample_at_ms=Some(300) last_sample_revision=Some(4)".into());
    lines.push("HOME_BATTERY generation=0 revision=4 health=Ok last_sample_at_ms=Some(301) last_sample_revision=Some(4)".into());
    lines
}

#[test]
fn accepts_deep_and_each_rejection_only_with_both_fresh_providers() {
    for mode in [Mode::Deep, Mode::RejectEnvironment, Mode::RejectBattery] {
        let mut lines = capture(mode);
        assert!(validate(&lines, 42, mode).unwrap().is_some());
        lines.pop();
        assert!(validate(&lines, 42, mode).unwrap().is_none());
    }
}

#[test]
fn refuses_stale_ack_wrong_wake_unrequested_injection_and_sleep_after_rejection() {
    for (mode, from, to) in [
        (Mode::Deep, "accepted=true", "accepted=false"),
        (Mode::Deep, "wake=Timer", "wake=Undefined"),
        (Mode::Deep, "DEEP_SLEEP_ENTER", "MISSING_ENTRY"),
        (Mode::RejectBattery, "battery_id=6", "battery_id=5"),
        (
            Mode::RejectEnvironment,
            "OBSERVATION_CLEANUP_INJECTED",
            "IGNORED",
        ),
        (Mode::Deep, "id=42 at_ms=200", "id=43 at_ms=200"),
    ] {
        let lines = capture(mode)
            .into_iter()
            .map(|l| l.replace(from, to))
            .collect::<Vec<_>>();
        assert!(validate(&lines, 42, mode).is_err(), "{from}");
    }
    let mut lines = capture(Mode::RejectBattery);
    lines.push("RESET reason=Some(CoreDeepSleep) wake=Timer".into());
    assert!(validate(&lines, 42, Mode::RejectBattery).is_err());
}

#[test]
fn cached_sample_or_new_generation_cannot_prove_rejection_recovery() {
    let mut lines = capture(Mode::RejectEnvironment);
    let sample = lines
        .iter_mut()
        .find(|l| l.starts_with("HOME_SAMPLE"))
        .unwrap();
    *sample = sample.replace("Some(300)", "Some(90)");
    assert!(validate(&lines, 42, Mode::RejectEnvironment)
        .unwrap()
        .is_none());
    let lines = capture(Mode::RejectEnvironment)
        .into_iter()
        .map(|l| {
            if l.starts_with("HOME_SAMPLE") {
                l.replace("generation=0", "generation=1")
            } else {
                l
            }
        })
        .collect::<Vec<_>>();
    assert!(validate(&lines, 42, Mode::RejectEnvironment).is_err());
}

fn periodic_capture(mode: Mode) -> Vec<String> {
    let mut lines = capture(mode);
    for (provider, id, name) in [(1, 40, "ENVIRONMENT"), (2, 41, "BATTERY")] {
        lines.insert(0, format!("OBSPER APPLIED id={id} provider={provider} status=Applied applied_at_ms=50 at_ms=50 expires_at_ms=240000 interval_ms=90000 live_fields=3 live_age0_ms=60000 live_age1_ms=60000"));
        let ack = lines
            .iter()
            .position(|l| l.starts_with(&format!("OBSERVATION_SUSPEND_ACK provider={name}")))
            .unwrap();
        lines.insert(ack, format!("OBSPER RESULT id={id} provider={provider} status=Closed applied_at_ms=50 at_ms=150 expires_at_ms=240000 interval_ms=90000 live_fields=0 live_age0_ms=none live_age1_ms=none"));
        lines.push(format!("OBSSLEEP DEMAND id=42 provider={provider} pending=false live_fields=3 live_age0_ms=60000 live_age1_ms=60000"));
    }
    lines.push("HOME_SAMPLE generation=0 revision=5 last_sample_at_ms=Some(60300) last_sample_revision=Some(5)".into());
    lines.push("HOME_BATTERY generation=0 revision=5 health=Ok last_sample_at_ms=Some(60301) last_sample_revision=Some(5)".into());
    lines
}

#[test]
fn periodic_rejection_requires_closure_current_live_demand_and_later_acquisition() {
    use super::super::periodic::Setup;
    for mode in [Mode::RejectEnvironment, Mode::RejectBattery] {
        let setup = Setup::new(90_000, 240_000, 42, mode).unwrap();
        let mut lines = periodic_capture(mode);
        let recovery = validate(&lines, 42, mode).unwrap().unwrap();
        assert!(setup.validate(&lines, 42, &recovery).unwrap().is_some());
        lines.pop();
        assert!(
            setup.validate(&lines, 42, &recovery).unwrap().is_none(),
            "entry refresh alone is not a periodic pass"
        );
    }
}

#[test]
fn periodic_rejection_refuses_wrong_closure_leaked_override_and_cached_recovery() {
    use super::super::periodic::Setup;
    let mode = Mode::RejectBattery;
    let setup = Setup::new(90_000, 240_000, 42, mode).unwrap();
    for (from, to) in [
        ("status=Closed", "status=Restored"),
        ("RESULT id=40", "RESULT id=39"),
        ("pending=false", "pending=true"),
        ("live_age0_ms=60000", "live_age0_ms=90000"),
        ("live_fields=0", "live_fields=3"),
        ("APPLIED id=40", "APPLIED id=39"),
        ("Some(60300)", "Some(90300)"),
        ("expires_at_ms=240000", "expires_at_ms=60000"),
    ] {
        let lines: Vec<_> = periodic_capture(mode)
            .into_iter()
            .map(|line| line.replace(from, to))
            .collect();
        let recovery = validate(&lines, 42, mode).unwrap().unwrap();
        assert!(setup.validate(&lines, 42, &recovery).is_err(), "{from}");
    }
    let mut lines = periodic_capture(mode);
    lines.push("OBSPER SAMPLE id=40 provider=1".into());
    let recovery = validate(&lines, 42, mode).unwrap().unwrap();
    assert!(setup.validate(&lines, 42, &recovery).is_err());
    let lines: Vec<_> = periodic_capture(mode)
        .into_iter()
        .map(|line| line.replace("Some(60300)", "Some(300)"))
        .collect();
    assert!(setup.validate(&lines, 42, &recovery).unwrap().is_none());
}

#[test]
fn periodic_setup_refuses_deep_sleep_bad_ids_and_unbounded_requests() {
    use super::super::periodic::Setup;
    for (period, validity, id, mode) in [
        (90_000, 240_000, 42, Mode::Deep),
        (60_000, 240_000, 42, Mode::RejectBattery),
        (300_001, 600_000, 42, Mode::RejectBattery),
        (90_000, 90_000, 42, Mode::RejectBattery),
        (90_000, 900_001, 42, Mode::RejectBattery),
        (90_000, 240_000, 2, Mode::RejectBattery),
    ] {
        assert!(Setup::new(period, validity, id, mode).is_err());
    }
    let setup = Setup::new(90_000, 240_000, 42, Mode::RejectBattery).unwrap();
    assert_eq!(
        setup.command(42, 1).unwrap(),
        "OBSPER SHTC3 40 90000 240000"
    );
    assert_eq!(setup.command(42, 2).unwrap(), "OBSPER ADC 41 90000 240000");
    assert!(setup.command(42, 3).is_err());
    assert!(!setup.applied(&[], 42, 1).unwrap());
    assert!(setup
        .applied(&["OBSPER RESULT id=40 status=Busy".into()], 42, 1)
        .is_err());
}

#[test]
fn hal12_deep_wake_requires_timer_as_the_only_cause() {
    for (reset, accepted) in [
        (
            "RESET reason=Some(CoreDeepSleep) wake=WakeupReason(EnumSet(Timer))",
            true,
        ),
        (
            "RESET reason=Some(CoreDeepSleep) wake=WakeupReason(EnumSet())",
            false,
        ),
        (
            "RESET reason=Some(CoreDeepSleep) wake=WakeupReason(EnumSet(Gpio))",
            false,
        ),
        (
            "RESET reason=Some(CoreDeepSleep) wake=WakeupReason(EnumSet(Timer | Gpio))",
            false,
        ),
        (
            "RESET reason=Some(CoreSoftware) wake=WakeupReason(EnumSet(Timer))",
            false,
        ),
    ] {
        let lines = capture(Mode::Deep)
            .into_iter()
            .map(|line| {
                if line.starts_with("RESET ") {
                    reset.to_owned()
                } else {
                    line
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            validate(&lines, 42, Mode::Deep).is_ok(),
            accepted,
            "{reset}"
        );
    }
}
