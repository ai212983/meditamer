use super::*;

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
