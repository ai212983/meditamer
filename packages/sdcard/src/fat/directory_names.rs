use crate::fat::SdFatError;

pub(super) fn encode_short_name(segment: &[u8]) -> Result<[u8; 11], SdFatError> {
    if segment == b"." || segment == b".." {
        let mut out = [b' '; 11];
        out[0] = b'.';
        if segment.len() == 2 {
            out[1] = b'.';
        }
        return Ok(out);
    }

    let mut out = [b' '; 11];
    let dot = segment.iter().position(|byte| *byte == b'.');
    let (name, extension) = match dot {
        Some(index) => {
            let extension = &segment[index + 1..];
            if extension.contains(&b'.') {
                return Err(SdFatError::InvalidShortName);
            }
            (&segment[..index], extension)
        }
        None => (segment, &[][..]),
    };
    if name.is_empty() || name.len() > 8 || extension.len() > 3 {
        return Err(SdFatError::InvalidShortName);
    }
    for (index, byte) in name.iter().enumerate() {
        out[index] = normalize_short_char(*byte)?;
    }
    for (index, byte) in extension.iter().enumerate() {
        out[8 + index] = normalize_short_char(*byte)?;
    }
    Ok(out)
}

fn normalize_short_char(byte: u8) -> Result<u8, SdFatError> {
    let upper = byte.to_ascii_uppercase();
    if upper.is_ascii_alphanumeric() || matches!(upper, b'_' | b'-' | b'$' | b'~') {
        Ok(upper)
    } else {
        Err(SdFatError::InvalidShortName)
    }
}

pub(super) fn make_short_alias(name: &[u8], attempt: u32) -> [u8; 11] {
    let mut out = [b' '; 11];
    let (base, extension) = split_name_parts(name);
    for (index, byte) in extension.iter().take(3).enumerate() {
        out[8 + index] = normalize_short_char(*byte).unwrap_or(b'_');
    }

    let (digits, digits_len) = suffix_digits(attempt.max(1));
    let max_base = 8usize.saturating_sub(1 + digits_len);
    let mut base_len = 0;
    for byte in base.iter().take(max_base) {
        out[base_len] = normalize_short_char(*byte).unwrap_or(b'_');
        base_len += 1;
    }
    if base_len == 0 {
        let fallback = b"FILE";
        let count = fallback.len().min(max_base);
        out[..count].copy_from_slice(&fallback[..count]);
        base_len = count;
    }
    if base_len < 8 {
        out[base_len] = b'~';
        base_len += 1;
        for index in 0..digits_len {
            if base_len >= 8 {
                break;
            }
            out[base_len] = digits[digits_len - 1 - index];
            base_len += 1;
        }
    }
    out
}

fn split_name_parts(name: &[u8]) -> (&[u8], &[u8]) {
    match name.iter().rposition(|byte| *byte == b'.') {
        Some(index) => (&name[..index], &name[index + 1..]),
        None => (name, &[]),
    }
}

fn suffix_digits(mut value: u32) -> ([u8; 10], usize) {
    let mut digits = [0u8; 10];
    let mut len = 0;
    while value > 0 {
        digits[len] = b'0' + (value % 10) as u8;
        len += 1;
        value /= 10;
    }
    (digits, len)
}

pub(super) fn short_name_to_text(raw: &[u8; 11], out: &mut [u8]) -> usize {
    let mut len = 0;
    append_short_component(&raw[..8], out, &mut len);
    if raw[8..].iter().any(|byte| *byte != b' ') && append_short_char(out, &mut len, b'.') {
        append_short_component(&raw[8..], out, &mut len);
    }
    len
}

fn append_short_component(raw: &[u8], out: &mut [u8], len: &mut usize) {
    for byte in raw {
        if *byte == b' ' || !append_short_char(out, len, *byte) {
            break;
        }
    }
}

fn append_short_char(out: &mut [u8], len: &mut usize, byte: u8) -> bool {
    if *len >= out.len() {
        return false;
    }
    out[*len] = byte;
    *len += 1;
    true
}
