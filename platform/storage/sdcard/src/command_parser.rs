use crate::request::StorageCommand;
use crate::{SD_PATH_MAX, SD_WRITE_MAX};

pub fn parse(line: &[u8]) -> Option<StorageCommand> {
    if let Some((path, path_len)) = parse_sdfatls_command(line) {
        return Some(StorageCommand::FatList { path, path_len });
    }
    if let Some((path, path_len)) = parse_sdfatread_command(line) {
        return Some(StorageCommand::FatRead { path, path_len });
    }
    if let Some((path, path_len, data, data_len)) = parse_sdfatwrite_command(line) {
        return Some(StorageCommand::FatWrite {
            path,
            path_len,
            data,
            data_len,
        });
    }
    if let Some((path, path_len)) = parse_sdfatstat_command(line) {
        return Some(StorageCommand::FatStat { path, path_len });
    }
    if let Some((path, path_len)) = parse_sdfatmkdir_command(line) {
        return Some(StorageCommand::FatMkdir { path, path_len });
    }
    if let Some((path, path_len)) = parse_sdfatrm_command(line) {
        return Some(StorageCommand::FatRemove { path, path_len });
    }
    if let Some((src_path, src_path_len, dst_path, dst_path_len)) = parse_sdfatren_command(line) {
        return Some(StorageCommand::FatRename {
            src_path,
            src_path_len,
            dst_path,
            dst_path_len,
        });
    }
    if let Some((path, path_len, data, data_len)) = parse_sdfatappend_command(line) {
        return Some(StorageCommand::FatAppend {
            path,
            path_len,
            data,
            data_len,
        });
    }
    if let Some((path, path_len, size)) = parse_sdfattrunc_command(line) {
        return Some(StorageCommand::FatTruncate {
            path,
            path_len,
            size,
        });
    }
    None
}

pub fn parse_sdfatls_command(line: &[u8]) -> Option<([u8; SD_PATH_MAX], u8)> {
    parse_single_path_or_root(line, b"SDFATLS", true)
}
pub fn parse_sdfatread_command(line: &[u8]) -> Option<([u8; SD_PATH_MAX], u8)> {
    parse_single_path_or_root(line, b"SDFATREAD", false)
}
pub fn parse_sdfatstat_command(line: &[u8]) -> Option<([u8; SD_PATH_MAX], u8)> {
    parse_single_path_or_root(line, b"SDFATSTAT", false)
}
pub fn parse_sdfatmkdir_command(line: &[u8]) -> Option<([u8; SD_PATH_MAX], u8)> {
    parse_single_path_or_root(line, b"SDFATMKDIR", false)
}
pub fn parse_sdfatrm_command(line: &[u8]) -> Option<([u8; SD_PATH_MAX], u8)> {
    parse_single_path_or_root(line, b"SDFATRM", false)
}

pub fn parse_sdfatwrite_command(
    line: &[u8],
) -> Option<([u8; SD_PATH_MAX], u8, [u8; SD_WRITE_MAX], u16)> {
    parse_payload(line, b"SDFATWRITE")
}
pub fn parse_sdfatappend_command(
    line: &[u8],
) -> Option<([u8; SD_PATH_MAX], u8, [u8; SD_WRITE_MAX], u16)> {
    parse_payload(line, b"SDFATAPPEND")
}
pub fn parse_sdfatren_command(
    line: &[u8],
) -> Option<([u8; SD_PATH_MAX], u8, [u8; SD_PATH_MAX], u8)> {
    let t = trim_ascii_whitespace(line);
    if !t.starts_with(b"SDFATREN") {
        return None;
    }
    if t.len() > 8 && !t[8].is_ascii_whitespace() {
        return None;
    }
    let (a, al, i) = parse_path_token(t, skip_ws(t, 8))?;
    let (b, bl, j) = parse_path_token(t, skip_ws(t, i))?;
    (skip_ws(t, j) == t.len()).then_some((a, al, b, bl))
}
pub fn parse_sdfattrunc_command(line: &[u8]) -> Option<([u8; SD_PATH_MAX], u8, u32)> {
    let t = trim_ascii_whitespace(line);
    if !t.starts_with(b"SDFATTRUNC") {
        return None;
    }
    if t.len() > 10 && !t[10].is_ascii_whitespace() {
        return None;
    }
    let (path, len, i) = parse_path_token(t, skip_ws(t, 10))?;
    let (size, j) = parse_u64_ascii(t, skip_ws(t, i))?;
    (size <= u32::MAX as u64 && skip_ws(t, j) == t.len()).then_some((path, len, size as u32))
}

fn parse_single_path_or_root(
    line: &[u8],
    cmd: &[u8],
    root: bool,
) -> Option<([u8; SD_PATH_MAX], u8)> {
    let t = trim_ascii_whitespace(line);
    if !t.starts_with(cmd) {
        return None;
    }
    if t.len() > cmd.len() && !t[cmd.len()].is_ascii_whitespace() {
        return None;
    }
    let i = skip_ws(t, cmd.len());
    if i == t.len() && root {
        let mut p = [0; SD_PATH_MAX];
        p[0] = b'/';
        return Some((p, 1));
    }
    let (p, l, j) = parse_path_token(t, i)?;
    (skip_ws(t, j) == t.len()).then_some((p, l))
}
fn parse_payload(
    line: &[u8],
    cmd: &[u8],
) -> Option<([u8; SD_PATH_MAX], u8, [u8; SD_WRITE_MAX], u16)> {
    let t = trim_ascii_whitespace(line);
    if !t.starts_with(cmd) {
        return None;
    }
    if t.len() > cmd.len() && !t[cmd.len()].is_ascii_whitespace() {
        return None;
    }
    let (path, len, i) = parse_path_token(t, skip_ws(t, cmd.len()))?;
    let payload = &t[skip_ws(t, i)..];
    if payload.len() > SD_WRITE_MAX {
        return None;
    }
    let mut data = [0; SD_WRITE_MAX];
    data[..payload.len()].copy_from_slice(payload);
    Some((path, len, data, payload.len() as u16))
}
pub fn parse_path_token(line: &[u8], start: usize) -> Option<([u8; SD_PATH_MAX], u8, usize)> {
    if start >= line.len() {
        return None;
    }
    let mut end = start;
    while end < line.len() && !line[end].is_ascii_whitespace() {
        end += 1;
    }
    if end == start || end - start > SD_PATH_MAX {
        return None;
    }
    let mut out = [0; SD_PATH_MAX];
    out[..end - start].copy_from_slice(&line[start..end]);
    Some((out, (end - start) as u8, end))
}
pub fn trim_ascii_whitespace(line: &[u8]) -> &[u8] {
    let mut a = 0;
    let mut b = line.len();
    while a < b && line[a].is_ascii_whitespace() {
        a += 1;
    }
    while b > a && line[b - 1].is_ascii_whitespace() {
        b -= 1;
    }
    &line[a..b]
}
pub fn parse_u64_ascii(bytes: &[u8], mut i: usize) -> Option<(u64, usize)> {
    let start = i;
    let mut v: u64 = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        v = v.checked_mul(10)?.checked_add((bytes[i] - b'0') as u64)?;
        i += 1;
    }
    (i != start).then_some((v, i))
}
fn skip_ws(line: &[u8], mut i: usize) -> usize {
    while i < line.len() && line[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_command_prefix_collisions() {
        assert!(parse_sdfatread_command(b"SDFATREADfoo").is_none());
        assert!(parse(b"SDFATWRITEfoo data").is_none());
    }

    #[test]
    fn preserves_payload_spaces_and_accepts_maximum_sizes() {
        let path = [b'p'; SD_PATH_MAX];
        let line = [&b"SDFATWRITE "[..], &path[..], b"  a  b  "].concat();
        let (_, _, data, len) = parse_sdfatwrite_command(&line).unwrap();
        assert_eq!(&data[..len as usize], b"a  b");

        let mut line = b"SDFATWRITE / ".to_vec();
        line.extend([b'x'; SD_WRITE_MAX]);
        assert!(parse_sdfatwrite_command(&line).is_some());
    }

    #[test]
    fn rejects_oversized_path_and_malformed_numeric() {
        let mut line = b"SDFATREAD /".to_vec();
        line.extend([b'p'; SD_PATH_MAX]);
        assert!(parse_sdfatread_command(&line).is_none());
        assert!(parse_sdfattrunc_command(b"SDFATTRUNC /file nope").is_none());
        assert!(parse_sdfattrunc_command(b"SDFATTRUNC /file 4294967296").is_none());
    }
}
