mod command_run;

use std::{fs, path::PathBuf};

use super::artifacts::{
    archive_firmware_artifacts, validate_app_capacity, validate_post_command_options,
    ArchiveFirmwareArtifactsOptions,
};
use super::flash::{
    build_app_flash_command, build_full_flash_command, resolve_partition_offset,
    FullFlashCommandOptions,
};
use super::paths::{
    acquire_port_lock, build_firmware_command, build_single_production_bootloader_command,
    normalize_output_root, prepare_output_paths,
};
use super::{
    BootTarget, CaptureMode, CommandSpec, FlashCaptureOptions, FlashMode, IdfEnv,
    DEFAULT_FLASH_BAUD,
};
use crate::scenarios::{execute_workflow, load_workflow, WorkflowRuntime};
use anyhow::Result;
use serde_json::Value;
use tempfile::tempdir;

#[test]
fn time_sync_capture_retains_diagnostics_and_preserves_boot_capture() {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("capture.log");
    fs::write(&path, "BOOT_RESET\n").expect("boot capture");
    super::capture::append_time_sync_diagnostics(
        &path,
        &[
            "I2C_PROXY_TIMEOUT phase=owner_claim id=12 addr=0x51 elapsed_us=40001 claimed=false".into(),
            "I2C_PROXY_OWNER id=12 phase=waiting last_id=11 last_addr=0x48 age_us=42000 received=11 discarded=0 bus_addr=0xff bus_age_us=0".into(),
            "I2C_TIMEOUT source=queue elapsed_us=40001".into(),
            "I2C_ADMISSION_ERROR reason=full".into(),
            "RTC_DIAG op=write_calendar reg=0x04 addr=0x51 err=TransferTimeout".into(),
            "TIME_SYNC ERR reason=i2c".into(),
            "NETCFG STATUS private configuration".into(),
        ],
    )
    .expect("append");
    assert_eq!(fs::read_to_string(path).expect("capture"),
        "BOOT_RESET\nI2C_PROXY_TIMEOUT phase=owner_claim id=12 addr=0x51 elapsed_us=40001 claimed=false\nI2C_PROXY_OWNER id=12 phase=waiting last_id=11 last_addr=0x48 age_us=42000 received=11 discarded=0 bus_addr=0xff bus_age_us=0\nI2C_TIMEOUT source=queue elapsed_us=40001\nI2C_ADMISSION_ERROR reason=full\nRTC_DIAG op=write_calendar reg=0x04 addr=0x51 err=TransferTimeout\nTIME_SYNC ERR reason=i2c\n");
}

#[test]
fn acquire_port_lock_can_be_reacquired_after_drop() {
    let port = format!("/dev/cu.hostctl-test-lock-{}", std::process::id());
    let first = acquire_port_lock(&port).expect("first lock");
    drop(first);
    let _second = acquire_port_lock(&port).expect("reacquire");
}

#[test]
fn output_paths_default_under_repo_logs() {
    let paths = prepare_output_paths(None).expect("paths");
    assert!(paths.root.starts_with(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("tools dir")
            .parent()
            .expect("repo root")
            .join("logs")
    ));
    assert!(paths.flash_log.ends_with("flash.log"));
    assert!(paths.capture_log.ends_with("capture.log"));
    assert!(paths.post_command_log.ends_with("post-command.log"));
    assert!(paths.summary.ends_with("summary.txt"));
    assert!(paths.firmware_elf.ends_with("firmware.elf"));
    assert!(paths.app_bin.ends_with("app.bin"));
    assert!(paths.bootloader_bin.ends_with("bootloader.bin"));
    assert!(paths.partition_table_bin.ends_with("partition-table.bin"));
    assert!(paths.hashes.ends_with("sha256.txt"));
    assert!(paths.build_metadata.ends_with("build-metadata.txt"));
}

#[test]
fn normalize_output_root_keeps_directory_override() {
    let root = PathBuf::from("logs/flash_capture_manual");
    let (normalized, warning) = normalize_output_root(Some(root.as_path()));
    assert_eq!(normalized, Some(root));
    assert!(warning.is_none());
}

#[test]
fn normalize_output_root_rewrites_file_like_capture_log_path() {
    let input = PathBuf::from("logs/flash_capture_manual/capture.log");
    let (normalized, warning) = normalize_output_root(Some(input.as_path()));
    assert_eq!(normalized, Some(PathBuf::from("logs/flash_capture_manual")));
    assert!(warning
        .expect("warning")
        .contains("treating file-like --log path"));
}

fn flash_options() -> FlashCaptureOptions {
    FlashCaptureOptions {
        profile: "release".into(),
        output_path: None,
        port: None,
        flash_mode: FlashMode::Auto,
        boot_target: BootTarget::Factory,
        capture_mode: CaptureMode::Boot,
        image: None,
        flash_baud: None,
        baud: None,
        boot_window_ms: None,
        idf_root: None,
        idf_tools_path: None,
        post_command: None,
        post_pattern: None,
        post_timeout_ms: None,
        no_time_sync: false,
    }
}

#[test]
fn full_flash_default_baud_uses_the_proven_stub_rate() {
    assert_eq!(DEFAULT_FLASH_BAUD, 460_800);
}

#[test]
fn post_command_requires_pattern_before_flash() {
    let mut options = flash_options();
    options.post_command = Some("LVGLSOAK 24".into());
    assert!(validate_post_command_options(&options)
        .expect_err("missing pattern")
        .to_string()
        .contains("--post-pattern"));

    options.post_pattern = Some("LVGL_SOAK_END".into());
    options.post_timeout_ms = Some(0);
    assert!(validate_post_command_options(&options)
        .expect_err("zero timeout")
        .to_string()
        .contains("greater than zero"));
}

#[test]
fn explicit_app_image_is_archived_with_hash_and_metadata() {
    let temp = tempdir().expect("tempdir");
    write_test_partitions(temp.path());
    let source = temp.path().join("source.bin");
    fs::write(&source, b"deterministic firmware").expect("write source");
    let outputs = prepare_output_paths(Some(&temp.path().join("artifacts"))).expect("output paths");

    let bootloader = temp
        .path()
        .join("target/single-production-bootloader/bootloader");
    let partition_table = temp
        .path()
        .join("target/single-production-bootloader/partition_table");
    fs::create_dir_all(&bootloader).expect("bootloader dir");
    fs::create_dir_all(&partition_table).expect("partition dir");
    fs::write(bootloader.join("bootloader.bin"), b"bootloader").expect("bootloader");
    fs::write(partition_table.join("partition-table.bin"), b"partitions").expect("partitions");
    archive_firmware_artifacts(ArchiveFirmwareArtifactsOptions {
        image_path: &source,
        outputs: &outputs,
        repo_dir: temp.path(),
        profile: "release",
        skip_update_check: true,
        include_bootloader: true,
        built_in_workflow: false,
    })
    .expect("archive");

    assert_eq!(
        fs::read(&outputs.app_bin).expect("app bin"),
        b"deterministic firmware"
    );
    let hashes = fs::read_to_string(&outputs.hashes).expect("hashes");
    assert!(hashes.contains("  app.bin\n"));
    assert!(hashes.contains("  bootloader.bin\n"));
    assert!(hashes.ends_with("  partition-table.bin\n"));
    let metadata = fs::read_to_string(&outputs.build_metadata).expect("metadata");
    assert!(metadata.contains("profile=release"));
    assert!(metadata.contains("image_source=explicit"));
    assert!(metadata.contains("requested_features=unverified"));
    assert!(metadata.contains("git_status_begin"));
}

#[test]
fn explicit_app_only_archive_does_not_require_bootloader_artifacts() {
    let temp = tempdir().expect("tempdir");
    write_test_partitions(temp.path());
    let source = temp.path().join("source.bin");
    fs::write(&source, b"deterministic firmware").expect("write source");
    let outputs = prepare_output_paths(Some(&temp.path().join("artifacts"))).expect("output paths");
    fs::write(&outputs.bootloader_bin, b"stale bootloader").expect("stale bootloader");
    fs::write(&outputs.partition_table_bin, b"stale partitions").expect("stale partitions");

    archive_firmware_artifacts(ArchiveFirmwareArtifactsOptions {
        image_path: &source,
        outputs: &outputs,
        repo_dir: temp.path(),
        profile: "release",
        skip_update_check: true,
        include_bootloader: false,
        built_in_workflow: false,
    })
    .expect("app-only archive");

    assert_eq!(
        fs::read(&outputs.app_bin).expect("app bin"),
        b"deterministic firmware"
    );
    assert!(!outputs.bootloader_bin.exists());
    assert!(!outputs.partition_table_bin.exists());
    let hashes = fs::read_to_string(&outputs.hashes).expect("hashes");
    assert!(hashes.contains("  app.bin\n"));
    assert!(!hashes.contains("bootloader.bin"));
    assert!(!hashes.contains("partition-table.bin"));
}

fn write_test_partitions(root: &std::path::Path) {
    fs::create_dir_all(root.join("config")).unwrap();
    fs::write(
        root.join("config/partitions-single-production.csv"),
        include_str!("../../../../../../config/partitions-single-production.csv"),
    )
    .unwrap();
}

#[test]
fn flash_capacity_uses_current_partition_and_preserves_headroom() {
    let temp = tempdir().unwrap();
    write_test_partitions(temp.path());
    let partitions = temp.path().join("config/partitions-single-production.csv");
    let image = temp.path().join("image.bin");
    let file = fs::File::create(&image).unwrap();
    // This rejected ordinary release image fits the single-production layout.
    file.set_len(1_957_584).unwrap();
    validate_app_capacity(&image, &partitions).unwrap();
    file.set_len(0x380000 - 0x20000).unwrap();
    validate_app_capacity(&image, &partitions).unwrap();
    file.set_len(0x380000 - 0x20000 + 1).unwrap();
    assert!(validate_app_capacity(&image, &partitions).is_err());
    // Follow a changed partition, rather than another compiled-in slot size.
    fs::write(&partitions, "ota_0,app,ota_0,0x80000,0x30000,\n").unwrap();
    file.set_len(0x10000).unwrap();
    validate_app_capacity(&image, &partitions).unwrap();
    file.set_len(0x10001).unwrap();
    assert!(validate_app_capacity(&image, &partitions).is_err());
    for invalid in [
        "",
        "ota_0,app,ota_0,0x80000,,",
        "ota_0,app,ota_0,0x80000,bad,",
        "ota_0,app,ota_0,0x80000,0x1000,",
    ] {
        fs::write(&partitions, invalid).unwrap();
        assert!(validate_app_capacity(&image, &partitions).is_err());
    }
}

#[derive(Default)]
struct RecordingRuntime {
    calls: Vec<(String, Value)>,
    /// When set, the simulated `time_sync` action reports failure instead of
    /// success, mirroring what the real action does when `sync_time` errors.
    force_time_sync_failure: bool,
}

impl WorkflowRuntime for RecordingRuntime {
    fn invoke(&mut self, action: &str, args: &Value, context: &mut Value) -> Result<()> {
        self.calls.push((action.to_owned(), args.clone()));
        match action {
            // Mirror the one piece of context the real `time_sync` action
            // always sets, so the workflow's `time_sync_result_gate` has a
            // field to read regardless of which fixture is driving it.
            "time_sync" => {
                if let Some(map) = context.as_object_mut() {
                    map.insert(
                        "time_sync_ok".to_string(),
                        Value::Bool(!self.force_time_sync_failure),
                    );
                }
                Ok(())
            }
            // The real action_fail_time_sync always errors when reached;
            // faithfully reproduce that rather than silently succeeding.
            "fail_time_sync" => Err(anyhow::anyhow!("simulated time sync failure")),
            _ => Ok(()),
        }
    }
}

fn action_index(calls: &[(String, Value)], action: &str) -> Option<usize> {
    calls.iter().position(|(name, _)| name == action)
}

#[test]
fn time_sync_runs_after_capture_and_before_the_summary_for_every_capture_mode() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/flash-capture.sw.yaml");
    let workflow = load_workflow(&path).expect("load workflow");

    for capture_mode in ["none", "stream", "boot"] {
        let mut runtime = RecordingRuntime::default();
        execute_workflow(
            &workflow,
            &mut runtime,
            &serde_json::json!({
                "flash_mode": "app-only",
                "capture_mode": capture_mode,
                "reset_after_flash": capture_mode != "boot",
                "image_supplied": true,
                "fallback_allowed": false,
                "post_command_supplied": false,
                "time_sync_enabled": true,
            }),
        )
        .unwrap_or_else(|error| panic!("{capture_mode} workflow: {error}"));

        let capture_index = action_index(&runtime.calls, "capture").expect("capture ran");
        let time_sync_index = action_index(&runtime.calls, "time_sync").expect("time_sync ran");
        let write_summary_index =
            action_index(&runtime.calls, "write_summary").expect("write_summary ran");
        assert!(
            capture_index < time_sync_index,
            "{capture_mode}: time_sync must run after capture"
        );
        assert!(
            time_sync_index < write_summary_index,
            "{capture_mode}: time_sync must run before write_summary"
        );
    }
}

#[test]
fn time_sync_runs_before_the_post_command_when_one_is_supplied() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/flash-capture.sw.yaml");
    let workflow = load_workflow(&path).expect("load workflow");
    let mut runtime = RecordingRuntime::default();
    execute_workflow(
        &workflow,
        &mut runtime,
        &serde_json::json!({
            "flash_mode": "app-only",
            "capture_mode": "none",
            "reset_after_flash": true,
            "image_supplied": true,
            "fallback_allowed": false,
            "post_command_supplied": true,
            "time_sync_enabled": true,
        }),
    )
    .expect("workflow with a post command");

    let time_sync_index = action_index(&runtime.calls, "time_sync").expect("time_sync ran");
    let post_command_index =
        action_index(&runtime.calls, "post_command").expect("post_command ran");
    assert!(time_sync_index < post_command_index);
}

#[test]
fn a_rejected_time_sync_still_writes_the_summary_before_the_workflow_fails() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/flash-capture.sw.yaml");
    let workflow = load_workflow(&path).expect("load workflow");
    let mut runtime = RecordingRuntime {
        force_time_sync_failure: true,
        ..Default::default()
    };
    let result = execute_workflow(
        &workflow,
        &mut runtime,
        &serde_json::json!({
            "flash_mode": "app-only",
            "capture_mode": "none",
            "reset_after_flash": true,
            "image_supplied": true,
            "fallback_allowed": false,
            "post_command_supplied": false,
            "time_sync_enabled": true,
        }),
    );

    assert!(
        result.is_err(),
        "the workflow must fail overall on a rejected time sync"
    );
    let write_summary_index =
        action_index(&runtime.calls, "write_summary").expect("summary was written");
    let fail_index = action_index(&runtime.calls, "fail_time_sync").expect("fail_time_sync ran");
    assert!(
        write_summary_index < fail_index,
        "the summary must be written before the terminal failing action runs"
    );
}

#[test]
fn a_rejected_time_sync_skips_the_post_command_but_still_writes_the_summary() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/flash-capture.sw.yaml");
    let workflow = load_workflow(&path).expect("load workflow");
    let mut runtime = RecordingRuntime {
        force_time_sync_failure: true,
        ..Default::default()
    };
    let result = execute_workflow(
        &workflow,
        &mut runtime,
        &serde_json::json!({
            "flash_mode": "app-only",
            "capture_mode": "none",
            "reset_after_flash": true,
            "image_supplied": true,
            "fallback_allowed": false,
            "post_command_supplied": true,
            "time_sync_enabled": true,
        }),
    );

    assert!(
        result.is_err(),
        "the workflow must fail overall on a rejected time sync"
    );
    assert!(
        !runtime
            .calls
            .iter()
            .any(|(action, _)| action == "post_command"),
        "the generic post-command must not run after a failed sync"
    );
    assert!(
        action_index(&runtime.calls, "write_summary").is_some(),
        "the summary must still be written"
    );
}

#[test]
fn find_captured_request_decodes_a_time_request_line_from_the_boot_log() {
    let temp = tempdir().expect("tempdir");
    let log = temp.path().join("capture.log");
    fs::write(
        &log,
        b"BOOT_PHASES_MS ...\r\nTIME_REQUEST version=1 session=3 nonce=99 reason=invalid_cold_boot deadline_ms=15000\r\nLVGL_INIT ...\r\n",
    )
    .expect("write capture log");

    let request = super::capture::find_captured_request(&log).expect("request found");
    assert_eq!(request.session, 3);
    assert_eq!(request.nonce, 99);
    assert_eq!(
        request.trigger,
        wall_clock::message::TriggerReason::InvalidColdBoot
    );
    assert_eq!(request.deadline_ms, 15_000);
}

#[test]
fn find_captured_request_picks_the_last_of_more_than_one() {
    let temp = tempdir().expect("tempdir");
    let log = temp.path().join("capture.log");
    fs::write(
        &log,
        b"TIME_REQUEST version=1 session=1 nonce=1 reason=invalid_cold_boot deadline_ms=15000\r\nTIME_REQUEST version=1 session=2 nonce=2 reason=cold_boot_offer deadline_ms=15000\r\n",
    )
    .expect("write capture log");

    let request = super::capture::find_captured_request(&log).expect("request found");
    assert_eq!(request.session, 2);
}

#[test]
fn find_captured_request_is_none_when_the_log_has_no_time_request() {
    let temp = tempdir().expect("tempdir");
    let log = temp.path().join("capture.log");
    fs::write(&log, b"BOOT_PHASES_MS ...\r\nLVGL_INIT ...\r\n").expect("write capture log");

    assert!(super::capture::find_captured_request(&log).is_none());
}

#[test]
fn find_captured_request_is_none_for_a_missing_log_file() {
    let temp = tempdir().expect("tempdir");
    let missing = temp.path().join("does-not-exist.log");
    assert!(super::capture::find_captured_request(&missing).is_none());
}

#[test]
fn app_only_workflow_skips_bootloader_preparation() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/flash-capture.sw.yaml");
    let workflow = load_workflow(&path).expect("load workflow");
    let mut runtime = RecordingRuntime::default();
    execute_workflow(
        &workflow,
        &mut runtime,
        &serde_json::json!({
            "flash_mode": "app-only",
            "capture_mode": "none",
            "reset_after_flash": true,
            "image_supplied": true,
            "fallback_allowed": false,
            "post_command_supplied": false,
        }),
    )
    .expect("app-only workflow");

    assert!(!runtime
        .calls
        .iter()
        .any(|(action, _)| action == "prepare_flash_layout"));
    let archive_args = runtime
        .calls
        .iter()
        .find_map(|(action, args)| (action == "archive_image").then_some(args))
        .expect("archive call");
    assert_eq!(archive_args["include_bootloader"], false);
    let flash_args = runtime
        .calls
        .iter()
        .find_map(|(action, args)| (action == "flash").then_some(args))
        .expect("flash call");
    assert_eq!(flash_args["strategy"], "app-only");
}

#[test]
fn full_workflow_passes_the_requested_boot_target_to_layout_preparation() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/flash-capture.sw.yaml");
    let workflow = load_workflow(&path).expect("load workflow");
    let mut runtime = RecordingRuntime::default();
    execute_workflow(
        &workflow,
        &mut runtime,
        &serde_json::json!({
            "flash_mode": "full",
            "boot_target": "production",
            "capture_mode": "none",
            "reset_after_flash": false,
            "image_supplied": true,
            "fallback_allowed": false,
            "post_command_supplied": false,
            "time_sync_enabled": false,
        }),
    )
    .expect("full production workflow");

    let prepare_args = runtime
        .calls
        .iter()
        .find_map(|(action, args)| (action == "prepare_flash_layout").then_some(args))
        .expect("layout preparation call");
    assert_eq!(prepare_args["boot_target"], "production");
    let flash_args = runtime
        .calls
        .iter()
        .find_map(|(action, args)| (action == "flash").then_some(args))
        .expect("flash call");
    assert_eq!(flash_args["reset_after_flash"], false);
}

#[test]
fn app_only_flash_command_uses_idf_python_and_esptool() {
    let idf_env = IdfEnv {
        idf_root: PathBuf::from("/tmp/idf"),
        python_bin: PathBuf::from("/tmp/idf/python"),
        esptool_bin: PathBuf::from("/tmp/idf/esptool.py"),
        idf_py_bin: None,
    };
    let spec = build_app_flash_command(
        &idf_env,
        "/dev/cu.usbserial-510",
        115_200,
        true,
        0x20000,
        PathBuf::from("/tmp/app.bin").as_path(),
    );
    assert_eq!(spec.program, idf_env.python_bin.into_os_string());
    let args: Vec<_> = spec
        .args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args[0], "/tmp/idf/esptool.py");
    assert!(args.contains(&"--no-stub".to_string()));
    assert!(args.contains(&"0x20000".to_string()));
}

#[test]
fn full_flash_command_uses_pinned_artifacts_and_resolved_offsets() {
    let idf_env = IdfEnv {
        idf_root: PathBuf::from("/tmp/idf"),
        python_bin: PathBuf::from("/tmp/idf/python"),
        esptool_bin: PathBuf::from("/tmp/idf/esptool.py"),
        idf_py_bin: None,
    };
    let bootloader = PathBuf::from("/tmp/bootloader.bin");
    let partition_table = PathBuf::from("/tmp/partition-table.bin");
    let ota_data = PathBuf::from("/tmp/ota-data.bin");
    let app_bin = PathBuf::from("/tmp/app.bin");
    let spec = build_full_flash_command(FullFlashCommandOptions {
        idf_env: &idf_env,
        port: "/dev/cu.usbserial-510",
        flash_baud: 115_200,
        no_stub: false,
        reset_after_flash: true,
        bootloader: &bootloader,
        partition_table: &partition_table,
        ota_data_offset: 0xf000,
        ota_data: &ota_data,
        app_offset: 0x20000,
        app_bin: &app_bin,
    });
    let args: Vec<_> = spec
        .args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(spec.program, idf_env.python_bin.into_os_string());
    assert!(!args.contains(&"--no-stub".to_string()));
    assert!(args.contains(&"/tmp/bootloader.bin".to_string()));
    assert!(args.contains(&"/tmp/partition-table.bin".to_string()));
    assert!(args.contains(&"0xf000".to_string()));
    assert!(args.contains(&"0x20000".to_string()));
    assert!(args.contains(&"/tmp/app.bin".to_string()));
}

#[test]
fn conservative_full_flash_disables_the_stub() {
    let idf_env = IdfEnv {
        idf_root: PathBuf::from("/tmp/idf"),
        python_bin: PathBuf::from("/tmp/idf/python"),
        esptool_bin: PathBuf::from("/tmp/idf/esptool.py"),
        idf_py_bin: None,
    };
    let artifact = PathBuf::from("/tmp/artifact.bin");
    let spec = build_full_flash_command(FullFlashCommandOptions {
        idf_env: &idf_env,
        port: "/dev/cu.usbserial-510",
        flash_baud: 115_200,
        no_stub: true,
        reset_after_flash: true,
        bootloader: &artifact,
        partition_table: &artifact,
        ota_data_offset: 0xf000,
        ota_data: &artifact,
        app_offset: 0x20000,
        app_bin: &artifact,
    });
    let args: Vec<_> = spec
        .args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert!(args.contains(&"--no-stub".to_string()));
}

#[test]
fn boot_capture_defers_reset_until_the_serial_port_is_open() {
    let idf_env = IdfEnv {
        idf_root: PathBuf::from("/tmp/idf"),
        python_bin: PathBuf::from("/tmp/idf/python"),
        esptool_bin: PathBuf::from("/tmp/idf/esptool.py"),
        idf_py_bin: None,
    };
    let artifact = PathBuf::from("/tmp/artifact.bin");
    let spec = build_full_flash_command(FullFlashCommandOptions {
        idf_env: &idf_env,
        port: "/dev/cu.usbserial-510",
        flash_baud: 460_800,
        no_stub: false,
        reset_after_flash: false,
        bootloader: &artifact,
        partition_table: &artifact,
        ota_data_offset: 0xf000,
        ota_data: &artifact,
        app_offset: 0x80000,
        app_bin: &artifact,
    });
    let args: Vec<_> = spec
        .args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert!(args.windows(2).any(|args| args == ["--after", "no_reset"]));
}

#[test]
fn app_offset_is_resolved_from_the_accepted_partition_table() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tools dir")
        .parent()
        .expect("repo root")
        .to_path_buf();
    let offset = resolve_partition_offset(
        &repo_root.join("config/partitions-single-production.csv"),
        "ota_0",
    )
    .expect("ota_0 offset");
    assert_eq!(offset, 0x80000);
}

#[test]
fn command_spec_records_current_dir() {
    let spec = CommandSpec::new("cargo")
        .arg("build")
        .current_dir("/tmp/worktree");
    assert_eq!(spec.current_dir, Some(PathBuf::from("/tmp/worktree")));
}

#[test]
fn command_spec_can_clear_inherited_env() {
    let spec = CommandSpec::new("cargo")
        .arg("build")
        .env_remove("RUSTUP_TOOLCHAIN");
    assert_eq!(spec.env_remove, vec!["RUSTUP_TOOLCHAIN"]);
}

#[test]
fn firmware_build_command_uses_canonical_script_and_clears_host_overrides() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tools dir")
        .parent()
        .expect("repo root")
        .to_path_buf();
    let spec = build_firmware_command("debug", &repo_root).expect("build command");
    let program = spec.program.to_string_lossy();
    assert!(program.ends_with("scripts/build/build.sh"));
    assert_eq!(spec.current_dir, Some(repo_root));
    assert!(spec.env_remove.contains(&"RUSTUP_TOOLCHAIN".into()));
    assert!(spec.env_remove.contains(&"CARGO_BUILD_TARGET".into()));
    assert!(spec.env_remove.contains(&"CARGO_ENCODED_RUSTFLAGS".into()));
    assert!(spec.env_remove.contains(&"RUSTFLAGS".into()));
}

#[test]
fn flash_layout_build_uses_the_single_production_bootloader_script() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tools dir")
        .parent()
        .expect("repo root")
        .to_path_buf();
    let spec = build_single_production_bootloader_command(&repo_root).expect("bootloader command");
    assert!(spec
        .program
        .to_string_lossy()
        .ends_with("scripts/build/single_production_bootloader.sh"));
    assert_eq!(spec.current_dir, Some(repo_root));
}

#[test]
fn firmware_build_command_rejects_removed_modes() {
    let temp = tempdir().expect("tempdir");
    let scripts = temp.path().join("scripts/build");
    fs::create_dir_all(&scripts).expect("scripts dir");
    fs::write(scripts.join("build.sh"), "#!/bin/sh\n").expect("build script");

    for mode in ["ble-release", "clippy"] {
        assert!(build_firmware_command(mode, temp.path()).is_err());
    }
}

#[test]
fn flash_capture_workflow_yaml_parses() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/flash-capture.sw.yaml");
    let workflow = load_workflow(&path).expect("load workflow");
    assert_eq!(workflow.document.name, "flash-capture");
}
