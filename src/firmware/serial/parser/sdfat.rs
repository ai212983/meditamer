use crate::firmware::types::{SD_PATH_MAX, SD_WRITE_MAX};

use super::util::{parse_path_token, parse_u64_ascii, trim_ascii_whitespace};

pub(super) fn parse_sdfatls_command(line: &[u8]) -> Option<([u8; SD_PATH_MAX], u8)> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"SDFATLS";
    if !trimmed.starts_with(cmd) {
        return None;
    }
    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i == trimmed.len() {
        let mut path = [0u8; SD_PATH_MAX];
        path[0] = b'/';
        return Some((path, 1));
    }
    let (path, path_len, next_i) = parse_path_token(trimmed, i)?;
    let mut j = next_i;
    while j < trimmed.len() && trimmed[j].is_ascii_whitespace() {
        j += 1;
    }
    if j != trimmed.len() {
        return None;
    }
    Some((path, path_len))
}

pub(super) fn parse_sdfatread_command(line: &[u8]) -> Option<([u8; SD_PATH_MAX], u8)> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"SDFATREAD";
    if !trimmed.starts_with(cmd) {
        return None;
    }
    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let (path, path_len, next_i) = parse_path_token(trimmed, i)?;
    let mut j = next_i;
    while j < trimmed.len() && trimmed[j].is_ascii_whitespace() {
        j += 1;
    }
    if j != trimmed.len() {
        return None;
    }
    Some((path, path_len))
}

pub(super) fn parse_sdfatwrite_command(
    line: &[u8],
) -> Option<([u8; SD_PATH_MAX], u8, [u8; SD_WRITE_MAX], u16)> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"SDFATWRITE";
    if !trimmed.starts_with(cmd) {
        return None;
    }
    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let (path, path_len, next_i) = parse_path_token(trimmed, i)?;
    i = next_i;
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i > trimmed.len() {
        return None;
    }

    let payload = &trimmed[i..];
    if payload.len() > SD_WRITE_MAX {
        return None;
    }

    let mut data = [0u8; SD_WRITE_MAX];
    data[..payload.len()].copy_from_slice(payload);
    Some((path, path_len, data, payload.len() as u16))
}

pub(super) fn parse_sdfatstat_command(line: &[u8]) -> Option<([u8; SD_PATH_MAX], u8)> {
    parse_single_path_command(line, b"SDFATSTAT")
}

pub(super) fn parse_sdfatmkdir_command(line: &[u8]) -> Option<([u8; SD_PATH_MAX], u8)> {
    parse_single_path_command(line, b"SDFATMKDIR")
}

pub(super) fn parse_sdfatrm_command(line: &[u8]) -> Option<([u8; SD_PATH_MAX], u8)> {
    parse_single_path_command(line, b"SDFATRM")
}

pub(super) fn parse_sdfatren_command(
    line: &[u8],
) -> Option<([u8; SD_PATH_MAX], u8, [u8; SD_PATH_MAX], u8)> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"SDFATREN";
    if !trimmed.starts_with(cmd) {
        return None;
    }

    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let (src_path, src_path_len, next_i) = parse_path_token(trimmed, i)?;
    i = next_i;
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let (dst_path, dst_path_len, next_i) = parse_path_token(trimmed, i)?;
    i = next_i;
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i != trimmed.len() {
        return None;
    }

    Some((src_path, src_path_len, dst_path, dst_path_len))
}

pub(super) fn parse_sdfatappend_command(
    line: &[u8],
) -> Option<([u8; SD_PATH_MAX], u8, [u8; SD_WRITE_MAX], u16)> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"SDFATAPPEND";
    if !trimmed.starts_with(cmd) {
        return None;
    }

    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let (path, path_len, next_i) = parse_path_token(trimmed, i)?;
    i = next_i;
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i > trimmed.len() {
        return None;
    }

    let payload = &trimmed[i..];
    if payload.len() > SD_WRITE_MAX {
        return None;
    }

    let mut data = [0u8; SD_WRITE_MAX];
    data[..payload.len()].copy_from_slice(payload);
    Some((path, path_len, data, payload.len() as u16))
}

pub(super) fn parse_sdfattrunc_command(line: &[u8]) -> Option<([u8; SD_PATH_MAX], u8, u32)> {
    let trimmed = trim_ascii_whitespace(line);
    let cmd = b"SDFATTRUNC";
    if !trimmed.starts_with(cmd) {
        return None;
    }
    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let (path, path_len, next_i) = parse_path_token(trimmed, i)?;
    i = next_i;
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let (size, next_i) = parse_u64_ascii(trimmed, i)?;
    if size > u32::MAX as u64 {
        return None;
    }
    i = next_i;
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i != trimmed.len() {
        return None;
    }

    Some((path, path_len, size as u32))
}

fn parse_single_path_command(line: &[u8], cmd: &[u8]) -> Option<([u8; SD_PATH_MAX], u8)> {
    let trimmed = trim_ascii_whitespace(line);
    if !trimmed.starts_with(cmd) {
        return None;
    }
    let mut i = cmd.len();
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    let (path, path_len, next_i) = parse_path_token(trimmed, i)?;
    i = next_i;
    while i < trimmed.len() && trimmed[i].is_ascii_whitespace() {
        i += 1;
    }
    if i != trimmed.len() {
        return None;
    }
    Some((path, path_len))
}
