use super::{super::commands::SerialCommand, util::trim_ascii_whitespace};

pub(super) fn parse_firmware_command(line: &[u8]) -> Option<SerialCommand> {
    let line = trim_ascii_whitespace(line);
    match line {
        b"FWFACTORYBOOT" => Some(SerialCommand::FirmwareFactoryBoot),
        _ => None,
    }
}
