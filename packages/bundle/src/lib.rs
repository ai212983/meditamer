#![no_std]
#![doc = r#"
Signed firmware bundle format shared between the factory updater and host
tooling (ADR-0014, Phase 1). One implementation of the header layout, the
signing message, and the validation rules is compiled into both consumers so
"host and updater validation produce the same results" by construction: there
is exactly one code path, not two independently written ones.

A bundle on SD is this fixed-size header immediately followed by
`firmware_len` bytes of firmware image payload. The header commits to the
target, the partition layout, the firmware's build identity, the payload
length, and its SHA-256 digest; the whole header (other than the signature
field itself) is what gets signed.
"#]

use ed25519_dalek::{Signature, VerifyingKey};
use sha2::{Digest, Sha256};

/// Bytes that must open every bundle. Encodes the format version (`1`) so a
/// future incompatible layout fails fast instead of being misparsed.
pub const MAGIC: [u8; 8] = *b"MDTMBND1";

/// Maximum length of the embedded build identity string, matching the
/// firmware's own `MEDITAMER_FIRMWARE_BUILD_ID` constraint (see `build.rs`).
pub const BUILD_ID_MAX: usize = 31;

/// Domain-separates bundle signatures from the legacy per-chunk A/B stream
/// signature (`MEDITAMER-FIRMWARE-V1`, see `src/firmware/update.rs`), so a
/// signature valid for one protocol can never be replayed as the other.
pub const SIGNING_DOMAIN: &[u8] = b"MEDITAMER-BUNDLE-V1";

const TARGET_ID_LEN: usize = 2;
const LAYOUT_ID_LEN: usize = 2;
const BUILD_ID_LEN_LEN: usize = 1;
const FIRMWARE_LEN_LEN: usize = 4;
const DIGEST_LEN: usize = 32;
const SIGNATURE_LEN: usize = 64;

/// Total on-SD header length in bytes. The firmware payload starts here.
pub const HEADER_LEN: usize = MAGIC.len()
    + TARGET_ID_LEN
    + LAYOUT_ID_LEN
    + BUILD_ID_LEN_LEN
    + BUILD_ID_MAX
    + FIRMWARE_LEN_LEN
    + DIGEST_LEN
    + SIGNATURE_LEN;

const MESSAGE_LEN: usize = SIGNING_DOMAIN.len()
    + TARGET_ID_LEN
    + LAYOUT_ID_LEN
    + BUILD_ID_LEN_LEN
    + BUILD_ID_MAX
    + FIRMWARE_LEN_LEN
    + DIGEST_LEN;

/// Everything that can be wrong with a bundle header or its payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BundleError {
    /// Fewer than [`HEADER_LEN`] bytes were supplied.
    Truncated,
    /// The leading [`MAGIC`] bytes did not match.
    Magic,
    /// `build_id_len` exceeds [`BUILD_ID_MAX`], or the build id bytes beyond
    /// it are non-zero, or a byte within it is not ASCII alphanumeric /
    /// `.` / `_` / `-`.
    BuildId,
    /// The header's `target_id` does not match what the caller expects.
    Target,
    /// The header's `layout_id` does not match what the caller expects.
    Layout,
    /// `firmware_len` is zero, not a multiple of 4, or exceeds the caller's
    /// `max_firmware_len`.
    Length,
    /// The streamed payload's SHA-256 digest did not match the header.
    Digest,
    /// The ed25519 signature over the header did not verify.
    Signature,
}

/// A parsed, still-unverified bundle header. Use [`BundleHeader::decode`] to
/// build one from raw bytes and [`BundleHeader::verify`] to check it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BundleHeader {
    pub target_id: u16,
    pub layout_id: u16,
    build_id_len: u8,
    build_id: [u8; BUILD_ID_MAX],
    pub firmware_len: u32,
    pub firmware_digest: [u8; DIGEST_LEN],
    pub signature: [u8; SIGNATURE_LEN],
}

fn valid_build_id_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-')
}

impl BundleHeader {
    /// Builds a header from its fields, ready for [`BundleHeader::encode`]
    /// and signing. `build_id` longer than [`BUILD_ID_MAX`] is rejected.
    pub fn new(
        target_id: u16,
        layout_id: u16,
        build_id: &str,
        firmware_len: u32,
        firmware_digest: [u8; DIGEST_LEN],
        signature: [u8; SIGNATURE_LEN],
    ) -> Result<Self, BundleError> {
        let bytes = build_id.as_bytes();
        if bytes.is_empty()
            || bytes.len() > BUILD_ID_MAX
            || !bytes.iter().copied().all(valid_build_id_byte)
        {
            return Err(BundleError::BuildId);
        }
        let mut stored = [0u8; BUILD_ID_MAX];
        stored[..bytes.len()].copy_from_slice(bytes);
        Ok(Self {
            target_id,
            layout_id,
            build_id_len: bytes.len() as u8,
            build_id: stored,
            firmware_len,
            firmware_digest,
            signature,
        })
    }

    pub fn build_id(&self) -> &str {
        core::str::from_utf8(&self.build_id[..self.build_id_len as usize]).unwrap_or("")
    }

    /// Serializes this header to its fixed [`HEADER_LEN`]-byte wire form.
    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let mut out = [0u8; HEADER_LEN];
        let mut cursor = 0;
        out[cursor..cursor + MAGIC.len()].copy_from_slice(&MAGIC);
        cursor += MAGIC.len();
        out[cursor..cursor + TARGET_ID_LEN].copy_from_slice(&self.target_id.to_le_bytes());
        cursor += TARGET_ID_LEN;
        out[cursor..cursor + LAYOUT_ID_LEN].copy_from_slice(&self.layout_id.to_le_bytes());
        cursor += LAYOUT_ID_LEN;
        out[cursor] = self.build_id_len;
        cursor += BUILD_ID_LEN_LEN;
        out[cursor..cursor + BUILD_ID_MAX].copy_from_slice(&self.build_id);
        cursor += BUILD_ID_MAX;
        out[cursor..cursor + FIRMWARE_LEN_LEN].copy_from_slice(&self.firmware_len.to_le_bytes());
        cursor += FIRMWARE_LEN_LEN;
        out[cursor..cursor + DIGEST_LEN].copy_from_slice(&self.firmware_digest);
        cursor += DIGEST_LEN;
        out[cursor..cursor + SIGNATURE_LEN].copy_from_slice(&self.signature);
        cursor += SIGNATURE_LEN;
        debug_assert_eq!(cursor, HEADER_LEN);
        out
    }

    /// Parses a header from at least [`HEADER_LEN`] bytes. Checks structural
    /// validity (magic, build-id charset) but not the signature or target /
    /// layout match — call [`BundleHeader::verify`] for that.
    pub fn decode(bytes: &[u8]) -> Result<Self, BundleError> {
        if bytes.len() < HEADER_LEN {
            return Err(BundleError::Truncated);
        }
        if bytes[..MAGIC.len()] != MAGIC {
            return Err(BundleError::Magic);
        }
        let mut cursor = MAGIC.len();
        let target_id =
            u16::from_le_bytes(bytes[cursor..cursor + TARGET_ID_LEN].try_into().unwrap());
        cursor += TARGET_ID_LEN;
        let layout_id =
            u16::from_le_bytes(bytes[cursor..cursor + LAYOUT_ID_LEN].try_into().unwrap());
        cursor += LAYOUT_ID_LEN;
        let build_id_len = bytes[cursor];
        cursor += BUILD_ID_LEN_LEN;
        let build_id_bytes = &bytes[cursor..cursor + BUILD_ID_MAX];
        let mut build_id = [0u8; BUILD_ID_MAX];
        build_id.copy_from_slice(build_id_bytes);
        cursor += BUILD_ID_MAX;
        let firmware_len =
            u32::from_le_bytes(bytes[cursor..cursor + FIRMWARE_LEN_LEN].try_into().unwrap());
        cursor += FIRMWARE_LEN_LEN;
        let mut firmware_digest = [0u8; DIGEST_LEN];
        firmware_digest.copy_from_slice(&bytes[cursor..cursor + DIGEST_LEN]);
        cursor += DIGEST_LEN;
        let mut signature = [0u8; SIGNATURE_LEN];
        signature.copy_from_slice(&bytes[cursor..cursor + SIGNATURE_LEN]);
        cursor += SIGNATURE_LEN;
        debug_assert_eq!(cursor, HEADER_LEN);

        let build_id_len_usize = build_id_len as usize;
        if build_id_len_usize == 0 || build_id_len_usize > BUILD_ID_MAX {
            return Err(BundleError::BuildId);
        }
        if !build_id[..build_id_len_usize]
            .iter()
            .copied()
            .all(valid_build_id_byte)
            || build_id[build_id_len_usize..].iter().any(|byte| *byte != 0)
        {
            return Err(BundleError::BuildId);
        }

        Ok(Self {
            target_id,
            layout_id,
            build_id_len,
            build_id,
            firmware_len,
            firmware_digest,
            signature,
        })
    }

    /// The exact bytes an ed25519 signature over this header covers: the
    /// domain tag followed by every field except the signature itself. Host
    /// signing tools and the updater's verifier both derive this the same
    /// way, so they can never disagree about what was signed.
    pub fn signing_message(&self) -> [u8; MESSAGE_LEN] {
        let mut message = [0u8; MESSAGE_LEN];
        let mut cursor = 0;
        message[cursor..cursor + SIGNING_DOMAIN.len()].copy_from_slice(SIGNING_DOMAIN);
        cursor += SIGNING_DOMAIN.len();
        message[cursor..cursor + TARGET_ID_LEN].copy_from_slice(&self.target_id.to_le_bytes());
        cursor += TARGET_ID_LEN;
        message[cursor..cursor + LAYOUT_ID_LEN].copy_from_slice(&self.layout_id.to_le_bytes());
        cursor += LAYOUT_ID_LEN;
        message[cursor] = self.build_id_len;
        cursor += BUILD_ID_LEN_LEN;
        message[cursor..cursor + BUILD_ID_MAX].copy_from_slice(&self.build_id);
        cursor += BUILD_ID_MAX;
        message[cursor..cursor + FIRMWARE_LEN_LEN]
            .copy_from_slice(&self.firmware_len.to_le_bytes());
        cursor += FIRMWARE_LEN_LEN;
        message[cursor..cursor + DIGEST_LEN].copy_from_slice(&self.firmware_digest);
        cursor += DIGEST_LEN;
        debug_assert_eq!(cursor, MESSAGE_LEN);
        message
    }

    /// Checks this header against the running device's identity and a
    /// caller-supplied payload size ceiling (the target `ota_0` capacity),
    /// then verifies the ed25519 signature. Does not touch the payload
    /// itself — combine with [`PayloadHasher`] to confirm the digest.
    pub fn verify(
        &self,
        expected_target_id: u16,
        expected_layout_id: u16,
        max_firmware_len: u32,
        public_key: &VerifyingKey,
    ) -> Result<(), BundleError> {
        if self.target_id != expected_target_id {
            return Err(BundleError::Target);
        }
        if self.layout_id != expected_layout_id {
            return Err(BundleError::Layout);
        }
        if self.firmware_len == 0
            || !self.firmware_len.is_multiple_of(4)
            || self.firmware_len > max_firmware_len
        {
            return Err(BundleError::Length);
        }
        let signature = Signature::from_bytes(&self.signature);
        public_key
            .verify_strict(&self.signing_message(), &signature)
            .map_err(|_| BundleError::Signature)
    }
}

/// Streaming SHA-256 over the firmware payload, fed in bounded chunks as it
/// is read from SD. Both the updater (verifying) and any host-side signer
/// (computing the digest to sign) use this so the digest is computed
/// identically on both ends.
#[derive(Clone)]
pub struct PayloadHasher(Sha256);

impl Default for PayloadHasher {
    fn default() -> Self {
        Self::new()
    }
}

impl PayloadHasher {
    pub fn new() -> Self {
        Self(Sha256::new())
    }

    pub fn update(&mut self, chunk: &[u8]) {
        self.0.update(chunk);
    }

    /// Consumes the hasher and reports whether the accumulated digest
    /// matches `expected` (the header's `firmware_digest`).
    pub fn finish_matches(self, expected: &[u8; DIGEST_LEN]) -> bool {
        let digest: [u8; DIGEST_LEN] = self.0.finalize().into();
        digest == *expected
    }

    pub fn finish(self) -> [u8; DIGEST_LEN] {
        self.0.finalize().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    // Fixed seed: deterministic fixtures need no RNG, matching how the host
    // firmware-update signer already loads a key (see
    // tools/hostctl/src/workflows/firmware_update.rs::read_signing_key).
    const TEST_SEED: [u8; 32] = [7u8; 32];

    fn test_key() -> (SigningKey, VerifyingKey) {
        let signing_key = SigningKey::from_bytes(&TEST_SEED);
        let verifying_key = signing_key.verifying_key();
        (signing_key, verifying_key)
    }

    fn signed_header(
        signing_key: &SigningKey,
        target_id: u16,
        layout_id: u16,
        build_id: &str,
        firmware_len: u32,
        firmware_digest: [u8; DIGEST_LEN],
    ) -> BundleHeader {
        let unsigned = BundleHeader::new(
            target_id,
            layout_id,
            build_id,
            firmware_len,
            firmware_digest,
            [0; SIGNATURE_LEN],
        )
        .expect("valid fixture header");
        let signature = signing_key.sign(&unsigned.signing_message()).to_bytes();
        BundleHeader::new(
            target_id,
            layout_id,
            build_id,
            firmware_len,
            firmware_digest,
            signature,
        )
        .expect("valid signed header")
    }

    #[test]
    fn round_trips_through_encode_decode() {
        let (signing_key, _) = test_key();
        let header = signed_header(
            &signing_key,
            1,
            1,
            "2026.08.17-abcdef",
            4096,
            [9u8; DIGEST_LEN],
        );
        let encoded = header.encode();
        assert_eq!(encoded.len(), HEADER_LEN);
        let decoded = BundleHeader::decode(&encoded).expect("decode");
        assert_eq!(decoded, header);
        assert_eq!(decoded.build_id(), "2026.08.17-abcdef");
    }

    #[test]
    fn accepts_a_correctly_signed_bundle() {
        let (signing_key, verifying_key) = test_key();
        let header = signed_header(&signing_key, 1, 1, "unlabeled", 4096, [3u8; DIGEST_LEN]);
        assert_eq!(header.verify(1, 1, 4096, &verifying_key), Ok(()));
    }

    #[test]
    fn rejects_truncated_bytes() {
        let bytes = [0u8; HEADER_LEN - 1];
        assert_eq!(BundleHeader::decode(&bytes), Err(BundleError::Truncated));
    }

    #[test]
    fn rejects_bad_magic() {
        let (signing_key, _) = test_key();
        let header = signed_header(&signing_key, 1, 1, "unlabeled", 4096, [3u8; DIGEST_LEN]);
        let mut encoded = header.encode();
        encoded[0] ^= 0xff;
        assert_eq!(BundleHeader::decode(&encoded), Err(BundleError::Magic));
    }

    #[test]
    fn rejects_build_id_with_stray_trailing_bytes() {
        let mut encoded = {
            let (signing_key, _) = test_key();
            signed_header(&signing_key, 1, 1, "short", 4096, [3u8; DIGEST_LEN]).encode()
        };
        // Build-id bytes past build_id_len must stay zero; corrupt one.
        let build_id_start = MAGIC.len() + TARGET_ID_LEN + LAYOUT_ID_LEN + BUILD_ID_LEN_LEN;
        encoded[build_id_start + 10] = b'x';
        assert_eq!(BundleHeader::decode(&encoded), Err(BundleError::BuildId));
    }

    #[test]
    fn rejects_wrong_target() {
        let (signing_key, verifying_key) = test_key();
        let header = signed_header(&signing_key, 1, 1, "unlabeled", 4096, [3u8; DIGEST_LEN]);
        assert_eq!(
            header.verify(2, 1, 4096, &verifying_key),
            Err(BundleError::Target)
        );
    }

    #[test]
    fn rejects_wrong_layout() {
        let (signing_key, verifying_key) = test_key();
        let header = signed_header(&signing_key, 1, 1, "unlabeled", 4096, [3u8; DIGEST_LEN]);
        assert_eq!(
            header.verify(1, 2, 4096, &verifying_key),
            Err(BundleError::Layout)
        );
    }

    #[test]
    fn rejects_length_over_partition_capacity() {
        let (signing_key, verifying_key) = test_key();
        let header = signed_header(&signing_key, 1, 1, "unlabeled", 8192, [3u8; DIGEST_LEN]);
        assert_eq!(
            header.verify(1, 1, 4096, &verifying_key),
            Err(BundleError::Length)
        );
    }

    #[test]
    fn rejects_length_not_a_multiple_of_four() {
        let (signing_key, verifying_key) = test_key();
        let header = signed_header(&signing_key, 1, 1, "unlabeled", 4097, [3u8; DIGEST_LEN]);
        assert_eq!(
            header.verify(1, 1, 8192, &verifying_key),
            Err(BundleError::Length)
        );
    }

    #[test]
    fn rejects_tampered_signature() {
        let (signing_key, verifying_key) = test_key();
        let mut header = signed_header(&signing_key, 1, 1, "unlabeled", 4096, [3u8; DIGEST_LEN]);
        header.signature[0] ^= 0xff;
        assert_eq!(
            header.verify(1, 1, 4096, &verifying_key),
            Err(BundleError::Signature)
        );
    }

    #[test]
    fn rejects_signature_over_a_different_digest() {
        // A signature valid for one firmware_digest must not verify once the
        // header's digest field is swapped for a different (still correctly
        // shaped) one — catching a header/payload mix-and-match attempt.
        let (signing_key, verifying_key) = test_key();
        let header = signed_header(&signing_key, 1, 1, "unlabeled", 4096, [3u8; DIGEST_LEN]);
        let swapped =
            BundleHeader::new(1, 1, "unlabeled", 4096, [4u8; DIGEST_LEN], header.signature)
                .expect("valid fixture header");
        assert_eq!(
            swapped.verify(1, 1, 4096, &verifying_key),
            Err(BundleError::Signature)
        );
    }

    #[test]
    fn payload_hasher_matches_direct_sha256() {
        let mut hasher = PayloadHasher::new();
        hasher.update(b"complete ");
        hasher.update(b"bundle payload");
        let expected: [u8; 32] = Sha256::digest(b"complete bundle payload").into();
        assert_eq!(hasher.finish(), expected);
    }

    #[test]
    fn payload_hasher_flags_a_mismatched_digest() {
        let mut hasher = PayloadHasher::new();
        hasher.update(b"tampered payload");
        assert!(!hasher.finish_matches(&[0u8; DIGEST_LEN]));
    }
}
