mod command_run;

use std::{fs, path::PathBuf};

use super::artifacts::{archive_firmware_artifacts, validate_post_command_options};
use super::flash::build_app_flash_command;
use super::paths::{
    acquire_port_lock, build_firmware_command, normalize_output_root, prepare_output_paths,
};
use super::{CaptureMode, CommandSpec, FlashCaptureOptions, FlashMode, IdfEnv, DEFAULT_FLASH_BAUD};
use crate::scenarios::load_workflow;
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
fn full_flash_default_baud_matches_board_safe_rate() {
    assert_eq!(DEFAULT_FLASH_BAUD, 115_200);
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

    archive_firmware_artifacts(&source, &outputs, temp.path(), "release", true).expect("archive");

    assert_eq!(
        fs::read(&outputs.app_bin).expect("app bin"),
        b"deterministic firmware"
    );
    let hashes = fs::read_to_string(&outputs.hashes).expect("hashes");
    assert!(hashes.ends_with("  app.bin\n"));
    let metadata = fs::read_to_string(&outputs.build_metadata).expect("metadata");
    assert!(metadata.contains("profile=release"));
    assert!(metadata.contains("git_status_begin"));
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
    assert!(args.contains(&"0x10000".to_string()));
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
fn flash_capture_workflow_yaml_parses() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/flash-capture.sw.yaml");
    let workflow = load_workflow(&path).expect("load workflow");
    assert_eq!(workflow.document.name, "flash-capture");
}
