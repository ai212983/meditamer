fn maps_timeset_to_event_and_responses() {
    let cmd = parse_serial_command(b"TIMESET 1762531200 -300").expect("command");
    let (app_event, sd_command, ok, busy) = serial_command_event_and_responses(cmd);
    assert!(sd_command.is_none());
    match app_event {
        Some(AppEvent::TimeSync(sync)) => {
            assert_eq!(sync.unix_epoch_utc_seconds, 1_762_531_200);
            assert_eq!(sync.tz_offset_minutes, -300);
        }
        _ => panic!("expected timesync event"),
    };
    assert_eq!(ok, b"TIMESET OK\r\n");
    assert_eq!(busy, b"TIMESET BUSY\r\n");
}

#[test]
fn maps_sdfatstat_to_event_and_responses() {
    let cmd = parse_serial_command(b"SDFATSTAT /notes/TODO.txt").expect("command");
    let (app_event, sd_command, ok, busy) = serial_command_event_and_responses(cmd);
    assert!(app_event.is_none());
    match sd_command {
        Some(SdCommand::FatStat { path, path_len }) => {
            assert_eq!(path_from(&path, path_len), "/notes/TODO.txt");
        }
        _ => panic!("expected sdfat stat event"),
    };
    assert_eq!(ok, b"SDFATSTAT OK\r\n");
    assert_eq!(busy, b"SDFATSTAT BUSY\r\n");
}

#[test]
fn maps_sdfatren_to_event_and_responses() {
    let cmd = parse_serial_command(b"SDFATREN /a.txt /b.txt").expect("command");
    let (app_event, sd_command, ok, busy) = serial_command_event_and_responses(cmd);
    assert!(app_event.is_none());
    match sd_command {
        Some(SdCommand::FatRename {
            src_path,
            src_path_len,
            dst_path,
            dst_path_len,
        }) => {
            assert_eq!(path_from(&src_path, src_path_len), "/a.txt");
            assert_eq!(path_from(&dst_path, dst_path_len), "/b.txt");
        }
        _ => panic!("expected sdfat rename event"),
    };
    assert_eq!(ok, b"SDFATREN OK\r\n");
    assert_eq!(busy, b"SDFATREN BUSY\r\n");
}

#[test]
fn maps_state_set_to_app_state_command() {
    let cmd = parse_serial_command(b"STATE SET upload=on").expect("command");
    let state_cmd = app_state_command_for_serial(cmd).expect("app-state command");
    assert!(matches!(state_cmd, AppStateCommand::SetUpload(true)));
}

#[test]
fn maps_state_diag_to_app_state_command() {
    let cmd = parse_serial_command(b"STATE DIAG kind=DEBUG targets=IMU").expect("command");
    let state_cmd = app_state_command_for_serial(cmd).expect("app-state command");
    match state_cmd {
        AppStateCommand::SetDiag { kind, targets } => {
            assert!(matches!(kind, DiagKind::Debug));
            assert_eq!(targets.as_persisted(), 1 << 4);
        }
        _ => panic!("expected state diag command"),
    }
}
