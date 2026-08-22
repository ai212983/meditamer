//! Build, sign, and inspect ADR-0014 signed bundles (Phase 4). One
//! implementation of the header layout lives in the `bundle` crate — this
//! module is just the host-side signer/inspector wrapped around it, reusing
//! `signing_key`'s shared signing-key file format (raw 32 bytes or 64 hex
//! chars) so both bundle formats' host tooling read keys the same way.

use std::{fs, path::Path};

use anyhow::{bail, Context, Result};
use ed25519_dalek::{Signer, VerifyingKey};
use sha2::{Digest, Sha256};

use bundle::{BundleError, BundleHeader};

use super::super::signing_key::read_signing_key;

#[derive(Debug)]
pub struct BuiltBundle {
    pub bundle_bytes: usize,
    pub firmware_len: u32,
    pub firmware_digest: [u8; 32],
    pub build_id: String,
    pub public_key_hex: String,
}

/// Reads `firmware_path`, signs it into an ADR-0014 bundle with the key at
/// `key_path`, and writes header-then-payload to `out_path`. Self-verifies
/// the freshly built bundle against its own public key before returning —
/// a bundle that fails its own signer's verification would never pass the
/// updater's, so catch that here rather than on a board.
pub fn build_and_sign(
    firmware_path: &Path,
    key_path: &Path,
    target_id: u16,
    layout_id: u16,
    build_id: &str,
    out_path: &Path,
) -> Result<BuiltBundle> {
    let firmware = fs::read(firmware_path)
        .with_context(|| format!("failed to read firmware image {}", firmware_path.display()))?;
    if firmware.is_empty() || !firmware.len().is_multiple_of(4) {
        bail!(
            "firmware image length {} must be non-zero and a multiple of 4",
            firmware.len()
        );
    }
    let signing_key = read_signing_key(key_path)?;
    let digest: [u8; 32] = Sha256::digest(&firmware).into();

    let mut header = BundleHeader::new(target_id, layout_id, build_id, firmware.len() as u32, digest, [0u8; 64])
        .map_err(|err| bundle_error("build header", err))?;
    header.signature = signing_key.sign(&header.signing_message()).to_bytes();

    header
        .verify(target_id, layout_id, u32::MAX, &signing_key.verifying_key())
        .map_err(|err| bundle_error("self-verify freshly built bundle", err))?;

    let mut out = Vec::with_capacity(bundle::HEADER_LEN + firmware.len());
    out.extend_from_slice(&header.encode());
    out.extend_from_slice(&firmware);
    fs::write(out_path, &out)
        .with_context(|| format!("failed to write bundle to {}", out_path.display()))?;

    Ok(BuiltBundle {
        bundle_bytes: out.len(),
        firmware_len: header.firmware_len,
        firmware_digest: header.firmware_digest,
        build_id: header.build_id().to_string(),
        public_key_hex: hex(&signing_key.verifying_key().to_bytes()),
    })
}

pub struct InspectedBundle {
    pub target_id: u16,
    pub layout_id: u16,
    pub build_id: String,
    pub firmware_len: u32,
    pub firmware_digest: [u8; 32],
    /// `None` if no public key was supplied to check against; `Some(true)`
    /// or `Some(false)` if one was.
    pub signature_valid: Option<bool>,
    /// `None` if the file is shorter than the header claims (can't hash a
    /// payload that isn't there); `Some(true)`/`Some(false)` otherwise.
    /// Independent of `signature_valid` — a bundle can have a genuine
    /// signature over a header whose digest field doesn't match payload
    /// bytes that were truncated or corrupted after signing.
    pub payload_digest_matches: Option<bool>,
}

/// Parses `bundle_path`'s header and reports its fields, optionally checking
/// the signature against `public_key_hex` (64 hex chars) and always checking
/// the header's committed digest against the file's actual payload bytes
/// (not just trusting the header) if the file is at least as long as it
/// claims.
pub fn inspect(bundle_path: &Path, public_key_hex: Option<&str>) -> Result<InspectedBundle> {
    let bytes = fs::read(bundle_path)
        .with_context(|| format!("failed to read bundle {}", bundle_path.display()))?;
    if bytes.len() < bundle::HEADER_LEN {
        bail!(
            "bundle is {} bytes, shorter than the {}-byte header",
            bytes.len(),
            bundle::HEADER_LEN
        );
    }
    let header = BundleHeader::decode(&bytes[..bundle::HEADER_LEN])
        .map_err(|err| bundle_error("decode header", err))?;

    let signature_valid = match public_key_hex {
        None => None,
        Some(hex_key) => {
            let key_bytes = decode_hex_32(hex_key)?;
            let public_key = VerifyingKey::from_bytes(&key_bytes)
                .context("public key bytes are not a valid ed25519 verifying key")?;
            Some(
                header
                    .verify(header.target_id, header.layout_id, u32::MAX, &public_key)
                    .is_ok(),
            )
        }
    };

    let payload = &bytes[bundle::HEADER_LEN..];
    let payload_digest_matches = if (payload.len() as u64) < header.firmware_len as u64 {
        None
    } else {
        let digest: [u8; 32] = Sha256::digest(&payload[..header.firmware_len as usize]).into();
        Some(digest == header.firmware_digest)
    };

    Ok(InspectedBundle {
        target_id: header.target_id,
        layout_id: header.layout_id,
        build_id: header.build_id().to_string(),
        firmware_len: header.firmware_len,
        firmware_digest: header.firmware_digest,
        signature_valid,
        payload_digest_matches,
    })
}

fn bundle_error(step: &str, err: BundleError) -> anyhow::Error {
    anyhow::anyhow!("failed to {step}: {err:?}")
}

fn decode_hex_32(value: &str) -> Result<[u8; 32]> {
    let value = value.trim();
    if value.len() != 64 {
        bail!("public key must be 64 hex characters, got {}", value.len());
    }
    let mut out = [0u8; 32];
    for (i, chunk) in value.as_bytes().chunks_exact(2).enumerate() {
        let hi = hex_nibble(chunk[0])?;
        let lo = hex_nibble(chunk[1])?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_nibble(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => bail!("invalid hex character {}", byte as char),
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_key(dir: &Path) -> std::path::PathBuf {
        let path = dir.join("key.bin");
        fs::write(&path, [0x42u8; 32]).unwrap();
        path
    }

    #[test]
    fn builds_a_bundle_that_inspects_and_verifies_cleanly() {
        let dir = tempdir().unwrap();
        let firmware_path = dir.path().join("fw.bin");
        fs::write(&firmware_path, vec![0xABu8; 256]).unwrap();
        let key_path = write_key(dir.path());
        let out_path = dir.path().join("bundle.bin");

        let built = build_and_sign(&firmware_path, &key_path, 1, 1, "test-build", &out_path).unwrap();
        assert_eq!(built.firmware_len, 256);
        assert_eq!(built.build_id, "test-build");
        assert_eq!(built.bundle_bytes, bundle::HEADER_LEN + 256);

        let inspected = inspect(&out_path, Some(&built.public_key_hex)).unwrap();
        assert_eq!(inspected.target_id, 1);
        assert_eq!(inspected.layout_id, 1);
        assert_eq!(inspected.build_id, "test-build");
        assert_eq!(inspected.firmware_len, 256);
        assert_eq!(inspected.signature_valid, Some(true));
        assert_eq!(inspected.payload_digest_matches, Some(true));
    }

    #[test]
    fn inspect_flags_a_wrong_public_key() {
        let dir = tempdir().unwrap();
        let firmware_path = dir.path().join("fw.bin");
        fs::write(&firmware_path, vec![0x11u8; 64]).unwrap();
        let key_path = write_key(dir.path());
        let out_path = dir.path().join("bundle.bin");
        build_and_sign(&firmware_path, &key_path, 1, 1, "b", &out_path).unwrap();

        let wrong_key = hex(&[0u8; 32]);
        let inspected = inspect(&out_path, Some(&wrong_key)).unwrap();
        assert_eq!(inspected.signature_valid, Some(false));
    }

    #[test]
    fn inspect_flags_a_payload_truncated_after_signing() {
        let dir = tempdir().unwrap();
        let firmware_path = dir.path().join("fw.bin");
        fs::write(&firmware_path, vec![0x22u8; 128]).unwrap();
        let key_path = write_key(dir.path());
        let out_path = dir.path().join("bundle.bin");
        build_and_sign(&firmware_path, &key_path, 1, 1, "b", &out_path).unwrap();

        let mut bytes = fs::read(&out_path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xFF;
        fs::write(&out_path, &bytes).unwrap();

        let inspected = inspect(&out_path, None).unwrap();
        assert_eq!(inspected.payload_digest_matches, Some(false));
    }

    #[test]
    fn rejects_a_firmware_length_that_is_not_a_multiple_of_four() {
        let dir = tempdir().unwrap();
        let firmware_path = dir.path().join("fw.bin");
        fs::write(&firmware_path, vec![0x00u8; 5]).unwrap();
        let key_path = write_key(dir.path());
        let out_path = dir.path().join("bundle.bin");

        let err = build_and_sign(&firmware_path, &key_path, 1, 1, "b", &out_path).unwrap_err();
        assert!(err.to_string().contains("multiple of 4"));
    }
}
