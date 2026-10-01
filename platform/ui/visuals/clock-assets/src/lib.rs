//! Runtime source-map pack for the analog clock (600-map format, version 1).
//!
//! The device rotates and lights the original PNG normal/spec sprites at
//! runtime from these borrowed source maps. Nothing here is pre-rendered
//! for a dial position, downscaled, renormalized, or gamma-adjusted.
//!
//! Layout: a 32-byte header followed by the fixed payload in canonical
//! order: dial gray 600x600; hour albedo gray 245x810, hour alpha 245x810,
//! hour normal RGB 3x245x810, hour spec 245x810; minute albedo gray
//! 156x1014, minute alpha 156x1014, minute normal RGB 3x156x1014, minute
//! spec 156x1014. Diffuse albedo stores one gray channel: the canonical
//! source has all three RGB channels identical, and the host exporter
//! rejects non-grayscale input instead of silently dropping color.
//!
//! `no_std`, no allocation, no large statics. All borrowed bytes are owned
//! by the caller. The CRC is an IEEE CRC32 with a 16-entry nibble lookup over the
//! payload, computed once per SD transfer; no device timing is
//! claimed here.

#![no_std]

#[cfg(test)]
extern crate std;

/// Header length in bytes: 8 magic + 4 payload length + 4 CRC32 + 16 reserved.
pub const HEADER_LEN: usize = 32;
/// Payload length in bytes (concatenated canonical maps).
pub const PAYLOAD_LEN: usize = 2_499_804;
/// Total file length in bytes (header + payload).
pub const FILE_LEN: usize = HEADER_LEN + PAYLOAD_LEN;

/// 8-byte ASCII magic identifying the 600-map format version 1.
pub const MAGIC: [u8; 8] = *b"MCLKMAP1";

/// Canonical geometry.
pub const DIAL_W: usize = 600;
pub const DIAL_H: usize = 600;
pub const HOUR_W: usize = 245;
pub const HOUR_H: usize = 810;
pub const MINUTE_W: usize = 156;
pub const MINUTE_H: usize = 1014;

/// Canonical byte lengths of each payload segment.
pub const DIAL_LEN: usize = DIAL_W * DIAL_H; // 360_000
pub const HOUR_PIXELS: usize = HOUR_W * HOUR_H; // 198_450
pub const HOUR_NORMAL_LEN: usize = HOUR_PIXELS * 3; // 595_350
pub const MINUTE_PIXELS: usize = MINUTE_W * MINUTE_H; // 158_184
pub const MINUTE_NORMAL_LEN: usize = MINUTE_PIXELS * 3; // 474_552

/// Byte offset of each payload segment from the payload start.
pub const OFF_DIAL: usize = 0;
pub const OFF_HOUR_ALBEDO: usize = OFF_DIAL + DIAL_LEN;
pub const OFF_HOUR_ALPHA: usize = OFF_HOUR_ALBEDO + HOUR_PIXELS;
pub const OFF_HOUR_NORMAL: usize = OFF_HOUR_ALPHA + HOUR_PIXELS;
pub const OFF_HOUR_SPEC: usize = OFF_HOUR_NORMAL + HOUR_NORMAL_LEN;
pub const OFF_MINUTE_ALBEDO: usize = OFF_HOUR_SPEC + HOUR_PIXELS;
pub const OFF_MINUTE_ALPHA: usize = OFF_MINUTE_ALBEDO + MINUTE_PIXELS;
pub const OFF_MINUTE_NORMAL: usize = OFF_MINUTE_ALPHA + MINUTE_PIXELS;
pub const OFF_MINUTE_SPEC: usize = OFF_MINUTE_NORMAL + MINUTE_NORMAL_LEN;
/// The nine independently allocated source maps in file order.
pub const MAP_OFFSETS: [usize; 9] = [
    OFF_DIAL,
    OFF_HOUR_ALBEDO,
    OFF_HOUR_ALPHA,
    OFF_HOUR_NORMAL,
    OFF_HOUR_SPEC,
    OFF_MINUTE_ALBEDO,
    OFF_MINUTE_ALPHA,
    OFF_MINUTE_NORMAL,
    OFF_MINUTE_SPEC,
];
pub const MAP_LENGTHS: [usize; 9] = [
    DIAL_LEN,
    HOUR_PIXELS,
    HOUR_PIXELS,
    HOUR_NORMAL_LEN,
    HOUR_PIXELS,
    MINUTE_PIXELS,
    MINUTE_PIXELS,
    MINUTE_NORMAL_LEN,
    MINUTE_PIXELS,
];

/// Codec and borrow errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Payload slice is not exactly [`PAYLOAD_LEN`] bytes.
    BadPayloadLen,
    /// File slice is not exactly [`FILE_LEN`] bytes.
    BadFileLen,
    /// First 8 bytes are not [`MAGIC`].
    BadMagic,
    /// Header payload-length field does not equal [`PAYLOAD_LEN`].
    BadLengthField,
    /// One of the reserved bytes 16..32 is nonzero.
    BadReserved,
    /// Header CRC32 does not match the payload.
    BadChecksum,
}

/// Borrowed views over one decoded pack. Lifetimes tie to the caller's
/// buffer; this type owns nothing.
#[derive(Clone, Copy, Debug)]
pub struct ClockAssets<'a> {
    payload: &'a [u8],
}

/// Borrowed views over the nine independently allocated source maps.
/// The caller validates the stream header and CRC before exposing these.
pub struct ClockAssetParts<'a> {
    maps: [&'a [u8]; 9],
}

impl<'a> ClockAssetParts<'a> {
    pub fn borrow_validated(maps: [&'a [u8]; 9]) -> Result<Self, Error> {
        if maps
            .iter()
            .zip(MAP_LENGTHS)
            .any(|(map, len)| map.len() != len)
        {
            return Err(Error::BadPayloadLen);
        }
        Ok(Self { maps })
    }

    pub fn dial(&self) -> analog_clock::DialMap<'a> {
        analog_clock::DialMap {
            width: DIAL_W as u32,
            height: DIAL_H as u32,
            pixels: self.maps[0],
        }
    }

    pub fn hands(&self) -> analog_clock::Hands<'a> {
        let p = &self.maps;
        analog_clock::Hands {
            hour: analog_clock::HandMaps {
                width: HOUR_W as u16,
                height: HOUR_H as u16,
                albedo: p[1],
                alpha: p[2],
                normal: p[3],
                spec: p[4],
                pivot_x: analog_clock::HOUR_PIVOT.0,
                pivot_y: analog_clock::HOUR_PIVOT.1,
            },
            minute: analog_clock::HandMaps {
                width: MINUTE_W as u16,
                height: MINUTE_H as u16,
                albedo: p[5],
                alpha: p[6],
                normal: p[7],
                spec: p[8],
                pivot_x: analog_clock::MINUTE_PIVOT.0,
                pivot_y: analog_clock::MINUTE_PIVOT.1,
            },
        }
    }
}

impl<'a> ClockAssets<'a> {
    /// Borrow the two hands with canonical pivots. Albedo slices are the
    /// single stored gray channel (one byte per pixel); alpha/spec likewise
    /// one byte per pixel, normals tightly packed RGB rows.
    pub fn hands(&self) -> analog_clock::Hands<'a> {
        let p = self.payload;
        analog_clock::Hands {
            hour: analog_clock::HandMaps {
                width: HOUR_W as u16,
                height: HOUR_H as u16,
                albedo: &p[OFF_HOUR_ALBEDO..OFF_HOUR_ALPHA],
                alpha: &p[OFF_HOUR_ALPHA..OFF_HOUR_NORMAL],
                normal: &p[OFF_HOUR_NORMAL..OFF_HOUR_SPEC],
                spec: &p[OFF_HOUR_SPEC..OFF_MINUTE_ALBEDO],
                pivot_x: analog_clock::HOUR_PIVOT.0,
                pivot_y: analog_clock::HOUR_PIVOT.1,
            },
            minute: analog_clock::HandMaps {
                width: MINUTE_W as u16,
                height: MINUTE_H as u16,
                albedo: &p[OFF_MINUTE_ALBEDO..OFF_MINUTE_ALPHA],
                alpha: &p[OFF_MINUTE_ALPHA..OFF_MINUTE_NORMAL],
                normal: &p[OFF_MINUTE_NORMAL..OFF_MINUTE_SPEC],
                spec: &p[OFF_MINUTE_SPEC..PAYLOAD_LEN],
                pivot_x: analog_clock::MINUTE_PIVOT.0,
                pivot_y: analog_clock::MINUTE_PIVOT.1,
            },
        }
    }

    /// Borrow the dial as one gray byte per pixel, row major.
    pub fn dial(&self) -> analog_clock::DialMap<'a> {
        analog_clock::DialMap {
            width: DIAL_W as u32,
            height: DIAL_H as u32,
            pixels: &self.payload[OFF_DIAL..OFF_HOUR_ALBEDO],
        }
    }

    /// Raw payload (exactly [`PAYLOAD_LEN`] bytes).
    pub fn payload(&self) -> &'a [u8] {
        self.payload
    }
}

/// IEEE CRC32 (polynomial 0xEDB88320) via a 16-entry nibble lookup.
///
/// The 64-byte lookup preserves the bitwise routine's digest and requires
/// no allocation. Each byte is processed as two four-bit nibbles.
const CRC_TABLE: [u32; 16] = [
    0x0000_0000,
    0x1DB7_1064,
    0x3B6E_20C8,
    0x26D9_30AC,
    0x76DC_4190,
    0x6B6B_51F4,
    0x4DB2_6158,
    0x5005_713C,
    0xEDB8_8320,
    0xF00F_9344,
    0xD6D6_A3E8,
    0xCB61_B38C,
    0x9B64_C2B0,
    0x86D3_D2D4,
    0xA00A_E278,
    0xBDBD_F21C,
];

/// Incremental digest for a sector stream or several PSRAM regions.
pub struct Crc32(u32);

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

impl Crc32 {
    pub const fn new() -> Self {
        Self(0xFFFF_FFFF)
    }
    pub fn update(&mut self, data: &[u8]) {
        for &b in data {
            self.0 ^= u32::from(b);
            self.0 = CRC_TABLE[(self.0 & 0xF) as usize] ^ (self.0 >> 4);
            self.0 = CRC_TABLE[(self.0 & 0xF) as usize] ^ (self.0 >> 4);
        }
    }
    pub const fn finish(self) -> u32 {
        !self.0
    }
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = Crc32::new();
    crc.update(data);
    crc.finish()
}

/// Encodes the 32-byte header for `payload`, which must be exactly
/// [`PAYLOAD_LEN`] bytes: magic, LE payload length, IEEE CRC32 of the
/// payload, then 16 zero reserved bytes.
pub fn encode_header(payload: &[u8]) -> Result<[u8; HEADER_LEN], Error> {
    if payload.len() != PAYLOAD_LEN {
        return Err(Error::BadPayloadLen);
    }
    let mut header = [0u8; HEADER_LEN];
    header[0..8].copy_from_slice(&MAGIC);
    header[8..12].copy_from_slice(&(PAYLOAD_LEN as u32).to_le_bytes());
    header[12..16].copy_from_slice(&crc32(payload).to_le_bytes());
    Ok(header)
}

/// Validates the header, exact file size, reserved bytes, and checksum,
/// then borrows the payload. Rejects bad, truncated, or over-long files
/// before borrowing anything.
pub fn decode(bytes: &[u8]) -> Result<ClockAssets<'_>, Error> {
    let assets = decode_validated(bytes)?;
    let want = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    if crc32(assets.payload) != want {
        return Err(Error::BadChecksum);
    }
    Ok(assets)
}

/// Borrow a pack already checksum-validated by its owner. This still checks
/// its shape and header, but avoids hashing the full pack on each render tick.
pub fn decode_validated(bytes: &[u8]) -> Result<ClockAssets<'_>, Error> {
    if bytes.len() != FILE_LEN {
        return Err(Error::BadFileLen);
    }
    let header: &[u8; HEADER_LEN] = bytes[..HEADER_LEN]
        .try_into()
        .map_err(|_| Error::BadFileLen)?;
    validate_header(header)?;
    let payload = &bytes[HEADER_LEN..];
    Ok(ClockAssets { payload })
}

/// Validate a streamed file header and return its expected payload CRC.
pub fn validate_header(bytes: &[u8; HEADER_LEN]) -> Result<u32, Error> {
    if bytes[0..8] != MAGIC {
        return Err(Error::BadMagic);
    }
    let len = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    if len != PAYLOAD_LEN {
        return Err(Error::BadLengthField);
    }
    if bytes[16..32].iter().any(|&b| b != 0) {
        return Err(Error::BadReserved);
    }
    Ok(u32::from_le_bytes([
        bytes[12], bytes[13], bytes[14], bytes[15],
    ]))
}

/// Validate a streamed pack held in nine map regions, including its CRC.
pub fn decode_parts<'a>(
    header: &[u8; HEADER_LEN],
    maps: [&'a [u8]; 9],
) -> Result<ClockAssetParts<'a>, Error> {
    let expected = validate_header(header)?;
    let parts = ClockAssetParts::borrow_validated(maps)?;
    let mut crc = Crc32::new();
    for map in maps {
        crc.update(map);
    }
    if crc.finish() != expected {
        return Err(Error::BadChecksum);
    }
    Ok(parts)
}

/// Route one file-stream chunk into the nine canonical PSRAM regions.
/// Chunks may cross the header or region boundaries.
pub fn copy_stream_chunk(
    header: &mut [u8; HEADER_LEN],
    maps: &mut [&mut [u8]; 9],
    offset: usize,
    bytes: &[u8],
) -> Result<(), Error> {
    if maps
        .iter()
        .zip(MAP_LENGTHS)
        .any(|(map, len)| map.len() != len)
    {
        return Err(Error::BadPayloadLen);
    }
    if offset
        .checked_add(bytes.len())
        .is_none_or(|end| end > FILE_LEN)
    {
        return Err(Error::BadFileLen);
    }
    fn copy(dst: &mut [u8], region_start: usize, offset: usize, src: &[u8]) {
        let lo = offset.max(region_start);
        let hi = (offset + src.len()).min(region_start + dst.len());
        if lo < hi {
            dst[lo - region_start..hi - region_start]
                .copy_from_slice(&src[lo - offset..hi - offset]);
        }
    }
    copy(header, 0, offset, bytes);
    for (index, map) in maps.iter_mut().enumerate() {
        copy(map, HEADER_LEN + MAP_OFFSETS[index], offset, bytes);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segmented_pack_matches_whole_decode_and_rejects_corruption() {
        let mut payload = std::vec![0u8; PAYLOAD_LEN];
        payload[0] = 17;
        payload[OFF_HOUR_NORMAL + 7] = 29;
        payload[OFF_MINUTE_SPEC + 11] = 43;
        let header = encode_header(&payload).unwrap();
        let maps =
            core::array::from_fn(|i| &payload[MAP_OFFSETS[i]..MAP_OFFSETS[i] + MAP_LENGTHS[i]]);
        let parts = decode_parts(&header, maps).unwrap();
        let mut whole = std::vec::Vec::from(header);
        whole.extend_from_slice(&payload);
        let flat = decode(&whole).unwrap();
        assert_eq!(parts.dial().pixels, flat.dial().pixels);
        assert_eq!(parts.hands().hour.normal, flat.hands().hour.normal);
        assert_eq!(parts.hands().minute.spec, flat.hands().minute.spec);
        payload[OFF_MINUTE_SPEC + 11] ^= 1;
        let maps =
            core::array::from_fn(|i| &payload[MAP_OFFSETS[i]..MAP_OFFSETS[i] + MAP_LENGTHS[i]]);
        assert!(matches!(
            decode_parts(&header, maps),
            Err(Error::BadChecksum)
        ));
    }

    #[test]
    fn streamed_chunks_cross_all_region_boundaries() {
        let mut payload = std::vec![0u8; PAYLOAD_LEN];
        for (index, byte) in payload.iter_mut().enumerate() {
            *byte = index as u8;
        }
        let header = encode_header(&payload).unwrap();
        let mut flat = std::vec::Vec::from(header);
        flat.extend_from_slice(&payload);
        let mut streamed_header = [0u8; HEADER_LEN];
        let mut maps: [std::vec::Vec<u8>; 9] =
            core::array::from_fn(|i| std::vec![0u8; MAP_LENGTHS[i]]);
        for (i, chunk) in flat.chunks(509).enumerate() {
            let [a, b, c, d, e, f, g, h, j] = &mut maps;
            let mut slices = [
                a.as_mut_slice(),
                b.as_mut_slice(),
                c.as_mut_slice(),
                d.as_mut_slice(),
                e.as_mut_slice(),
                f.as_mut_slice(),
                g.as_mut_slice(),
                h.as_mut_slice(),
                j.as_mut_slice(),
            ];
            copy_stream_chunk(&mut streamed_header, &mut slices, i * 509, chunk).unwrap();
        }
        assert_eq!(streamed_header, header);
        let slices = core::array::from_fn(|i| maps[i].as_slice());
        assert!(decode_parts(&streamed_header, slices).is_ok());
        for i in 0..9 {
            assert_eq!(
                maps[i],
                payload[MAP_OFFSETS[i]..MAP_OFFSETS[i] + MAP_LENGTHS[i]]
            );
        }
    }
}
