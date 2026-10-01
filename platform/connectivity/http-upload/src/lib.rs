#![no_std]

//! Shared streaming request engine for the asset upload HTTP service.
//!
//! Socket lifetime, DHCP/listener gating, storage ownership, and product
//! telemetry remain with the target. This crate owns authentication, routing,
//! body draining and the pipelined SD upload stream, using borrowed buffers.

#[cfg(test)]
extern crate std;
mod host;
pub use host::*;
mod connection;
mod helpers;
pub use connection::{
    handle_connection, HandledRequest, RequestRouteKind, HTTP_HEADER_KEEPALIVE_IDLE_TIMEOUT_MS,
    HTTP_HEADER_READ_TIMEOUT_MS,
};
pub const HTTP_SOCKET_TIMEOUT_SECS: u64 = 60;
pub const HTTP_HEADER_MAX: usize = 1024;

use sdcard::SD_PATH_MAX;
const ROOT: &[u8] = b"/assets";
const TOKEN_HEADER: &[u8] = b"x-upload-token";
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ParseError {
    InvalidPath,
    PathTooLong,
    InvalidPercentEncoding,
}
struct Path {
    bytes: [u8; SD_PATH_MAX],
    len: u8,
}
impl Path {
    const fn empty() -> Self {
        Self {
            bytes: [0; SD_PATH_MAX],
            len: 0,
        }
    }
    fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}
fn path_within_root(path: &Path) -> bool {
    path.as_bytes() == ROOT
        || (path.as_bytes().starts_with(ROOT) && path.as_bytes().get(ROOT.len()) == Some(&b'/'))
}

fn decode_path(encoded: &[u8]) -> Result<Path, ParseError> {
    let mut path = Path::empty();
    let mut i = 0;
    while i < encoded.len() {
        let value = match encoded[i] {
            b'%' if i + 2 < encoded.len() => {
                let high = hex(encoded[i + 1]).ok_or(ParseError::InvalidPercentEncoding)?;
                let low = hex(encoded[i + 2]).ok_or(ParseError::InvalidPercentEncoding)?;
                i += 3;
                (high << 4) | low
            }
            b'%' => return Err(ParseError::InvalidPercentEncoding),
            b'+' => {
                i += 1;
                b' '
            }
            value => {
                i += 1;
                value
            }
        };
        if path.len as usize == SD_PATH_MAX {
            return Err(ParseError::PathTooLong);
        }
        path.bytes[path.len as usize] = value;
        path.len += 1;
    }
    if path.len == 0 || path.bytes[0] != b'/' || !path_within_root(&path) {
        return Err(ParseError::InvalidPath);
    }
    Ok(path)
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

/// Validate the optional compile-time token without allocating or exposing
/// the configured value. Products pass the complete request header.
fn token_matches(header: &[u8], expected: Option<&[u8]>) -> bool {
    let Some(expected) = expected else {
        return true;
    };
    header
        .split(|b| *b == b'\n')
        .skip(1)
        .find_map(|line| {
            let (name, value) = split_header(line)?;
            name.eq_ignore_ascii_case(TOKEN_HEADER)
                .then_some(value.trim_ascii())
        })
        .is_some_and(|provided| provided == expected)
}

fn split_header(line: &[u8]) -> Option<(&[u8], &[u8])> {
    let index = line.iter().position(|value| *value == b':')?;
    Some((&line[..index], &line[index + 1..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn token_contract_uses_first_header_and_exact_value() {
        assert!(token_matches(
            b"GET / HTTP/1.1\r\nx-upload-token: secret\r\n",
            Some(b"secret")
        ));
        assert!(!token_matches(
            b"GET / HTTP/1.1\r\nx-upload-token: wrong\r\nx-upload-token: secret\r\n",
            Some(b"secret")
        ));
        assert!(!token_matches(b"GET / HTTP/1.1\r\n", Some(b"secret")));
        assert!(token_matches(b"GET / HTTP/1.1\r\n", None));
    }
}
