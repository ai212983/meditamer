//! Runtime sky/sun pack for Ambient Home (format version 1).
//!
//! The host packer (`tools/ambient_sky`) clips the dithered sun sprite to
//! its painted bounds and stores the full-frame sky plus the clipped sun
//! as MSB-first 1-bit planes (`1` = ink). The device expands them into
//! its L8 canvas at composition time; nothing here is pre-rendered for a
//! time position.
//!
//! Layout: a 64-byte header followed by the payload: sky ink bits
//! (`SKY_W` x `SKY_H` / 8 bytes), sun ink bits (`sun_stride` x `sun_h`
//! bytes), then sun mask bits (same length; `1` = opaque).
//!
//! Header: 8 magic bytes, `u16` sky_w, `u16` sky_h, `u16` sun_w, `u16`
//! sun_h, `f32` anchor_x, `f32` anchor_y (mask centroid in crop pixels,
//! all little-endian), `u32` sky_len, `u32` sun_len (ink bytes, equal to
//! mask bytes), `u32` IEEE CRC32 of the payload, then zero reserved bytes
//! to 64.
//!
//! `no_std`, no allocation, no large statics. All borrowed bytes are owned
//! by the caller. The CRC uses the same IEEE polynomial as `clock-assets`
//! (duplicated, not depended on, so this crate stands alone).

#![no_std]

#[cfg(test)]
extern crate std;

/// Header length in bytes.
pub const HEADER_LEN: usize = 64;
/// Sky geometry: the pack always covers the full 600x600 surface.
pub const SKY_W: usize = 600;
pub const SKY_H: usize = 600;
/// Sky ink bytes (600x600 bits, MSB-first, byte-aligned rows).
pub const SKY_LEN: usize = SKY_W * SKY_H / 8; // 45_000
/// Sun sprite bound: the host clip must fit (the canonical clip is
/// 183x133; the bound leaves headroom for re-exports, not for full
/// frames -- a full-frame "sprite" is a packer bug, rejected here).
pub const SUN_MAX: usize = 256;
/// Largest file this format can describe: header + sky + the biggest
/// sun ink and mask planes.
pub const MAX_FILE_LEN: usize = HEADER_LEN + SKY_LEN + 2 * (SUN_MAX * SUN_MAX / 8);

/// 8-byte ASCII magic identifying the ambient sky/sun format version 1.
pub const MAGIC: [u8; 8] = *b"AMBSKY01";

/// Codec and borrow errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// File slice is shorter than [`HEADER_LEN`] or longer than
    /// [`MAX_FILE_LEN`].
    BadFileLen,
    /// First 8 bytes are not [`MAGIC`].
    BadMagic,
    /// Sky dimensions are not 600x600.
    BadSkyGeometry,
    /// Sun dimensions are zero or exceed [`SUN_MAX`].
    BadSunGeometry,
    /// Sky/sun length fields disagree with the geometry.
    BadLengthField,
    /// A reserved byte is nonzero.
    BadReserved,
    /// Payload length disagrees with the file size.
    BadPayloadLen,
    /// Header CRC32 does not match the payload.
    BadChecksum,
}

/// Borrowed views over one decoded pack. Lifetimes tie to the caller's
/// buffer; this type owns nothing.
#[derive(Clone, Copy, Debug)]
pub struct AmbientAssets<'a> {
    payload: &'a [u8],
    sun_w: usize,
    sun_h: usize,
    sun_stride: usize,
    anchor_x: f32,
    anchor_y: f32,
    sky_len: usize,
    sun_len: usize,
}

impl<'a> AmbientAssets<'a> {
    /// Sky ink bits: exactly [`SKY_LEN`] bytes, MSB-first, `1` = ink.
    pub fn sky(&self) -> &'a [u8] {
        &self.payload[..self.sky_len]
    }

    /// Sun ink bits: `sun_stride` x `sun_h` bytes, MSB-first, `1` = ink.
    pub fn sun_ink(&self) -> &'a [u8] {
        &self.payload[self.sky_len..self.sky_len + self.sun_len]
    }

    /// Sun mask bits: same layout as ink, `1` = opaque.
    pub fn sun_mask(&self) -> &'a [u8] {
        &self.payload[self.sky_len + self.sun_len..]
    }

    /// Sun sprite width in pixels.
    pub fn sun_w(&self) -> usize {
        self.sun_w
    }

    /// Sun sprite height in pixels.
    pub fn sun_h(&self) -> usize {
        self.sun_h
    }

    /// Sun row stride in bytes.
    pub fn sun_stride(&self) -> usize {
        self.sun_stride
    }

    /// Mask centroid in crop pixels; the composer centers this on the
    /// trajectory point.
    pub fn anchor(&self) -> (f32, f32) {
        (self.anchor_x, self.anchor_y)
    }

    /// Raw payload (sky + sun ink + sun mask).
    pub fn payload(&self) -> &'a [u8] {
        self.payload
    }
}

/// IEEE CRC32 (polynomial 0xEDB88320) via a 16-entry nibble lookup.
/// Same digest as `clock_assets::crc32`; duplicated so this crate has no
/// dependency beyond `core`.
pub fn crc32(data: &[u8]) -> u32 {
    const TABLE: [u32; 16] = [
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
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= u32::from(b);
        crc = TABLE[(crc & 0xF) as usize] ^ (crc >> 4);
        crc = TABLE[(crc & 0xF) as usize] ^ (crc >> 4);
    }
    !crc
}

fn u16_le(bytes: &[u8]) -> usize {
    u16::from_le_bytes([bytes[0], bytes[1]]) as usize
}

fn u32_le(bytes: &[u8]) -> usize {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize
}

fn f32_le(bytes: &[u8]) -> f32 {
    f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

pub fn file_len_from_header(header: &[u8]) -> Result<usize, Error> {
    if header.len() < HEADER_LEN {
        return Err(Error::BadFileLen);
    }
    if header[0..8] != MAGIC {
        return Err(Error::BadMagic);
    }
    let sky_w = u16_le(&header[8..10]);
    let sky_h = u16_le(&header[10..12]);
    if sky_w != SKY_W || sky_h != SKY_H {
        return Err(Error::BadSkyGeometry);
    }
    let sun_w = u16_le(&header[12..14]);
    let sun_h = u16_le(&header[14..16]);
    if sun_w == 0 || sun_h == 0 || sun_w > SUN_MAX || sun_h > SUN_MAX {
        return Err(Error::BadSunGeometry);
    }
    let anchor_x = f32_le(&header[16..20]);
    let anchor_y = f32_le(&header[20..24]);
    if !anchor_x.is_finite() || !anchor_y.is_finite() {
        return Err(Error::BadSunGeometry);
    }
    let sky_len = u32_le(&header[24..28]);
    let sun_len = u32_le(&header[28..32]);
    let expect_sun = sun_w
        .div_ceil(8)
        .checked_mul(sun_h)
        .ok_or(Error::BadLengthField)?;
    if sky_len != SKY_LEN || sun_len != expect_sun {
        return Err(Error::BadLengthField);
    }
    if header[36..64].iter().any(|&b| b != 0) {
        return Err(Error::BadReserved);
    }
    let payload_len = sky_len
        .checked_add(sun_len.checked_mul(2).ok_or(Error::BadFileLen)?)
        .ok_or(Error::BadFileLen)?;
    let file_len = HEADER_LEN
        .checked_add(payload_len)
        .ok_or(Error::BadFileLen)?;
    if file_len > MAX_FILE_LEN {
        return Err(Error::BadFileLen);
    }
    Ok(file_len)
}

/// Validates the header, geometry, exact file size, reserved bytes, and
/// checksum, then borrows the planes. Rejects bad, truncated, or
/// over-long files before borrowing anything.
pub fn decode(bytes: &[u8]) -> Result<AmbientAssets<'_>, Error> {
    if bytes.len() < HEADER_LEN || bytes.len() > MAX_FILE_LEN {
        return Err(Error::BadFileLen);
    }
    let file_len = file_len_from_header(bytes)?;
    if bytes.len() != file_len {
        return Err(Error::BadPayloadLen);
    }
    let want = u32::from_le_bytes([bytes[32], bytes[33], bytes[34], bytes[35]]);
    let payload = &bytes[HEADER_LEN..];
    if crc32(payload) != want {
        return Err(Error::BadChecksum);
    }
    let sun_w = u16_le(&bytes[12..14]);
    let sun_h = u16_le(&bytes[14..16]);
    Ok(AmbientAssets {
        payload,
        sun_w,
        sun_h,
        sun_stride: sun_w.div_ceil(8),
        anchor_x: f32_le(&bytes[16..20]),
        anchor_y: f32_le(&bytes[20..24]),
        sky_len: u32_le(&bytes[24..28]),
        sun_len: u32_le(&bytes[28..32]),
    })
}
