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
    assert!(log.contains(
        "command progress stalled after 1s without esptool write advancement"
    ));
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
