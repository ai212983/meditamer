use crate::firmware::types::SD_PATH_MAX;

pub(super) fn parse_path_token(
    line: &[u8],
    start: usize,
) -> Option<([u8; SD_PATH_MAX], u8, usize)> {
    if start >= line.len() {
        return None;
    }
    let mut end = start;
    while end < line.len() && !line[end].is_ascii_whitespace() {
        end += 1;
    }
    if end == start {
        return None;
    }
    let token = &line[start..end];
    if token.len() > SD_PATH_MAX {
        return None;
    }
    let mut out = [0u8; SD_PATH_MAX];
    out[..token.len()].copy_from_slice(token);
    Some((out, token.len() as u8, end))
}

pub(super) fn trim_ascii_whitespace(line: &[u8]) -> &[u8] {
    let mut start = 0usize;
    let mut end = line.len();
    while start < end && line[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && line[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    &line[start..end]
}

pub(super) fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    (0..=haystack.len() - needle.len()).find(|&idx| &haystack[idx..idx + needle.len()] == needle)
}

pub(super) fn parse_u64_ascii(bytes: &[u8], mut i: usize) -> Option<(u64, usize)> {
    let mut value = 0u64;
    let start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        value = value
            .checked_mul(10)?
            .checked_add((bytes[i] - b'0') as u64)?;
        i += 1;
    }
    if i == start {
        None
    } else {
        Some((value, i))
    }
}

pub(super) fn parse_i32_ascii(bytes: &[u8], i: usize) -> Option<(i32, usize)> {
    if i >= bytes.len() {
        return None;
    }
    let mut idx = i;
    let mut sign = 1i64;
    if bytes[idx] == b'-' {
        sign = -1;
        idx += 1;
    } else if bytes[idx] == b'+' {
        idx += 1;
    }

    let (unsigned, next_idx) = parse_u64_ascii(bytes, idx)?;
    let signed = sign.checked_mul(unsigned as i64)?;
    if signed < i32::MIN as i64 || signed > i32::MAX as i64 {
        return None;
    }
    Some((signed as i32, next_idx))
}
