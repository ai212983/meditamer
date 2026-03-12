#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, time::Duration};

    use super::{
        acquire_port_lock, build_app_flash_command, build_firmware_command, normalize_output_root,
        prepare_output_paths, run_command_logged, CommandProgressMode, CommandRunOptions,
        CommandSpec, IdfEnv,
    };
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
        assert!(paths.root.starts_with(PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().expect("tools dir").parent().expect("repo root").join("logs")));
        assert!(paths.flash_log.ends_with("flash.log"));
        assert!(paths.capture_log.ends_with("capture.log"));
        assert!(paths.summary.ends_with("summary.txt"));
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
    fn run_command_logged_kills_idle_child_even_with_heartbeat() {
        let temp = tempdir().expect("tempdir");
        let log_path = temp.path().join("flash.log");
        let spec = CommandSpec::new("python3").args([
            "-c",
            "import time\nprint('start', flush=True)\ntime.sleep(5)\nprint('done', flush=True)\n",
        ]);

        let status = run_command_logged(
            &spec,
            &log_path,
            CommandRunOptions {
                timeout: Some(Duration::from_secs(10)),
                progress_interval: Some(Duration::from_millis(200)),
                progress_label: Some("app-only flash"),
                idle_timeout: Some(Duration::from_secs(1)),
                progress_stall_timeout: None,
                progress_mode: None,
                log_drain_timeout: Duration::from_millis(200),
            },
        )
        .expect("run command");

        assert!(!status.success());
        let log = fs::read_to_string(&log_path).expect("read log");
        assert!(log.contains("start"));
        assert!(log.contains("app-only flash in progress"));
        assert!(log.contains("command output stalled after 1s without new child output"));
        assert!(!log.lines().any(|line| line == "done"));
    }

    #[test]
    fn run_command_logged_allows_child_that_keeps_emitting_output() {
        let temp = tempdir().expect("tempdir");
        let log_path = temp.path().join("flash.log");
        let spec = CommandSpec::new("python3").args([
            "-c",
            "import time\nfor i in range(5):\n    print(i, flush=True)\n    time.sleep(0.2)\n",
        ]);

        let status = run_command_logged(
            &spec,
            &log_path,
            CommandRunOptions {
                timeout: Some(Duration::from_secs(10)),
                progress_interval: Some(Duration::from_millis(200)),
                progress_label: Some("app-only flash"),
                idle_timeout: Some(Duration::from_secs(1)),
                progress_stall_timeout: None,
                progress_mode: None,
                log_drain_timeout: Duration::from_millis(200),
            },
        )
        .expect("run command");

        assert!(status.success());
        let log = fs::read_to_string(&log_path).expect("read log");
        assert!(log.contains("0"));
        assert!(log.contains("4"));
        assert!(!log.contains("command output stalled"));
    }

    #[test]
    fn run_command_logged_kills_child_when_esptool_write_progress_stalls() {
        let temp = tempdir().expect("tempdir");
        let log_path = temp.path().join("flash.log");
        let spec = CommandSpec::new("python3").args([
            "-c",
            "import time\nprint('Writing at 0x00010000... (0 %)', flush=True)\nfor _ in range(20):\n    print('still alive', flush=True)\n    time.sleep(0.2)\nprint('done', flush=True)\n",
        ]);

        let status = run_command_logged(
            &spec,
            &log_path,
            CommandRunOptions {
                timeout: Some(Duration::from_secs(10)),
                progress_interval: Some(Duration::from_millis(200)),
                progress_label: Some("app-only flash"),
                idle_timeout: Some(Duration::from_secs(5)),
                progress_stall_timeout: Some(Duration::from_secs(1)),
                progress_mode: Some(CommandProgressMode::EsptoolWriteFlash),
                log_drain_timeout: Duration::from_millis(200),
            },
        )
        .expect("run command");

        assert!(!status.success());
        let log = fs::read_to_string(&log_path).expect("read log");
        assert!(log.contains("Writing at 0x00010000... (0 %)"));
        assert!(log.contains("still alive"));
        assert!(
            log.contains("command progress stalled after 1s without esptool write advancement")
        );
        assert!(!log.lines().any(|line| line == "done"));
    }

    #[test]
    fn run_command_logged_allows_advancing_esptool_write_progress() {
        let temp = tempdir().expect("tempdir");
        let log_path = temp.path().join("flash.log");
        let spec = CommandSpec::new("python3").args([
            "-c",
            "import time\nfor marker in [\"Writing at 0x00010000... (0 %)\", \"Writing at 0x00011000... (1 %)\", \"Writing at 0x00012000... (2 %)\"]:\n    print(marker, flush=True)\n    time.sleep(0.2)\n",
        ]);

        let status = run_command_logged(
            &spec,
            &log_path,
            CommandRunOptions {
                timeout: Some(Duration::from_secs(10)),
                progress_interval: Some(Duration::from_millis(200)),
                progress_label: Some("app-only flash"),
                idle_timeout: Some(Duration::from_secs(5)),
                progress_stall_timeout: Some(Duration::from_secs(1)),
                progress_mode: Some(CommandProgressMode::EsptoolWriteFlash),
                log_drain_timeout: Duration::from_millis(200),
            },
        )
        .expect("run command");

        assert!(status.success());
        let log = fs::read_to_string(&log_path).expect("read log");
        assert!(log.contains("Writing at 0x00012000... (2 %)"));
        assert!(!log.contains("command progress stalled"));
    }

    #[test]
    fn run_command_logged_detaches_relay_threads_when_pipe_drain_lingers() {
        let temp = tempdir().expect("tempdir");
        let log_path = temp.path().join("flash.log");
        let spec = CommandSpec::new("python3").args([
            "-c",
            "import subprocess, sys\nsubprocess.Popen(['python3', '-c', 'import time; time.sleep(2)'], stdout=sys.stdout, stderr=sys.stderr)\nprint('child exiting', flush=True)\n",
        ]);

        let started = std::time::Instant::now();
        let status = run_command_logged(
            &spec,
            &log_path,
            CommandRunOptions {
                timeout: Some(Duration::from_secs(10)),
                progress_interval: None,
                progress_label: None,
                idle_timeout: None,
                progress_stall_timeout: None,
                progress_mode: None,
                log_drain_timeout: Duration::from_millis(200),
            },
        )
        .expect("run command");

        assert!(status.success());
        assert!(started.elapsed() < Duration::from_secs(2));
        let log = fs::read_to_string(&log_path).expect("read log");
        assert!(log.contains("child exiting"));
        assert!(log.contains("command log drain exceeded 200ms"));
    }

    #[test]
    fn flash_capture_workflow_yaml_parses() {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scenarios/flash-capture.sw.yaml");
        let workflow = load_workflow(&path).expect("load workflow");
        assert_eq!(workflow.document.name, "flash-capture");
    }
}
