use super::{commands::SdWaitTarget, parser::SDWAIT_DEFAULT_TIMEOUT_MS, *};

#[cfg(feature = "asset-upload-http")]
use super::super::types::RuntimeMode;
use super::super::types::{AppEvent, SD_PATH_MAX, SD_WRITE_MAX};

fn path_from(buf: &[u8; SD_PATH_MAX], len: u8) -> &str {
    core::str::from_utf8(&buf[..len as usize]).unwrap()
}

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

#[cfg(feature = "asset-upload-http")]
#[test]
fn parses_runmode_upload() {
    let cmd = parse_serial_command(b"RUNMODE UPLOAD");
    match cmd {
        Some(SerialCommand::RunMode { mode }) => {
            assert!(matches!(mode, RuntimeMode::Upload));
        }
        _ => panic!("unexpected command"),
    }
}

#[cfg(feature = "asset-upload-http")]
#[test]
fn parses_runmode_normal_case_insensitive() {
    let cmd = parse_serial_command(b"runmode normal");
    match cmd {
        Some(SerialCommand::RunMode { mode }) => {
            assert!(matches!(mode, RuntimeMode::Normal));
        }
        _ => panic!("unexpected command"),
    }
}

#[cfg(feature = "asset-upload-http")]
#[test]
fn parses_wifiset_with_password() {
    let cmd = parse_serial_command(b"WIFISET MyNet pass1234");
    match cmd {
        Some(SerialCommand::WifiSet { credentials }) => {
            assert_eq!(&credentials.ssid[..credentials.ssid_len as usize], b"MyNet");
            assert_eq!(
                &credentials.password[..credentials.password_len as usize],
                b"pass1234"
            );
        }
        _ => panic!("unexpected command"),
    }
}

#[cfg(feature = "asset-upload-http")]
#[test]
fn parses_wifiset_open_network() {
    let cmd = parse_serial_command(b"WIFISET CafeWiFi");
    match cmd {
        Some(SerialCommand::WifiSet { credentials }) => {
            assert_eq!(
                &credentials.ssid[..credentials.ssid_len as usize],
                b"CafeWiFi"
            );
            assert_eq!(credentials.password_len, 0);
        }
        _ => panic!("unexpected command"),
    }
}

#[test]
fn parses_sdwait_defaults() {
    let cmd = parse_serial_command(b"SDWAIT");
    match cmd {
        Some(SerialCommand::SdWait { target, timeout_ms }) => {
            assert!(matches!(target, SdWaitTarget::Next));
            assert_eq!(timeout_ms, SDWAIT_DEFAULT_TIMEOUT_MS);
        }
        _ => panic!("unexpected command"),
    }
}

#[test]
fn parses_sdwait_last_with_timeout() {
    let cmd = parse_serial_command(b"SDWAIT LAST 2500");
    match cmd {
        Some(SerialCommand::SdWait { target, timeout_ms }) => {
            assert!(matches!(target, SdWaitTarget::Last));
            assert_eq!(timeout_ms, 2500);
        }
        _ => panic!("unexpected command"),
    }
}

#[test]
fn parses_sdwait_by_id() {
    let cmd = parse_serial_command(b"SDWAIT 42");
    match cmd {
        Some(SerialCommand::SdWait { target, timeout_ms }) => {
            match target {
                SdWaitTarget::Id(id) => assert_eq!(id, 42),
                _ => panic!("unexpected target"),
            }
            assert_eq!(timeout_ms, SDWAIT_DEFAULT_TIMEOUT_MS);
        }
        _ => panic!("unexpected command"),
    }
}

#[test]
fn rejects_sdwait_invalid_trailing_tokens() {
    let cmd = parse_serial_command(b"SDWAIT 42 100 extra");
    assert!(cmd.is_none());
}

#[test]
fn rejects_oversized_sdfatwrite_payload() {
    let mut line = heapless::Vec::<u8, 512>::new();
    line.extend_from_slice(b"SDFATWRITE /notes/big.txt ")
        .expect("prefix");
    for _ in 0..(SD_WRITE_MAX + 1) {
        line.push(b'x').expect("payload");
    }
    let cmd = parse_serial_command(&line);
    assert!(cmd.is_none());
}

#[test]
fn parses_sdfattrunc() {
    let cmd = parse_serial_command(b"SDFATTRUNC /notes/log.txt 1024");
    match cmd {
        Some(SerialCommand::FatTruncate {
            path,
            path_len,
            size,
        }) => {
            assert_eq!(path_from(&path, path_len), "/notes/log.txt");
            assert_eq!(size, 1024);
        }
        _ => panic!("unexpected command"),
    }
}

#[test]
fn rejects_bad_sdfatrename() {
    let cmd = parse_serial_command(b"SDFATREN /only_one_arg");
    assert!(cmd.is_none());
}

#[test]
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
