mod command_run;

use std::{fs, path::PathBuf};

use super::artifacts::{
    archive_firmware_artifacts, validate_post_command_options, ArchiveFirmwareArtifactsOptions,
};
use super::flash::{
    build_app_flash_command, build_full_flash_command, resolve_partition_offset,
    FullFlashCommandOptions,
};
use super::paths::{
    acquire_port_lock, build_firmware_command, normalize_output_root, prepare_output_paths,
};
use super::{CaptureMode, CommandSpec, FlashCaptureOptions, FlashMode, IdfEnv, DEFAULT_FLASH_BAUD};
use crate::scenarios::{execute_workflow, load_workflow, WorkflowRuntime};
use anyhow::Result;
use serde_json::Value;
use tempfile::tempdir;

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
    let source = temp.path().join("source.bin");
    fs::write(&source, b"deterministic firmware").expect("write source");
    let outputs = prepare_output_paths(Some(&temp.path().join("artifacts"))).expect("output paths");

    let bootloader = temp.path().join("target/ota-bootloader/bootloader");
    let partition_table = temp.path().join("target/ota-bootloader/partition_table");
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

#[derive(Default)]
struct RecordingRuntime {
    calls: Vec<(String, Value)>,
}

impl WorkflowRuntime for RecordingRuntime {
    fn invoke(&mut self, action: &str, args: &Value, _context: &mut Value) -> Result<()> {
        self.calls.push((action.to_owned(), args.clone()));
        Ok(())
    }
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
            "image_supplied": true,
            "fallback_allowed": false,
            "post_command_supplied": false,
        }),
    )
    .expect("app-only workflow");

    assert!(!runtime
        .calls
        .iter()
        .any(|(action, _)| action == "prepare_bootloader"));
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
fn app_offset_is_resolved_from_the_accepted_partition_table() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tools dir")
        .parent()
        .expect("repo root")
        .to_path_buf();
    let offset = resolve_partition_offset(&repo_root.join("config/partitions-ab.csv"), "ota_0")
        .expect("ota_0 offset");
    assert_eq!(offset, 0x20000);
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
fn firmware_build_command_accepts_the_ble_probe_profile() {
    let temp = tempdir().expect("tempdir");
    let scripts = temp.path().join("scripts/build");
    fs::create_dir_all(&scripts).expect("scripts dir");
    fs::write(scripts.join("build.sh"), "#!/bin/sh\n").expect("build script");

    let spec = build_firmware_command("ble-release", temp.path()).expect("ble profile");
    assert!(spec
        .args
        .iter()
        .any(|arg| arg.to_string_lossy() == "ble-release"));
}

#[test]
fn flash_capture_workflow_yaml_parses() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/flash-capture.sw.yaml");
    let workflow = load_workflow(&path).expect("load workflow");
    assert_eq!(workflow.document.name, "flash-capture");
}
