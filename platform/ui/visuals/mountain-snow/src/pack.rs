//! Device pack codec: header, integrity, and borrowed row access.
//!
//! Pack layout mirrors the sibling sky/sun pack family: an 8-byte magic,
//! little-endian geometry and length fields, IEEE CRC32 over the payload,
//! and MSB-first packed bit rows. The mountain pack carries gray endpoint
//! rows plus barrier, eligibility, and noise rows for the measured
//! mountain-bounds band only; the device can stream rows from SD or retain
//! the same file image in segmented PSRAM for local row reads.
//!
//! Header (64 bytes): `magic[8]`, `u16` width, `u16` height, `u16` first
//! row, `u16` row count, `u32` rock/snow/barrier/eligibility/noise
//! lengths (bytes, in that order), `u32` IEEE CRC32 of the payload, then
//! zero reserved bytes to 64. Payload order matches the length fields:
//! rock gray rows, snow gray rows, barrier rows, packed eligibility rows,
//! noise rows. All row blocks are `rows * width` bytes except
//! eligibility (`rows * width / 8`; width is a multiple of 8 here).
//!
//! No heap, no large statics: the 1 KiB CRC table is a `const` built by a
//! `const fn`, so it lives in flash, and every accessor borrows from the
//! caller's pack bytes.
//!
//! [`parse_header`] validates the 64-byte header alone so a streaming loader
//! can size its transfer ([`Header::file_len`]) before any payload
//! byte arrives. File-relative [`ByteRange`]s ([`Header::payload_range`],
//! [`Header::row_ranges`]) locate the payload and each stored row inside the
//! exact file image, and [`Crc32`] checks the payload incrementally across
//! irregular chunk boundaries.

/// Header length in bytes.
pub const HEADER_LEN: usize = 64;
/// 8-byte ASCII magic identifying the mountain pack format version 1.
pub const MAGIC: [u8; 8] = *b"AMBMNT01";
/// Pack geometry: full 600-pixel rows; only `rows` rows from `first_row`
/// are stored.
pub const WIDTH: usize = 600;

/// Codec and borrow errors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PackError {
    /// File slice is shorter than [`HEADER_LEN`].
    BadFileLen,
    /// First 8 bytes are not [`MAGIC`].
    BadMagic,
    /// A reserved byte is nonzero.
    BadReserved,
    /// Geometry is not 600-wide with a non-empty in-frame row band.
    BadGeometry,
    /// A length field disagrees with the geometry.
    BadLengthField,
    /// File slice is shorter than header plus payload.
    TruncatedPayload,
    /// Payload CRC32 does not match the header field.
    BadCrc,
    /// Row index is outside the stored band.
    BadRow,
}

/// File-relative byte range: `offset..offset + len` indexes the exact pack
/// file image (header plus payload).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ByteRange {
    /// Byte offset from the start of the pack file image.
    pub offset: usize,
    /// Length in bytes.
    pub len: usize,
}

impl ByteRange {
    /// One-past-the-end offset, or `None` if `offset + len` overflows.
    #[must_use]
    pub fn end(self) -> Option<usize> {
        self.offset.checked_add(self.len)
    }
}

/// Parsed header fields; payload access goes through [`Pack`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Header {
    /// First stored row (inclusive).
    pub first_row: usize,
    /// Stored row count.
    pub rows: usize,
    /// Payload section lengths in pack order.
    pub section_lens: [usize; 5],
    /// Expected IEEE CRC32 of the payload (header bytes 36..40, LE). A
    /// streaming loader validates payload bytes against this field.
    pub crc: u32,
}

/// File-relative byte ranges of the five sections for one stored row.
///
/// Row indexing matches [`Pack`] row accessors: `0..header.rows` relative to
/// the stored band, not absolute frame rows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RowRanges {
    /// Rock-endpoint gray row (`WIDTH` bytes).
    pub rock: ByteRange,
    /// Snow-endpoint gray row (`WIDTH` bytes).
    pub snow: ByteRange,
    /// Arrival barrier row (`WIDTH` bytes).
    pub barrier: ByteRange,
    /// Eligibility bits, MSB-first (`WIDTH / 8` bytes).
    pub eligible: ByteRange,
    /// Blue-noise threshold row (`WIDTH` bytes).
    pub noise: ByteRange,
}

impl Header {
    /// Total payload bytes across the five sections.
    #[must_use]
    pub fn payload_len(&self) -> usize {
        let mut total = 0usize;
        for &len in &self.section_lens {
            // Unreachable for validated v1 headers; saturate, never wrap.
            total = total.saturating_add(len);
        }
        total
    }

    /// Exact total file length: [`HEADER_LEN`] plus [`Header::payload_len`].
    #[must_use]
    pub fn file_len(&self) -> usize {
        // Unreachable for validated v1 headers; saturate, never wrap.
        HEADER_LEN.saturating_add(self.payload_len())
    }

    /// File-relative range covering the whole payload.
    #[must_use]
    pub fn payload_range(&self) -> ByteRange {
        ByteRange {
            offset: HEADER_LEN,
            len: self.payload_len(),
        }
    }

    /// File-relative byte ranges of one stored row (`0..self.rows`).
    pub fn row_ranges(&self, row: usize) -> Result<RowRanges, PackError> {
        if row >= self.rows {
            return Err(PackError::BadRow);
        }
        let strides = [WIDTH, WIDTH, WIDTH, WIDTH / 8, WIDTH];
        let mut base = HEADER_LEN;
        let mut ranges = [ByteRange { offset: 0, len: 0 }; 5];
        for (i, range) in ranges.iter_mut().enumerate() {
            let start = base
                .checked_add(row.checked_mul(strides[i]).ok_or(PackError::BadRow)?)
                .ok_or(PackError::BadRow)?;
            *range = ByteRange {
                offset: start,
                len: strides[i],
            };
            base = base
                .checked_add(self.section_lens[i])
                .ok_or(PackError::BadRow)?;
        }
        Ok(RowRanges {
            rock: ranges[0],
            snow: ranges[1],
            barrier: ranges[2],
            eligible: ranges[3],
            noise: ranges[4],
        })
    }
}

/// Borrowed pack view. All byte slices point into the caller's buffer.
///
/// Row accessors take `0..header.rows` relative to the stored band, not
/// absolute frame rows (same indexing as [`Header::row_ranges`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Pack<'a> {
    header: Header,
    rock: &'a [u8],
    snow: &'a [u8],
    barrier: &'a [u8],
    eligible: &'a [u8],
    noise: &'a [u8],
}

const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0usize;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 1 {
                0xEDB8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

const CRC_TABLE: [u32; 256] = crc_table();

/// Incremental IEEE CRC32 digest over irregular chunk boundaries.
///
/// Feeding the payload in several [`Crc32::update`] calls yields the same
/// digest as one [`crc32_ieee`] call over the concatenation, so a streaming
/// loader can check the transfer without holding the payload contiguously.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Crc32 {
    state: u32,
}

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

impl Crc32 {
    /// Fresh digest state.
    #[must_use]
    pub const fn new() -> Self {
        Self { state: 0xFFFF_FFFF }
    }

    /// Feed one chunk of the payload; call repeatedly for chunked input.
    pub fn update(&mut self, data: &[u8]) {
        let mut state = self.state;
        for &byte in data {
            let index = (state ^ u32::from(byte)) & 0xFF;
            state = CRC_TABLE[index as usize] ^ (state >> 8);
        }
        self.state = state;
    }

    /// Finish and return the IEEE CRC32 digest.
    #[must_use]
    pub fn finalize(&self) -> u32 {
        self.state ^ 0xFFFF_FFFF
    }
}

/// IEEE CRC32 (polynomial `0xEDB88320`), matching the sky/sun pack family.
#[must_use]
pub fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc = Crc32::new();
    crc.update(data);
    crc.finalize()
}

fn read_u16_le(bytes: &[u8]) -> usize {
    usize::from(bytes[0]) | (usize::from(bytes[1]) << 8)
}

fn read_u32_le(bytes: &[u8]) -> usize {
    usize::from(bytes[0])
        | (usize::from(bytes[1]) << 8)
        | (usize::from(bytes[2]) << 16)
        | (usize::from(bytes[3]) << 24)
}

/// Parse and validate only the 64-byte v1 header: minimum length, magic,
/// reserved bytes, geometry, and section lengths.
///
/// Accepts at least [`HEADER_LEN`] bytes and ignores trailing bytes: payload
/// presence and CRC are *not* checked, so a streaming loader can size its
/// transfer from the header before payload bytes arrive. Whole-file
/// validation stays in [`parse`]; exact-length enforcement stays in
/// [`validate_source`](crate::source::validate_source).
///
/// Error precedence is unchanged: [`PackError::BadFileLen`] for slices
/// shorter than [`HEADER_LEN`], then [`PackError::BadMagic`], then
/// [`PackError::BadReserved`], then [`PackError::BadGeometry`], then
/// [`PackError::BadLengthField`].
pub fn parse_header(file: &[u8]) -> Result<Header, PackError> {
    if file.len() < HEADER_LEN {
        return Err(PackError::BadFileLen);
    }
    if file[..8] != MAGIC {
        return Err(PackError::BadMagic);
    }
    if file[40..HEADER_LEN].iter().any(|&b| b != 0) {
        return Err(PackError::BadReserved);
    }
    let width = read_u16_le(&file[8..10]);
    let height = read_u16_le(&file[10..12]);
    let first_row = read_u16_le(&file[12..14]);
    let rows = read_u16_le(&file[14..16]);
    let band_end = first_row.checked_add(rows).ok_or(PackError::BadGeometry)?;
    if width != WIDTH || height != WIDTH || rows == 0 || band_end > height {
        return Err(PackError::BadGeometry);
    }
    let mut lens = [0usize; 5];
    for (i, slot) in lens.iter_mut().enumerate() {
        *slot = read_u32_le(&file[16 + 4 * i..20 + 4 * i]);
    }
    let row_bytes = rows.checked_mul(WIDTH).ok_or(PackError::BadGeometry)?;
    let expected = [row_bytes, row_bytes, row_bytes, row_bytes / 8, row_bytes];
    if lens != expected {
        return Err(PackError::BadLengthField);
    }
    let crc = read_u32_le(&file[36..40]) as u32;
    Ok(Header {
        first_row,
        rows,
        section_lens: lens,
        crc,
    })
}

/// Parse and integrity-check one pack file image.
///
/// Validates the expected prefix (header plus payload) and ignores trailing
/// bytes (legacy behavior, unchanged); exact-length enforcement stays in
/// [`validate_source`](crate::source::validate_source).
///
/// Error precedence is unchanged: header errors first (same order as
/// [`parse_header`]), then [`PackError::TruncatedPayload`] when the slice is
/// shorter than header plus payload, then [`PackError::BadCrc`], then
/// [`PackError::TruncatedPayload`] for short section splits.
pub fn parse(file: &[u8]) -> Result<Pack<'_>, PackError> {
    let header = parse_header(file)?;
    let payload_len = header.payload_len();
    let end = HEADER_LEN
        .checked_add(payload_len)
        .ok_or(PackError::TruncatedPayload)?;
    let payload = file
        .get(HEADER_LEN..end)
        .ok_or(PackError::TruncatedPayload)?;
    if crc32_ieee(payload) != header.crc {
        return Err(PackError::BadCrc);
    }
    let lens = header.section_lens;
    let (rock, rest) = payload
        .split_at_checked(lens[0])
        .ok_or(PackError::TruncatedPayload)?;
    let (snow, rest) = rest
        .split_at_checked(lens[1])
        .ok_or(PackError::TruncatedPayload)?;
    let (barrier, rest) = rest
        .split_at_checked(lens[2])
        .ok_or(PackError::TruncatedPayload)?;
    let (eligible, noise) = rest
        .split_at_checked(lens[3])
        .ok_or(PackError::TruncatedPayload)?;
    if noise.len() != lens[4] {
        return Err(PackError::TruncatedPayload);
    }
    Ok(Pack {
        header,
        rock,
        snow,
        barrier,
        eligible,
        noise,
    })
}

impl<'a> Pack<'a> {
    /// Parsed header fields.
    #[must_use]
    pub fn header(&self) -> Header {
        self.header
    }

    fn row(&self, section: &'a [u8], row: usize, stride: usize) -> Result<&'a [u8], PackError> {
        if row >= self.header.rows {
            return Err(PackError::BadRow);
        }
        Ok(&section[row * stride..(row + 1) * stride])
    }

    /// Rock-endpoint gray row (`WIDTH` bytes); `row` is stored-band-relative.
    pub fn rock_row(&self, row: usize) -> Result<&'a [u8], PackError> {
        self.row(self.rock, row, WIDTH)
    }

    /// Snow-endpoint gray row (`WIDTH` bytes); `row` is stored-band-relative.
    pub fn snow_row(&self, row: usize) -> Result<&'a [u8], PackError> {
        self.row(self.snow, row, WIDTH)
    }

    /// Arrival barrier row (`WIDTH` bytes); `row` is stored-band-relative.
    pub fn barrier_row(&self, row: usize) -> Result<&'a [u8], PackError> {
        self.row(self.barrier, row, WIDTH)
    }

    /// Eligibility bits, MSB-first (`WIDTH / 8` bytes); `row` is stored-band-relative.
    pub fn eligible_row(&self, row: usize) -> Result<&'a [u8], PackError> {
        self.row(self.eligible, row, WIDTH / 8)
    }

    /// Blue-noise threshold row (`WIDTH` bytes); `row` is stored-band-relative.
    pub fn noise_row(&self, row: usize) -> Result<&'a [u8], PackError> {
        self.row(self.noise, row, WIDTH)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec::Vec;

    fn pack_bytes() -> Vec<u8> {
        // Two stored rows: eligible snow then eligible rock.
        let rows = 2usize;
        let rock = vec![60u8; rows * WIDTH];
        let snow = vec![220u8; rows * WIDTH];
        let barrier = vec![0u8; rows * WIDTH];
        let eligible = vec![0xFFu8; rows * WIDTH / 8];
        // Noise 250 against snow gray 220: 256*220 <= 255*250 + 127 inks.
        let noise = vec![250u8; rows * WIDTH];
        let mut payload = Vec::new();
        payload.extend_from_slice(&rock);
        payload.extend_from_slice(&snow);
        payload.extend_from_slice(&barrier);
        payload.extend_from_slice(&eligible);
        payload.extend_from_slice(&noise);
        let mut header = vec![0u8; HEADER_LEN];
        header[..8].copy_from_slice(&MAGIC);
        for (i, v) in [600u16, 600, 0, 2].iter().enumerate() {
            header[8 + 2 * i..10 + 2 * i].copy_from_slice(&v.to_le_bytes());
        }
        for (i, section) in [&rock, &snow, &barrier, &eligible, &noise]
            .iter()
            .enumerate()
        {
            header[16 + 4 * i..20 + 4 * i].copy_from_slice(&(section.len() as u32).to_le_bytes());
        }
        header[36..40].copy_from_slice(&crc32_ieee(&payload).to_le_bytes());
        header.extend_from_slice(&payload);
        header
    }

    #[test]
    fn crc32_matches_ieee_test_vector() {
        assert_eq!(crc32_ieee(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn round_trip_rows() {
        let file = pack_bytes();
        let pack = parse(&file).expect("valid pack");
        assert_eq!(pack.header().rows, 2);
        assert_eq!(pack.rock_row(1).expect("row").len(), WIDTH);
        assert_eq!(pack.noise_row(5), Err(PackError::BadRow));
    }

    #[test]
    fn corruptions_are_rejected() {
        let mut file = pack_bytes();
        let mut bad = file.clone();
        bad[0] ^= 0xFF;
        assert_eq!(parse(&bad), Err(PackError::BadMagic));
        bad = file.clone();
        bad[HEADER_LEN] ^= 0xFF;
        assert_eq!(parse(&bad), Err(PackError::BadCrc));
        bad = file.clone();
        bad[50] = 1;
        assert_eq!(parse(&bad), Err(PackError::BadReserved));
        file.truncate(HEADER_LEN + 10);
        assert_eq!(parse(&file), Err(PackError::TruncatedPayload));
        assert_eq!(parse(&file[..10]), Err(PackError::BadFileLen));
    }

    #[test]
    fn header_only_parse_needs_no_payload() {
        let file = pack_bytes();
        let header = parse_header(&file[..HEADER_LEN]).expect("header-only parse");
        assert_eq!(header.rows, 2);
        assert_eq!(header.payload_len(), file.len() - HEADER_LEN);
        assert_eq!(header.file_len(), file.len());
        assert_eq!(
            header.payload_range(),
            ByteRange {
                offset: HEADER_LEN,
                len: file.len() - HEADER_LEN,
            }
        );
        assert_eq!(header.crc, crc32_ieee(&file[HEADER_LEN..]));
        // Payload presence and CRC stay whole-file checks in `parse`.
        assert_eq!(parse(&file[..HEADER_LEN]), Err(PackError::TruncatedPayload));
        let mut bad_crc = file.clone();
        bad_crc[HEADER_LEN] ^= 0xFF;
        assert!(parse_header(&bad_crc).is_ok());
        assert_eq!(parse(&bad_crc), Err(PackError::BadCrc));
    }

    #[test]
    fn header_rejects_malformed_fields() {
        let file = pack_bytes();
        assert_eq!(parse_header(&file[..10]), Err(PackError::BadFileLen));
        let mut bad = file.clone();
        bad[0] ^= 0xFF;
        assert_eq!(parse_header(&bad), Err(PackError::BadMagic));
        bad = file.clone();
        bad[50] = 1;
        assert_eq!(parse_header(&bad), Err(PackError::BadReserved));
        bad = file.clone();
        bad[8] = 1; // width
        assert_eq!(parse_header(&bad), Err(PackError::BadGeometry));
        bad = file.clone();
        bad[14] = 0;
        bad[15] = 0; // zero rows
        assert_eq!(parse_header(&bad), Err(PackError::BadGeometry));
        bad = file.clone();
        bad[16] ^= 0xFF; // rock length
        assert_eq!(parse_header(&bad), Err(PackError::BadLengthField));
    }

    #[test]
    fn row_ranges_match_borrowed_rows() {
        let file = pack_bytes();
        let pack = parse(&file).expect("valid pack");
        let header = pack.header();
        let rows = header.rows;
        for row in 0..rows {
            let ranges = header.row_ranges(row).expect("in-band row");
            let end = |r: ByteRange| r.end().expect("no overflow");
            assert_eq!(
                &file[ranges.rock.offset..end(ranges.rock)],
                pack.rock_row(row).expect("row")
            );
            assert_eq!(
                &file[ranges.snow.offset..end(ranges.snow)],
                pack.snow_row(row).expect("row")
            );
            assert_eq!(
                &file[ranges.barrier.offset..end(ranges.barrier)],
                pack.barrier_row(row).expect("row")
            );
            assert_eq!(
                &file[ranges.eligible.offset..end(ranges.eligible)],
                pack.eligible_row(row).expect("row")
            );
            assert_eq!(
                &file[ranges.noise.offset..end(ranges.noise)],
                pack.noise_row(row).expect("row")
            );
            assert_eq!(ranges.rock.len, WIDTH);
            assert_eq!(ranges.eligible.len, WIDTH / 8);
        }
        assert_eq!(header.row_ranges(rows), Err(PackError::BadRow));
        assert_eq!(header.row_ranges(usize::MAX), Err(PackError::BadRow));
    }

    #[test]
    fn incremental_crc_matches_oneshot() {
        let file = pack_bytes();
        let payload = &file[HEADER_LEN..];
        // Irregular chunk boundaries, including 1-byte steps.
        let mut crc = Crc32::new();
        let mut i = 0;
        for step in [1usize, 7, 64, 1000] {
            let end = (i + step).min(payload.len());
            crc.update(&payload[i..end]);
            i = end;
        }
        crc.update(&payload[i..]);
        assert_eq!(crc.finalize(), crc32_ieee(payload));
        assert_eq!(Crc32::new().finalize(), crc32_ieee(&[]));
        assert_eq!(Crc32::default().finalize(), 0);
    }

    #[test]
    fn full_row_composes_through_pack_rows() {
        use crate::composer::{compose_row, Cut, RowInputs};
        let file = pack_bytes();
        let pack = parse(&file).expect("valid pack");
        let inputs = RowInputs {
            rock: pack.rock_row(0).expect("row"),
            snow: pack.snow_row(0).expect("row"),
            barrier: pack.barrier_row(0).expect("row"),
            eligible: pack.eligible_row(0).expect("row"),
            noise: pack.noise_row(0).expect("row"),
        };
        let mut out = vec![0u8; WIDTH / 8];
        compose_row(
            &inputs,
            Cut {
                level: 0,
                frac: 256,
            },
            0,
            &mut out,
        )
        .expect("lengths agree");
        assert!(out.iter().all(|&b| b == 0xFF));
    }
}
