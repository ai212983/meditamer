use super::{super::commands::SerialCommand, util::trim_ascii_whitespace};
use crate::firmware::update::LEGACY_CHUNK_MAX;

pub(super) fn parse_firmware_command(line: &[u8]) -> Option<SerialCommand> {
    let line = trim_ascii_whitespace(line);
    match line {
        b"FWSTATUS" => return Some(SerialCommand::FirmwareStatus),
        b"FWPREPARE" => return Some(SerialCommand::FirmwarePrepare),
        b"FWFINISH" => return Some(SerialCommand::FirmwareFinish),
        b"FWACTIVATE" => return Some(SerialCommand::FirmwareActivate),
        b"FWABORT" => return Some(SerialCommand::FirmwareAbort),
        _ => {}
    }
    if let Some(args) = line.strip_prefix(b"FWBEGIN ") {
        return parse_begin(args);
    }
    if let Some(args) = line.strip_prefix(b"FWCHUNK ") {
        return parse_chunk(args);
    }
    if let Some(args) = line.strip_prefix(b"FWSTREAM ") {
        let baud = parse_u32(trim_ascii_whitespace(args))?;
        return Some(SerialCommand::FirmwareStream { baud });
    }
    None
}

fn parse_begin(args: &[u8]) -> Option<SerialCommand> {
    let mut parts = args.split(|byte| byte.is_ascii_whitespace());
    let image_len = parse_u32(parts.next()?)?;
    let digest = decode_hex_array::<32>(parts.next()?)?;
    let signature = decode_hex_array::<64>(parts.next()?)?;
    if parts.any(|part| !part.is_empty()) {
        return None;
    }
    Some(SerialCommand::FirmwareBegin {
        image_len,
        digest,
        signature,
    })
}

fn parse_chunk(args: &[u8]) -> Option<SerialCommand> {
    let split = args.iter().position(|byte| byte.is_ascii_whitespace())?;
    let offset = parse_u32(&args[..split])?;
    let encoded = trim_ascii_whitespace(&args[split..]);
    if encoded.is_empty()
        || encoded.len() > LEGACY_CHUNK_MAX * 2
        || !encoded.len().is_multiple_of(8)
    {
        return None;
    }
    let mut bytes = [0u8; LEGACY_CHUNK_MAX];
    let len = encoded.len() / 2;
    decode_hex_into(encoded, &mut bytes[..len])?;
    Some(SerialCommand::FirmwareChunk {
        offset,
        bytes,
        len: len as u16,
    })
}

fn parse_u32(value: &[u8]) -> Option<u32> {
    if value.is_empty() {
        return None;
    }
    value.iter().try_fold(0u32, |acc, byte| {
        let digit = byte.checked_sub(b'0')?;
        if digit > 9 {
            return None;
        }
        acc.checked_mul(10)?.checked_add(digit as u32)
    })
}

fn decode_hex_array<const N: usize>(encoded: &[u8]) -> Option<[u8; N]> {
    if encoded.len() != N * 2 {
        return None;
    }
    let mut decoded = [0u8; N];
    decode_hex_into(encoded, &mut decoded)?;
    Some(decoded)
}

fn decode_hex_into(encoded: &[u8], decoded: &mut [u8]) -> Option<()> {
    if encoded.len() != decoded.len() * 2 {
        return None;
    }
    for (pair, output) in encoded.chunks_exact(2).zip(decoded.iter_mut()) {
        *output = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
    }
    Some(())
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(all(test, not(target_os = "none")))]
mod tests {
    use super::*;

    #[test]
    fn parses_firmware_chunk() {
        let command = parse_firmware_command(b"FWCHUNK 256 00112233").expect("command");
        match command {
            SerialCommand::FirmwareChunk { offset, bytes, len } => {
                assert_eq!(offset, 256);
                assert_eq!(len, 4);
                assert_eq!(&bytes[..4], &[0x00, 0x11, 0x22, 0x33]);
            }
            _ => panic!("wrong command"),
        }
    }
}
