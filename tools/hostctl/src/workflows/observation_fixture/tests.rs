use std::{
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
};

use serialport::{SerialPort, TTYPort};
use tempfile::tempdir;

use super::*;

fn sampled(id: u64) -> String {
    format!(
        "OBSFIX RESULT id={id} provider=2 status=Sampled fields=1 owner_generation=1 \
         admitted_at_ms=100 expires_at_ms=200 generation=0 revision=5 health=Ok \
         last_attempt_at_ms=150 last_sample_at_ms=150 last_sample_revision=5 \
         baseline_generation=0 baseline_sample_revision=4 percent=73"
    )
}

#[test]
fn parses_and_verifies_success_and_later_degraded_state() -> Result<()> {
    for line in [
        sampled(42),
        sampled(42)
            .replace("revision=5 health=Ok", "revision=6 health=Degraded")
            .replace("last_attempt_at_ms=150", "last_attempt_at_ms=160"),
        sampled(42)
            .replace("baseline_generation=0", "baseline_generation=none")
            .replace(
                "baseline_sample_revision=4",
                "baseline_sample_revision=none",
            ),
    ] {
        verify_sample(
            &parse_result(&line, FixtureProvider::Battery)?,
            42,
            300,
            FixtureProvider::Battery,
        )?;
    }
    Ok(())
}

#[test]
fn rejects_invalid_success_metadata() {
    for (old, new) in [
        ("provider=2", "provider=1"),
        ("fields=1", "fields=2"),
        ("owner_generation=1", "owner_generation=0"),
        ("admitted_at_ms=100", "admitted_at_ms=200"),
        ("expires_at_ms=200", "expires_at_ms=401"),
        ("last_sample_at_ms=150", "last_sample_at_ms=99"),
        ("last_sample_at_ms=150", "last_sample_at_ms=200"),
        ("last_sample_at_ms=150", "last_sample_at_ms=none"),
        ("last_attempt_at_ms=150", "last_attempt_at_ms=149"),
        ("last_sample_revision=5", "last_sample_revision=none"),
        ("last_sample_revision=5", "last_sample_revision=0"),
        ("last_sample_revision=5", "last_sample_revision=6"),
        ("baseline_sample_revision=4", "baseline_sample_revision=5"),
        ("baseline_sample_revision=4", "baseline_sample_revision=6"),
        ("generation=0 revision=5", "generation=1 revision=5"),
        ("baseline_generation=0", "baseline_generation=1"),
        (
            "baseline_sample_revision=4",
            "baseline_sample_revision=none",
        ),
        ("health=Ok", "health=Unknown"),
        ("health=Ok", "health=Failed"),
        ("percent=73", "percent=101"),
        ("percent=73", "percent=none"),
    ] {
        let line = format!(
            "{} temperature_centidegrees=2400 humidity_millipercent=45000",
            sampled(42).replace(old, new)
        );
        let result = parse_result(&line, FixtureProvider::Battery).expect("syntactically valid");
        assert!(
            verify_sample(&result, 42, 300, FixtureProvider::Battery).is_err(),
            "accepted {new}"
        );
    }
}

#[test]
fn malformed_or_duplicate_metadata_is_not_evidence() {
    for line in [
        sampled(42).replace("percent=73", "percent=nope"),
        sampled(42).replace(" percent=73", ""),
        format!("{} percent=74", sampled(42)),
        format!("{} id=43", sampled(42)),
        sampled(0),
        sampled(42).replace("status=Sampled", "status=Success"),
        "OBSFIX QUEUED id=42 provider=2".to_owned(),
    ] {
        assert!(
            parse_result(&line, FixtureProvider::Battery).is_err(),
            "accepted {line}"
        );
    }
}

#[test]
fn short_rejections_need_no_success_metadata() -> Result<()> {
    for status in [
        "Expired",
        "Cancelled",
        "Restarted",
        "Busy",
        "Invalid",
        "StaleId",
        "GenerationExhausted",
        "Full",
        "Closed",
        "Unavailable",
    ] {
        let result = parse_result(
            &format!("OBSFIX RESULT id=42 provider=2 status={status}"),
            FixtureProvider::Battery,
        )?;
        let error = verify_sample(&result, 42, 300, FixtureProvider::Battery).unwrap_err();
        assert!(error.to_string().contains(status));
    }
    Ok(())
}

#[test]
fn option_boundaries_preserve_full_u64_ids_and_bounded_validity() -> Result<()> {
    validate_options(1, 1, 1, 0, false)?;
    validate_options(u64::MAX, 300_000, 300_000, 360_000, false)?;
    assert!(validate_options(0, 1, 1, 0, false).is_err());
    assert!(validate_options(1, 0, 1, 0, false).is_err());
    assert!(validate_options(1, 300_001, 1, 0, false).is_err());
    assert!(validate_options(1, 1, 0, 0, false).is_err());
    verify_sample(
        &parse_result(&sampled(u64::MAX), FixtureProvider::Battery)?,
        u64::MAX,
        300,
        FixtureProvider::Battery,
    )?;
    assert!(generated_request_id()? > 0);
    Ok(())
}

#[test]
fn cli_accepts_u64_max_and_rejects_overflowing_request_id() {
    use clap::Parser;

    let cli = crate::Cli::try_parse_from([
        "hostctl",
        "test",
        "observation-fixture",
        "--request-id",
        "18446744073709551615",
    ])
    .expect("maximum ID parses");
    let crate::Commands::Test(args) = cli.command else {
        panic!("wrong command");
    };
    let crate::TestSubcommand::ObservationFixture(args) = args.test else {
        panic!("wrong test command");
    };
    assert_eq!(args.request_id, Some(u64::MAX));
    assert!(crate::Cli::try_parse_from([
        "hostctl",
        "test",
        "observation-fixture",
        "--request-id",
        "18446744073709551616",
    ])
    .is_err());
}

#[test]
fn report_path_never_replaces_capture_for_any_extension() {
    for name in [
        "capture.log",
        "capture.json",
        "capture.report.json",
        "capture",
    ] {
        let capture = Path::new(name);
        let report = report_path(capture);
        assert_ne!(report, capture);
        assert_eq!(report, PathBuf::from(format!("{name}.report.json")));
    }
}

struct FakeRun {
    passed: bool,
    error: String,
    report: Value,
    commands: Vec<String>,
    capture: String,
}

fn fake_run(responses: Vec<String>) -> Result<FakeRun> {
    fake_run_options(responses, vec!["PONG".to_owned()], 0)
}

fn fake_run_options(
    responses: Vec<String>,
    ping_responses: Vec<String>,
    observe_ms: u64,
) -> Result<FakeRun> {
    fake_run_provider(
        responses,
        ping_responses,
        observe_ms,
        FixtureProvider::Battery,
    )
}

fn fake_run_provider(
    responses: Vec<String>,
    ping_responses: Vec<String>,
    observe_ms: u64,
    provider: FixtureProvider,
) -> Result<FakeRun> {
    fake_run_mode(
        responses,
        ping_responses,
        (observe_ms, provider, None, false),
    )
}

fn fake_response(
    master: &mut TTYPort,
    command: &str,
    pings: &mut impl Iterator<Item = String>,
    responses: &[String],
    observe_ms: u64,
    mode: (Option<bool>, bool),
) -> Result<()> {
    let (periodic_cancel, panel_cycle) = mode;
    if command == "PING" {
        writeln!(master, "{}\r", pings.next().unwrap_or_default())?;
    } else if panel_cycle
        && command == "REPAINT 41"
        && responses[0].starts_with("PANEL_FIXTURE END id=41")
    {
        writeln!(master, "{}\r", responses[0])?;
    } else if panel_cycle && command == "REPAINT 41" {
        master.write_all(format!("{}\r\n", panel_lines(41).join("\r\n")).as_bytes())?;
    } else if panel_cycle && command.starts_with("OBSPER ") && command.ends_with(" REPAINT") {
        master.write_all(format!("{}\r\n", responses.join("\r\n")).as_bytes())?;
    } else if command.ends_with(" CANCEL") {
        writeln!(master, "{}\r", responses.last().unwrap())?;
    } else if command.starts_with("OBSFIX ") || command.starts_with("OBSPER ") {
        let count = responses.len() - usize::from(periodic_cancel == Some(true));
        // Write a single packet to exercise buffered APPLIED/SAMPLE/RESULT.
        master.write_all(format!("{}\r\n", responses[..count].join("\r\n")).as_bytes())?;
        if observe_ms > 0 {
            master.flush()?;
            thread::sleep(Duration::from_millis(10));
            master.write_all(b"BATTERY_DELIVER fixture=post_window\r\n")?;
        }
    }
    Ok(())
}

fn fake_responder(
    mut master: TTYPort,
    responder_done: Arc<AtomicBool>,
    responses: Vec<String>,
    ping_responses: Vec<String>,
    observe_ms: u64,
    mode: (Option<bool>, bool),
) -> Result<Vec<String>> {
    let (periodic_cancel, panel_cycle) = mode;
    let mut rx = Vec::new();
    let mut chunk = [0; 512];
    let mut commands = Vec::new();
    let mut pings = ping_responses.into_iter();
    while !responder_done.load(Ordering::Acquire) {
        match master.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => rx.extend_from_slice(&chunk[..n]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                        | std::io::ErrorKind::WouldBlock
                ) =>
            {
                continue
            }
            Err(_) => break,
        }
        while let Some(end) = rx.iter().position(|byte| *byte == b'\n') {
            let bytes: Vec<_> = rx.drain(..=end).collect();
            let command = String::from_utf8(bytes)?.trim().to_owned();
            fake_response(
                &mut master,
                &command,
                &mut pings,
                &responses,
                observe_ms,
                (periodic_cancel, panel_cycle),
            )?;
            master.flush()?;
            commands.push(command);
        }
    }
    Ok(commands)
}

fn fake_run_mode(
    responses: Vec<String>,
    ping_responses: Vec<String>,
    config: (u64, FixtureProvider, Option<bool>, bool),
) -> Result<FakeRun> {
    let (observe_ms, provider, periodic_cancel, panel_cycle) = config;
    let (mut master, mut slave) = TTYPort::pair()?;
    master.set_timeout(Duration::from_millis(10))?;
    slave.set_timeout(Duration::from_millis(5))?;
    let done = Arc::new(AtomicBool::new(false));
    let responder_done = done.clone();
    let responder = thread::spawn(move || {
        fake_responder(
            master,
            responder_done,
            responses,
            ping_responses,
            observe_ms,
            (periodic_cancel, panel_cycle),
        )
    });
    let directory = tempdir()?;
    let log_path = directory.path().join("fixture.log");
    let console = SerialConsole::from_port_for_tests(Box::new(slave), Some(&log_path))?;
    let workflow = load_workflow(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        if panel_cycle {
            "scenarios/observation-panel-cycle.sw.yaml"
        } else if periodic_cancel.is_some() {
            "scenarios/observation-periodic.sw.yaml"
        } else {
            "scenarios/observation-fixture.sw.yaml"
        },
    ))?;
    let mut logger = Logger::new(None)?;
    let mut runtime = ObservationFixtureRuntime {
        logger: &mut logger,
        console,
        id: 42,
        provider,
        period_ms: periodic_cancel.map(|_| 60_000),
        cancel_after_samples: periodic_cancel.filter(|cancel| *cancel).map(|_| 2),
        periodic_evidence: None,
        panel_cycle,
        lifecycle_evidence: None,
        preflight_mark: 0,
        validity_ms: if periodic_cancel.is_some() {
            150_000
        } else {
            300
        },
        timeout: Duration::from_millis(100),
        observe: Duration::from_millis(observe_ms),
        log_path: log_path.clone(),
        command_mark: 0,
        result: None,
    };
    let result = execute_workflow(
        &workflow,
        &mut runtime,
        &json!({ "fixture_passed": false, "cancel_requested": periodic_cancel == Some(true) }),
    );
    done.store(true, Ordering::Release);
    let commands = responder.join().expect("fake UART responder panicked")?;
    let report = serde_json::from_slice(&std::fs::read(report_path(&log_path))?)?;
    Ok(FakeRun {
        passed: result.is_ok(),
        error: result
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default(),
        report,
        commands,
        capture: std::fs::read_to_string(log_path)?,
    })
}

#[test]
fn workflow_ignores_other_ids_and_queued_until_exact_result() -> Result<()> {
    let run = fake_run(vec![
        sampled(41),
        sampled(420),
        "OBSFIX QUEUED id=42 provider=2".to_owned(),
        sampled(42),
    ])?;
    assert!(run.passed, "{}", run.error);
    assert_eq!(run.report["passed"], true);
    assert_eq!(run.report["result"]["id"], 42);
    assert_eq!(run.commands, ["PING", "OBSFIX BATTERY 42 300"]);
    Ok(())
}

#[test]
fn queued_and_wrong_ids_timeout_without_retry_or_cancellation() -> Result<()> {
    let run = fake_run(vec![
        "OBSFIX QUEUED id=42 provider=2".to_owned(),
        sampled(41),
    ])?;
    assert!(!run.passed);
    assert!(run
        .error
        .contains("remains eligible only until its firmware deadline"));
    assert!(run.error.contains("no cancellation or retry was sent"));
    assert_eq!(run.report["passed"], false);
    assert!(run.report["result"].is_null());
    assert_eq!(run.commands, ["PING", "OBSFIX BATTERY 42 300"]);
    Ok(())
}

#[test]
fn rejection_and_malformed_results_fail_and_keep_report() -> Result<()> {
    for response in [
        "OBSFIX RESULT id=42 provider=2 status=Busy".to_owned(),
        sampled(42).replace("percent=73", "percent=invalid"),
    ] {
        let run = fake_run(vec![response])?;
        assert!(!run.passed);
        assert_eq!(run.report["passed"], false);
        assert!(run.report["error"]["message"].is_string());
        assert_eq!(run.commands, ["PING", "OBSFIX BATTERY 42 300"]);
    }
    Ok(())
}

#[test]
fn boot_readiness_gets_one_confirming_ping_and_keeps_capture_open() -> Result<()> {
    let run = fake_run_options(
        vec![sampled(42)],
        vec![
            "BOOT_RESET reason=Some(ChipPowerOn) code=1\r\nRUNTIME_READY app_state=ready display=ready".to_owned(),
            "PONG".to_owned(),
        ],
        40,
    )?;
    assert!(run.passed, "{}", run.error);
    assert_eq!(run.commands, ["PING", "PING", "OBSFIX BATTERY 42 300"]);
    assert_eq!(run.report["sample_verified"], true);
    assert_eq!(run.report["observation_completed"], true);
    assert_eq!(run.report["observe_ms"], 40);
    assert!(run.capture.contains("BATTERY_DELIVER fixture=post_window"));
    Ok(())
}

#[test]
fn missing_live_pong_never_submits_fixture() -> Result<()> {
    for pings in [
        vec![String::new()],
        vec![
            "RUNTIME_READY app_state=ready display=ready".to_owned(),
            String::new(),
        ],
    ] {
        let run = fake_run_options(vec![sampled(42)], pings, 0)?;
        assert!(!run.passed);
        assert!(run.commands.iter().all(|command| command == "PING"));
        assert_eq!(run.report["sample_verified"], false);
        assert_eq!(run.report["observation_completed"], false);
        assert!(run.report["error"]["message"].is_string());
    }
    Ok(())
}

#[test]
fn bme_combined_sample_is_correlated_and_requires_both_values() -> Result<()> {
    let line = sampled(42)
        .replace("provider=2", "provider=1")
        .replace("fields=1", "fields=3")
        .replace(
            "percent=73",
            "temperature_centidegrees=-250 humidity_millipercent=41000",
        );
    verify_sample(
        &parse_result(&line, FixtureProvider::Bme688)?,
        42,
        300,
        FixtureProvider::Bme688,
    )?;
    let run = fake_run_provider(
        vec![line.clone()],
        vec!["PONG".to_owned()],
        0,
        FixtureProvider::Bme688,
    )?;
    assert!(run.passed, "{}", run.error);
    assert_eq!(run.commands, ["PING", "OBSFIX BME688 42 300"]);
    assert_eq!(run.report["provider"], 1);
    assert_eq!(
        run.report["result"]["sample"]["temperature_centidegrees"],
        -250
    );
    for (old, new) in [
        (
            "temperature_centidegrees=-250",
            "temperature_centidegrees=none",
        ),
        ("humidity_millipercent=41000", "humidity_millipercent=none"),
        ("fields=3", "fields=1"),
        ("baseline_sample_revision=4", "baseline_sample_revision=5"),
    ] {
        assert!(verify_sample(
            &parse_result(&line.replace(old, new), FixtureProvider::Bme688)?,
            42,
            300,
            FixtureProvider::Bme688
        )
        .is_err());
    }
    assert!(verify_sample(
        &parse_result(&sampled(42), FixtureProvider::Battery)?,
        42,
        300,
        FixtureProvider::Bme688
    )
    .is_err());
    Ok(())
}

#[test]
fn medinote_providers_use_correct_units_and_cannot_alias_inkplate_samples() -> Result<()> {
    let shtc3 = sampled(42)
        .replace("provider=2", "provider=1")
        .replace("fields=1", "fields=3")
        .replace(
            "percent=73",
            "temperature_millicelsius=-1250 humidity_millipercent=42000",
        );
    let adc = sampled(42).replace("fields=1", "fields=3") + " millivolts=3800";
    for (provider, line, expected_command, value_key, value) in [
        (
            FixtureProvider::Shtc3,
            shtc3.clone(),
            "OBSFIX SHTC3 42 300",
            "temperature_millicelsius",
            -1250,
        ),
        (
            FixtureProvider::Adc,
            adc.clone(),
            "OBSFIX ADC 42 300",
            "millivolts",
            3800,
        ),
    ] {
        verify_sample(&parse_result(&line, provider)?, 42, 300, provider)?;
        let run = fake_run_provider(vec![line.clone()], vec!["PONG".to_owned()], 0, provider)?;
        assert!(run.passed, "{}", run.error);
        assert_eq!(run.commands, ["PING", expected_command]);
        assert_eq!(run.report["provider_kind"], provider.command());
        assert_eq!(run.report["result"]["sample"][value_key], value);
        let without_value = line.replace(&format!("{value_key}={value}"), "");
        assert!(parse_result(&without_value, provider).is_err());
        let no_value = line.replace(
            &format!("{value_key}={value}"),
            &format!("{value_key}=none"),
        );
        assert!(verify_sample(&parse_result(&no_value, provider)?, 42, 300, provider).is_err());
    }
    assert!(parse_result(&shtc3, FixtureProvider::Bme688).is_err());
    let bme = shtc3.replace(
        "temperature_millicelsius=-1250",
        "temperature_centidegrees=-125",
    );
    assert!(parse_result(&bme, FixtureProvider::Shtc3).is_err());
    assert!(parse_result(
        &bme.replace("humidity_millipercent=42000", "humidity_millipercent=-1"),
        FixtureProvider::Bme688
    )
    .is_err());
    assert!(parse_result(&sampled(42), FixtureProvider::Adc).is_err());
    assert!(verify_sample(
        &parse_result(&adc, FixtureProvider::Battery)?,
        42,
        300,
        FixtureProvider::Battery
    )
    .is_err());
    Ok(())
}

fn periodic_lines(cancelled: bool) -> Vec<String> {
    let event = "id=42 provider=2 applied_at_ms=100 expires_at_ms=150100 interval_ms=60000 live_fields=1 live_age0_ms=300000 live_age1_ms=none";
    vec![
        format!("OBSPER APPLIED {event} status=Applied at_ms=100"),
        "OBSPER SAMPLE id=42 provider=2 fields=1 generation=0 revision=3 sampled_at_ms=100 health=Ok percent=73".into(),
        "OBSPER SAMPLE id=42 provider=2 fields=1 generation=0 revision=4 sampled_at_ms=60100 health=Ok percent=73".into(),
        format!("OBSPER RESULT {event} status={} at_ms={}", if cancelled { "Cancelled" } else { "Restored" }, if cancelled { 60200 } else { 150100 }),
    ]
}

#[test]
fn periodic_evidence_requires_application_new_samples_and_restoration() -> Result<()> {
    for cancelled in [false, true] {
        periodic::verify(
            &periodic_lines(cancelled),
            42,
            FixtureProvider::Battery,
            60_000,
            150_000,
            cancelled,
        )?;
    }
    for (index, old, new) in [
        (0, "status=Applied", "status=Restored"),
        (0, "interval_ms=60000", "interval_ms=60001"),
        (0, "applied_at_ms=100", "applied_at_ms=none"),
        (1, "percent=73", "percent=101"),
        (1, "provider=2", "provider=1"),
        (1, "fields=1", "fields=3"),
        (1, "revision=3", "revision=0"),
        (1, "health=Ok", "health=Failed"),
        (1, "sampled_at_ms=100", "sampled_at_ms=99"),
        (2, "revision=4", "revision=3"),
        (2, "generation=0", "generation=1"),
        (2, "sampled_at_ms=60100", "sampled_at_ms=60099"),
        (2, "sampled_at_ms=60100", "sampled_at_ms=150100"),
        (3, "at_ms=150100", "at_ms=150099"),
        (3, "live_fields=1", "live_fields=0"),
        (3, "status=Restored", "status=Closed"),
    ] {
        let mut lines = periodic_lines(false);
        lines[index] = lines[index].replace(old, new);
        assert!(
            periodic::verify(&lines, 42, FixtureProvider::Battery, 60_000, 150_000, false).is_err(),
            "accepted {new}"
        );
    }
    for index in 0..4 {
        let mut lines = periodic_lines(false);
        lines.remove(index);
        assert!(
            periodic::verify(&lines, 42, FixtureProvider::Battery, 60_000, 150_000, false).is_err()
        );
    }
    Ok(())
}

#[test]
fn periodic_workflow_handles_batched_expiry_and_correlated_cancellation() -> Result<()> {
    for cancelled in [false, true] {
        let run = fake_run_mode(
            periodic_lines(cancelled),
            vec!["PONG".into()],
            (0, FixtureProvider::Battery, Some(cancelled), false),
        )?;
        assert!(run.passed, "{}", run.error);
        assert_eq!(
            run.report["periodic"]["samples"].as_array().unwrap().len(),
            2
        );
        let mut expected = vec!["PING", "OBSPER BATTERY 42 60000 150000"];
        if cancelled {
            expected.push("OBSPER BATTERY 42 CANCEL");
        }
        assert_eq!(run.commands, expected);
    }
    Ok(())
}

#[test]
fn periodic_workflow_failure_retains_report_without_retry() -> Result<()> {
    for lines in [
        vec!["OBSPER QUEUED id=42 provider=2".into()],
        vec!["OBSPER RESULT id=42 provider=2 status=Busy".into()],
    ] {
        let run = fake_run_mode(
            lines,
            vec!["PONG".into()],
            (0, FixtureProvider::Battery, Some(false), false),
        )?;
        assert!(!run.passed);
        assert_eq!(run.report["passed"], false);
        assert_eq!(run.commands, ["PING", "OBSPER BATTERY 42 60000 150000"]);
    }
    Ok(())
}

#[test]
fn periodic_option_limits_are_explicit_and_independent_of_one_shot() -> Result<()> {
    periodic::validate(Some(60_000), None, 150_000)?;
    periodic::validate(Some(300_000), Some(2), 900_000)?;
    validate_options(1, 900_000, 1, 0, true)?;
    for (period, cancel, validity) in [
        (None, Some(2), 150_000),
        (Some(59_999), None, 150_000),
        (Some(300_001), None, 900_000),
        (Some(60_000), None, 60_000),
        (Some(60_000), None, 900_001),
        (Some(60_000), Some(1), 150_000),
        (Some(60_000), Some(16), 150_000),
    ] {
        assert!(periodic::validate(period, cancel, validity).is_err());
    }
    Ok(())
}

fn panel_lines(id: u64) -> Vec<String> {
    vec![
        format!("PANEL_FIXTURE BEGIN id={id} at_ms=200"),
        format!("PANEL_FIXTURE CONTROL id={id} phase=suspend all_ok=true environment_id=10 environment_ack=Some(Quiesced) battery_id=20 battery_ack=Some(Quiesced)"),
        format!("PANEL_FIXTURE CONTROL id={id} phase=resume all_ok=true environment_id=11 environment_ack=Some(Running) battery_id=21 battery_ack=Some(Running)"),
        format!("PANEL_FIXTURE LIVE id={id} provider=2 fields=1 age0_ms=300000 age1_ms=none periodic_pending=false generation=0 revision=2 sampled_at_ms=100"),
        format!("PANEL_FIXTURE LIVE id={id} provider=1 fields=3 age0_ms=300000 age1_ms=300000 periodic_pending=false generation=0 revision=2 sampled_at_ms=100"),
        format!("PANEL_FIXTURE END id={id} status=Completed at_ms=400"),
    ]
}
fn lifecycle_lines(provider: FixtureProvider) -> Vec<String> {
    let mut lines = panel_lines(42);
    let ages = if provider.fields() == 1 {
        "none"
    } else {
        "300000"
    };
    let event = format!("id=42 provider={} applied_at_ms=100 expires_at_ms=150100 interval_ms=60000 live_fields={} live_age0_ms=300000 live_age1_ms={ages}", provider.id(), provider.fields());
    lines.insert(1, format!("OBSPER RESULT {event} status=Closed at_ms=210"));
    lines.insert(
        0,
        format!("OBSPER APPLIED {event} status=Applied at_ms=100"),
    );
    let sample = match provider {
        FixtureProvider::Battery => "BATTERY_DELIVER percent=73",
        FixtureProvider::Bme688 => {
            "BME688_DELIVER temperature_centidegrees=-250 humidity_millipercent=40000"
        }
        _ => unreachable!(),
    };
    lines.push(format!("{sample} health=Ok generation=0 revision=3 last_sample_revision=Some(3) last_sample_at_ms=Some(300500) last_attempt_at_ms=Some(300500)"));
    lines
}
#[test]
fn lifecycle_workflow_requires_correlated_repaint_and_fresh_live_recovery() -> Result<()> {
    for provider in [FixtureProvider::Battery, FixtureProvider::Bme688] {
        let run = fake_run_mode(
            lifecycle_lines(provider),
            vec!["PONG".into()],
            (0, provider, Some(false), true),
        )?;
        assert!(run.passed, "{}", run.error);
        assert_eq!(run.report["lifecycle"]["fresh"]["revision"], 3);
        assert_eq!(
            run.commands,
            [
                "PING".to_owned(),
                "REPAINT 41".into(),
                format!("OBSPER {} 42 60000 150000 REPAINT", provider.command()),
            ]
        );
    }
    Ok(())
}
#[test]
fn lifecycle_rejects_unrelated_closure_stale_controls_and_unrestored_demand() {
    let original = lifecycle_lines(FixtureProvider::Battery);
    for (old, new) in [
        ("status=Closed", "status=Cancelled"),
        ("at_ms=210", "at_ms=199"),
        ("phase=suspend all_ok=true", "phase=suspend all_ok=false"),
        ("environment_ack=Some(Quiesced)", "environment_ack=None"),
        ("battery_ack=Some(Running)", "battery_ack=Some(Quiesced)"),
        ("battery_id=21", "battery_id=20"),
        ("periodic_pending=false", "periodic_pending=true"),
        ("age0_ms=300000", "age0_ms=60000"),
        ("status=Completed", "status=Failed"),
    ] {
        let lines: Vec<_> = original
            .iter()
            .map(|line| {
                if line.starts_with("OBSPER") {
                    line.clone()
                } else {
                    line.replace(old, new)
                }
            })
            .collect();
        let lines = if old.starts_with("status=Closed") || old == "at_ms=210" {
            original.iter().map(|line| line.replace(old, new)).collect()
        } else {
            lines
        };
        assert!(
            lifecycle::verify(&lines, 42, FixtureProvider::Battery, 60000, 150000).is_err(),
            "accepted {new}"
        );
    }
    let mut early = original.clone();
    early.swap(1, 2);
    assert!(lifecycle::verify(&early, 42, FixtureProvider::Battery, 60000, 150000).is_err());
    let mut duplicate = original.clone();
    duplicate.insert(3, original[2].clone());
    assert!(lifecycle::verify(&duplicate, 42, FixtureProvider::Battery, 60000, 150000).is_err());
}
#[test]
fn lifecycle_does_not_accept_cached_delivery_or_retry_after_failure() -> Result<()> {
    for mutation in ["cached", "missing", "failed", "restart"] {
        let mut lines = lifecycle_lines(FixtureProvider::Battery);
        let last = lines.last_mut().unwrap();
        match mutation {
            "cached" => *last = last.replace("Some(300500)", "Some(100)"),
            "missing" => {
                lines.pop();
            }
            "failed" => *last = last.replace("health=Ok", "health=Failed"),
            "restart" => *last = last.replace("generation=0", "generation=1"),
            _ => unreachable!(),
        }
        let run = fake_run_mode(
            lines,
            vec!["PONG".into()],
            (0, FixtureProvider::Battery, Some(false), true),
        )?;
        assert!(!run.passed, "accepted {mutation}");
        assert_eq!(run.commands.len(), 3);
        assert_eq!(run.report["sample_verified"], false);
    }
    Ok(())
}

#[test]
fn lifecycle_cli_and_validation_keep_other_targets_and_fixture_modes_separate() -> Result<()> {
    use clap::Parser;
    let cli = crate::Cli::try_parse_from([
        "hostctl",
        "test",
        "observation-fixture",
        "--panel-cycle",
        "--period-ms",
        "60000",
    ])?;
    let crate::Commands::Test(args) = cli.command else {
        panic!("wrong command")
    };
    let crate::TestSubcommand::ObservationFixture(args) = args.test else {
        panic!("wrong test")
    };
    assert!(args.panel_cycle);
    let mut opts = ObservationFixtureOptions {
        provider: FixtureProvider::Battery,
        panel_cycle: true,
        period_ms: Some(60000),
        cancel_after_samples: None,
        request_id: Some(42),
        validity_ms: 150000,
        timeout_ms: 420000,
        observe_ms: 0,
        output_path: None,
    };
    lifecycle::validate(&opts, 42)?;
    assert!(lifecycle::validate(&opts, 1).is_err());
    opts.provider = FixtureProvider::Shtc3;
    assert!(lifecycle::validate(&opts, 42).is_err());
    opts.provider = FixtureProvider::Bme688;
    opts.cancel_after_samples = Some(2);
    assert!(lifecycle::validate(&opts, 42).is_err());
    opts.cancel_after_samples = None;
    opts.period_ms = None;
    assert!(lifecycle::validate(&opts, 42).is_err());
    Ok(())
}

#[test]
fn lifecycle_refused_preflight_never_starts_a_fixture() -> Result<()> {
    let run = fake_run_mode(
        vec!["PANEL_FIXTURE END id=41 status=UploadBlocked".into()],
        vec!["PONG".into()],
        (0, FixtureProvider::Battery, Some(false), true),
    )?;
    assert!(!run.passed);
    assert!(run.error.contains("UploadBlocked"));
    assert_eq!(run.commands, ["PING", "REPAINT 41"]);
    Ok(())
}
