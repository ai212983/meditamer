#[test]
fn parses_sdfatstat() {
    let cmd = parse_serial_command(b"SDFATSTAT /notes/TODO.txt");
    match cmd {
        Some(SerialCommand::FatStat { path, path_len }) => {
            assert_eq!(path_from(&path, path_len), "/notes/TODO.txt");
        }
        _ => panic!("unexpected command"),
    }
}

#[test]
fn parses_sdfatmkdir() {
    let cmd = parse_serial_command(b"SDFATMKDIR /logs");
    match cmd {
        Some(SerialCommand::FatMkdir { path, path_len }) => {
            assert_eq!(path_from(&path, path_len), "/logs");
        }
        _ => panic!("unexpected command"),
    }
}

#[test]
fn parses_sdfatrename() {
    let cmd = parse_serial_command(b"SDFATREN /old/name.txt /new/name.txt");
    match cmd {
        Some(SerialCommand::FatRename {
            src_path,
            src_path_len,
            dst_path,
            dst_path_len,
        }) => {
            assert_eq!(path_from(&src_path, src_path_len), "/old/name.txt");
            assert_eq!(path_from(&dst_path, dst_path_len), "/new/name.txt");
        }
        _ => panic!("unexpected command"),
    }
}

#[test]
fn parses_sdfatappend() {
    let cmd = parse_serial_command(b"SDFATAPPEND /notes/log.txt hello");
    match cmd {
        Some(SerialCommand::FatAppend {
            path,
            path_len,
            data,
            data_len,
        }) => {
            assert_eq!(path_from(&path, path_len), "/notes/log.txt");
            assert_eq!(&data[..data_len as usize], b"hello");
        }
        _ => panic!("unexpected command"),
    }
}

#[test]
fn parses_sdrwverify() {
    let cmd = parse_serial_command(b"SDRWVERIFY 2048");
    match cmd {
        Some(SerialCommand::RwVerify { lba }) => assert_eq!(lba, 2048),
        _ => panic!("unexpected command"),
    }
}

#[test]
fn parses_psram_allocator_status_command() {
    let cmd = parse_serial_command(b"PSRAM");
    assert!(matches!(cmd, Some(SerialCommand::AllocatorStatus)));
}

#[test]
fn parses_ping_command() {
    let cmd = parse_serial_command(b"PING");
    assert!(matches!(cmd, Some(SerialCommand::Ping)));
}

#[test]
fn parses_touch_scheduler_reset_command() {
    let cmd = parse_serial_command(b"TOUCHSCHEDRESET");
    assert!(matches!(cmd, Some(SerialCommand::TouchSchedReset)));
}

#[test]
fn parses_heap_allocator_status_alias() {
    let cmd = parse_serial_command(b"HEAP");
    assert!(matches!(cmd, Some(SerialCommand::AllocatorStatus)));
}

#[test]
fn parses_psram_alloc_probe_command() {
    let cmd = parse_serial_command(b"PSRAMALLOC 4096");
    match cmd {
        Some(SerialCommand::AllocatorAllocProbe { bytes }) => assert_eq!(bytes, 4096),
        _ => panic!("unexpected command"),
    }
}

#[test]
fn rejects_psram_alloc_probe_without_size() {
    let cmd = parse_serial_command(b"PSRAMALLOC");
    assert!(cmd.is_none());
}

#[test]
fn parses_state_get() {
    let cmd = parse_serial_command(b"STATE GET");
    assert!(matches!(cmd, Some(SerialCommand::StateGet)));
}

#[test]
fn rejects_removed_timeset_command() {
    assert!(parse_serial_command(b"TIMESET 1762531200 -300").is_none());
    assert!(parse_serial_command(b"TIMESET").is_none());
}

#[test]
fn parses_state_status_alias() {
    let cmd = parse_serial_command(b"STATE STATUS");
    assert!(matches!(cmd, Some(SerialCommand::StateGet)));
}

#[test]
fn parses_diag_get() {
    let cmd = parse_serial_command(b"DIAG GET");
    assert!(matches!(cmd, Some(SerialCommand::DiagGet)));
}

#[test]
fn parses_telemetry_status() {
    let cmd = parse_serial_command(b"TELEM STATUS");
    assert!(matches!(cmd, Some(SerialCommand::TelemetryStatus)));
}

#[test]
fn parses_telemetry_set_domain_on() {
    let cmd = parse_serial_command(b"TELEMSET WIFI ON");
    match cmd {
        Some(SerialCommand::TelemetrySet { operation }) => {
            assert!(matches!(
                operation,
                TelemetrySetOperation::Domain {
                    domain: TelemetryDomain::Wifi,
                    enabled: true
                }
            ));
        }
        _ => panic!("unexpected command"),
    }
}

#[test]
fn parses_telemetry_set_all_off() {
    let cmd = parse_serial_command(b"TELEMSET ALL OFF");
    match cmd {
        Some(SerialCommand::TelemetrySet { operation }) => {
            assert!(matches!(
                operation,
                TelemetrySetOperation::All { enabled: false }
            ));
        }
        _ => panic!("unexpected command"),
    }
}

#[test]
fn parses_state_set_upload_on() {
    let cmd = parse_serial_command(b"STATE SET upload=on");
    match cmd {
        Some(SerialCommand::StateSet { operation }) => {
            assert!(matches!(operation, StateSetOperation::Upload(true)));
        }
        _ => panic!("unexpected command"),
    }
}

#[test]
fn parses_state_diag() {
    let cmd = parse_serial_command(b"STATE DIAG kind=TEST targets=SD|WIFI|TOUCH");
    match cmd {
        Some(SerialCommand::StateDiag { kind, targets }) => {
            assert!(matches!(kind, DiagKind::Test));
            assert_eq!(targets.as_persisted(), 0b01011);
        }
        _ => panic!("unexpected command"),
    }
}

#[test]
fn rejects_invalid_state_commands() {
    assert!(parse_serial_command(b"STATE SET base=DAY").is_none());
    assert!(parse_serial_command(b"STATE SET day_bg=PORTRAIT").is_none());
    assert!(parse_serial_command(b"STATE SET overlay=CLOCK").is_none());
    assert!(parse_serial_command(b"STATE SET assets=off").is_none());
    assert!(parse_serial_command(b"STATE DIAG kind=TEST targets=GPS").is_none());
}
