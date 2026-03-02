#[cfg(feature = "asset-upload-http")]
#[test]
fn parses_netcfg_set_with_password() {
    let cmd = parse_serial_command(
        br#"NETCFG SET {"ssid":"MyNet","password":"pass1234","dhcp_timeout_ms":25000}"#,
    );
    match cmd {
        Some(SerialCommand::NetCfgSet { config }) => {
            let credentials = config.credentials.expect("credentials");
            assert_eq!(&credentials.ssid[..credentials.ssid_len as usize], b"MyNet");
            assert_eq!(
                &credentials.password[..credentials.password_len as usize],
                b"pass1234"
            );
            assert_eq!(config.policy.dhcp_timeout_ms, 25_000);
        }
        _ => panic!("unexpected command"),
    }
}

#[cfg(feature = "asset-upload-http")]
#[test]
fn parses_netcfg_set_open_network() {
    let cmd = parse_serial_command(br#"NETCFG SET {"ssid":"CafeWiFi"}"#);
    match cmd {
        Some(SerialCommand::NetCfgSet { config }) => {
            let credentials = config.credentials.expect("credentials");
            assert_eq!(
                &credentials.ssid[..credentials.ssid_len as usize],
                b"CafeWiFi"
            );
            assert_eq!(credentials.password_len, 0);
        }
        _ => panic!("unexpected command"),
    }
}

#[cfg(feature = "asset-upload-http")]
#[test]
fn parses_net_control_commands() {
    assert!(matches!(
        parse_serial_command(b"NET START"),
        Some(SerialCommand::NetStart)
    ));
    assert!(matches!(
        parse_serial_command(b"NET STOP"),
        Some(SerialCommand::NetStop)
    ));
    assert!(matches!(
        parse_serial_command(b"NET STATUS"),
        Some(SerialCommand::NetStatus)
    ));
    assert!(matches!(
        parse_serial_command(b"NET RECOVER"),
        Some(SerialCommand::NetRecover)
    ));
    assert!(matches!(
        parse_serial_command(b"NET LISTENER ON"),
        Some(SerialCommand::NetListenerSet { enabled: true })
    ));
    assert!(matches!(
        parse_serial_command(b"NET LISTENER OFF"),
        Some(SerialCommand::NetListenerSet { enabled: false })
    ));
    assert!(matches!(
        parse_serial_command(b"NETCFG GET"),
        Some(SerialCommand::NetCfgGet)
    ));
}
