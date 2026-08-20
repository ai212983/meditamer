//! Shared firmware-signing-key file format (raw 32-byte seed or 64
//! hex-character seed), read identically by every signer in this tool:
//! the retired A/B `firmware-update` workflow historically owned this, and
//! `workflows::single_production::bundle` (ADR-0014) now shares it so both
//! signed-bundle formats' host tooling read keys the same way. Split out on
//! its own so neither side has to depend on the other's workflow module.

use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use ed25519_dalek::SigningKey;

use crate::workflows::common::repo_path;

pub fn firmware_public_key_hex(path: &Path) -> Result<String> {
    Ok(hex(&read_signing_key(&repo_path(path))?
        .verifying_key()
        .to_bytes()))
}

pub(crate) fn read_signing_key(path: &Path) -> Result<SigningKey> {
    let raw = std::fs::read(path).with_context(|| format!("read signing key {}", path.display()))?;
    let bytes = if raw.len() == 32 {
        raw
    } else {
        let text = std::str::from_utf8(&raw)?.trim();
        if text.len() != 64 {
            bail!("signing key must be 32 raw bytes or 64 hex characters");
        }
        decode_hex(text.as_bytes())?
    };
    let seed: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow!("invalid key length"))?;
    Ok(SigningKey::from_bytes(&seed))
}

fn decode_hex(value: &[u8]) -> Result<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        bail!("hex value has odd length");
    }
    value
        .chunks_exact(2)
        .map(|pair| Ok((hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?))
        .collect()
}

fn hex_nibble(value: u8) -> Result<u8> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => bail!("non-hex character in key"),
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut result, "{byte:02x}");
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_and_hex_seed_produce_same_public_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let raw_path = dir.path().join("raw");
        let hex_path = dir.path().join("hex");
        let seed = [7u8; 32];
        std::fs::write(&raw_path, seed).expect("raw");
        std::fs::write(&hex_path, hex(&seed)).expect("hex");
        assert_eq!(
            firmware_public_key_hex(&raw_path).expect("raw public"),
            firmware_public_key_hex(&hex_path).expect("hex public")
        );
    }
}
