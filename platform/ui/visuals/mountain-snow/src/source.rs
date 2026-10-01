//! Streaming validation and row reads over an [`AssetSource`].
//!
//! [`validate_source`] checks one pack image without holding it
//! contiguously: it reads exactly the 64-byte header, parses it with
//! [`parse_header`], requires the source length
//! to equal the header's exact file length, then streams the payload once
//! through a caller-owned CRC scratch buffer and [`Crc32`].
//! Success publishes a [`ValidatedPack`], which carries only the validated
//! immutable [`Header`] — not a policy lease or handle.
//!
//! [`ValidatedPack::read_row`] then fills a caller-owned [`RowScratch`]
//! (2475 bytes: rock, snow, barrier, eligibility, noise planes) with
//! exactly five [`read_exact_at`](AssetSource::read_exact_at) calls, one per
//! plane, so the composer streams source rows instead of holding the full
//! order field. The borrow fast path
//! ([`borrow_at`](AssetSource::borrow_at)) is never used here: every read
//! goes through `read_exact_at`, which also serves fragmented stores.
//!
//! No heap, no statics, no caching, no async: every buffer stays with the
//! caller.

use asset_source::AssetSource;

use crate::composer::RowInputs;
use crate::pack::{parse_header, Crc32, Header, PackError, HEADER_LEN, WIDTH};

/// Failure modes for source validation and row reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ValidationError<E> {
    /// A source read failed; carries the source's own error.
    Access(E),
    /// The bytes failed pack validation: a header problem, a payload CRC
    /// mismatch, or a row index outside the stored band.
    Invalid(PackError),
    /// The caller-owned CRC scratch buffer is empty.
    EmptyScratch,
    /// `source.len()` disagrees with the validated file length, so the
    /// source cannot be the image the header describes. A source that
    /// changed after validation fails here before any byte is used.
    LengthMismatch {
        /// Logical length the source currently reports.
        actual: usize,
        /// Exact file length the validated header requires.
        expected: usize,
    },
}

/// Validated pack metadata: the [`Header`] of a source whose length and
/// payload CRC [`validate_source`] has checked.
///
/// This is validated immutable metadata only, not a policy lease or handle:
/// each [`read_row`](ValidatedPack::read_row) re-checks the source length
/// against the header before touching any byte. It is not bound to a source
/// identity or generation: the recheck catches length changes only, not
/// same-length content replacement, so the caller must keep the same source
/// bytes immutable between validation and row reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValidatedPack {
    header: Header,
}

impl ValidatedPack {
    /// Adopt a complete ordered stream already checked by its caller for
    /// exact length and payload CRC. The codec still owns the header and
    /// checksum comparison; callers must not publish bytes until this passes.
    pub fn from_streamed_crc(
        header_bytes: &[u8; HEADER_LEN],
        file_len: usize,
        payload_crc: u32,
    ) -> Result<Self, PackError> {
        let header = parse_header(header_bytes)?;
        if file_len != header.file_len() {
            return Err(PackError::BadFileLen);
        }
        if payload_crc != header.crc {
            return Err(PackError::BadCrc);
        }
        Ok(Self { header })
    }

    /// The validated header.
    #[must_use]
    pub fn header(&self) -> Header {
        self.header
    }

    /// Fill `scratch` with one stored row (`0..header.rows`, relative to the
    /// stored band) using exactly five `read_exact_at` calls, one per plane.
    ///
    /// The source length is re-checked against the validated file length
    /// first, so a source whose length changed after validation fails before
    /// any byte is used. [`ValidationError::LengthMismatch`] and
    /// [`ValidationError::Invalid`] (`BadRow`) fail before the plane reads
    /// and leave `scratch` untouched; an [`ValidationError::Access`] failure
    /// may leave earlier planes already written, so callers must
    /// discard/reload `scratch` and never compose it after `Err`.
    pub fn read_row<S: AssetSource>(
        &self,
        source: &S,
        relative_row: usize,
        scratch: &mut RowScratch,
    ) -> Result<(), ValidationError<S::Error>> {
        let expected = self.header.file_len();
        let actual = source.len();
        if actual != expected {
            return Err(ValidationError::LengthMismatch { actual, expected });
        }
        let ranges = self
            .header
            .row_ranges(relative_row)
            .map_err(ValidationError::Invalid)?;
        source
            .read_exact_at(ranges.rock.offset, &mut scratch.rock)
            .map_err(ValidationError::Access)?;
        source
            .read_exact_at(ranges.snow.offset, &mut scratch.snow)
            .map_err(ValidationError::Access)?;
        source
            .read_exact_at(ranges.barrier.offset, &mut scratch.barrier)
            .map_err(ValidationError::Access)?;
        source
            .read_exact_at(ranges.eligible.offset, &mut scratch.eligible)
            .map_err(ValidationError::Access)?;
        source
            .read_exact_at(ranges.noise.offset, &mut scratch.noise)
            .map_err(ValidationError::Access)?;
        Ok(())
    }
}

/// Validate one pack image held by `source`.
///
/// Reads exactly the 64-byte header, parses it, requires
/// `source.len() == header.file_len()`, then streams the payload exactly
/// once through the caller-owned (nonempty) `crc_scratch` buffer and an
/// incremental [`Crc32`] digest. A digest mismatch fails
/// as [`PackError::BadCrc`]. The file is never coalesced: at most
/// `crc_scratch.len()` payload bytes are held at once, plus the header.
///
/// The returned [`ValidatedPack`] is not bound to a source identity or
/// generation: keep the same source bytes immutable between validation and
/// row reads. Later [`read_row`](ValidatedPack::read_row) calls recheck only
/// the length, so same-length replacement is not detected.
pub fn validate_source<S: AssetSource>(
    source: &S,
    crc_scratch: &mut [u8],
) -> Result<ValidatedPack, ValidationError<S::Error>> {
    if crc_scratch.is_empty() {
        return Err(ValidationError::EmptyScratch);
    }
    let mut header_bytes = [0u8; HEADER_LEN];
    source
        .read_exact_at(0, &mut header_bytes)
        .map_err(ValidationError::Access)?;
    let header = parse_header(&header_bytes).map_err(ValidationError::Invalid)?;
    let expected = header.file_len();
    let actual = source.len();
    if actual != expected {
        return Err(ValidationError::LengthMismatch { actual, expected });
    }
    let mut crc = Crc32::new();
    let mut offset = HEADER_LEN;
    let mut remaining = header.payload_len();
    while remaining > 0 {
        let take = remaining.min(crc_scratch.len());
        source
            .read_exact_at(offset, &mut crc_scratch[..take])
            .map_err(ValidationError::Access)?;
        crc.update(&crc_scratch[..take]);
        // `take <= remaining` by construction, so this cannot underflow;
        // `offset` stays inside the length-checked file image.
        remaining -= take;
        offset += take;
    }
    if crc.finalize() != header.crc {
        return Err(ValidationError::Invalid(PackError::BadCrc));
    }
    Ok(ValidatedPack { header })
}

/// Caller-owned planes for one stored row: 600 rock + 600 snow + 600
/// barrier + 75 eligibility + 600 noise bytes, 2475 bytes total.
///
/// Feed [`inputs`](RowScratch::inputs) to
/// [`compose_row`](crate::composer::compose_row).
#[derive(Debug, Eq, PartialEq)]
pub struct RowScratch {
    /// Rock-endpoint gray row (`WIDTH` bytes).
    pub rock: [u8; WIDTH],
    /// Snow-endpoint gray row (`WIDTH` bytes).
    pub snow: [u8; WIDTH],
    /// Arrival barrier row (`WIDTH` bytes).
    pub barrier: [u8; WIDTH],
    /// Eligibility bits, MSB-first (`WIDTH / 8` bytes).
    pub eligible: [u8; WIDTH / 8],
    /// Blue-noise threshold row (`WIDTH` bytes).
    pub noise: [u8; WIDTH],
}

impl RowScratch {
    /// Zeroed scratch.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            rock: [0; WIDTH],
            snow: [0; WIDTH],
            barrier: [0; WIDTH],
            eligible: [0; WIDTH / 8],
            noise: [0; WIDTH],
        }
    }

    /// Borrow the planes as composer inputs.
    #[must_use]
    pub fn inputs(&self) -> RowInputs<'_> {
        RowInputs {
            rock: &self.rock,
            snow: &self.snow,
            barrier: &self.barrier,
            eligible: &self.eligible,
            noise: &self.noise,
        }
    }
}

impl Default for RowScratch {
    fn default() -> Self {
        Self::new()
    }
}
