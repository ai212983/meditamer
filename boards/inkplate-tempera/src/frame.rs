//! Panel-native still frames: a small container format plus the transforms
//! between a natural row-major raster and this panel's own axes and packings.
//!
//! Lives in the board crate because every fact encoded here is a property of
//! *this display*, not of any application: the rotation, the two pixel
//! packings, and the grayscale polarity. Each of them corrupts an image
//! silently rather than raising an error when it is wrong, which is why they
//! are written down once, here, with the evidence.
//!
//! ## Container
//!
//! A 16-byte header followed by a raw panel buffer:
//!
//! ```text
//! 0..4   b"MDFR"
//! 4      version = 1
//! 5      format: 0 = binary (1 bpp), 1 = Gray4 (4 bpp)
//! 6..8   width  u16 LE
//! 8..10  height u16 LE
//! 10..16 reserved
//! ```
//!
//! ## Packings
//!
//! These are the **authoring** conventions -- how an image is laid out in a
//! file, before rotation. They are row-major, and match
//! `targets/meditamer-inkplate/src/panel_waveform_fixture.rs`.
//!
//! They are deliberately *not* the panel's own framebuffer organisation. The
//! panel buffer is **column-major with Y inverted** -- see
//! [`crate::panel_blit::blit_l8`], which is the shipping UI's writer and
//! therefore the authority on it. Authoring row-major and then applying
//! [`rotate_binary`] / [`rotate_gray4`] composes to that same organisation;
//! `panel_blit`'s tests pin the two against each other. Code that writes the
//! panel buffer directly should follow `panel_blit`, not this section.
//!
//! - **Binary**: `index = y * 75 + x / 8`, bit `1 << (x % 8)` -- **LSB-first**
//!   within the byte. Note this is the *opposite* of the Waveshare ST7305
//!   convention, so the two boards must never share packing code.
//! - **Gray4**: `index = y * 300 + x / 2`, two pixels per byte with the
//!   **high nibble holding the even (left) x**. The panel's eight physical
//!   levels occupy the **even** values of the 4-bit field, i.e. `level * 2`.
//!
//! ## Polarity
//!
//! A **higher** Gray4 value is **lighter**: the white ground is level 7
//! (nibble 14) and full black is 0. Undocumented in the datasheet; determined
//! by rendering both polarities and looking at the panel. Reversing it yields
//! a clean photographic negative.
//!
//! ## Orientation
//!
//! The panel's axes sit 90 degrees counter-clockwise from a natural raster,
//! so images are authored upright and rotated clockwise on the way into the
//! panel buffer: `dest(dx, dy)` samples `src(dy, HEIGHT - 1 - dx)`. The panel
//! is square, so dimensions are unchanged.

use crate::{E_INK_HEIGHT, E_INK_WIDTH, FRAMEBUFFER_BYTES, GRAYSCALE_FRAMEBUFFER_BYTES};

pub const HEADER_LEN: usize = 16;
pub const MAGIC: [u8; 4] = *b"MDFR";
pub const VERSION: u8 = 1;

/// Bytes per row in each packing.
pub const BINARY_ROW_BYTES: usize = E_INK_WIDTH / 8;
pub const GRAY4_ROW_BYTES: usize = E_INK_WIDTH / 2;

/// The lightest and darkest values a Gray4 nibble takes. Higher is lighter.
pub const GRAY4_WHITE: u8 = 14;
pub const GRAY4_BLACK: u8 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Binary,
    Gray4,
}

impl Format {
    pub const fn payload_len(self) -> usize {
        match self {
            Self::Binary => FRAMEBUFFER_BYTES,
            Self::Gray4 => GRAYSCALE_FRAMEBUFFER_BYTES,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub format: Format,
    pub width: u16,
    pub height: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderError {
    TooShort,
    BadMagic,
    UnsupportedVersion(u8),
    UnknownFormat(u8),
    /// Dimensions that do not match this panel. Carried so a caller can say
    /// what it received rather than just refusing.
    WrongDimensions {
        width: u16,
        height: u16,
    },
}

impl Header {
    pub fn parse(bytes: &[u8]) -> Result<Self, HeaderError> {
        if bytes.len() < HEADER_LEN {
            return Err(HeaderError::TooShort);
        }
        if bytes[0..4] != MAGIC {
            return Err(HeaderError::BadMagic);
        }
        if bytes[4] != VERSION {
            return Err(HeaderError::UnsupportedVersion(bytes[4]));
        }
        let format = match bytes[5] {
            0 => Format::Binary,
            1 => Format::Gray4,
            other => return Err(HeaderError::UnknownFormat(other)),
        };
        let width = u16::from_le_bytes([bytes[6], bytes[7]]);
        let height = u16::from_le_bytes([bytes[8], bytes[9]]);
        if usize::from(width) != E_INK_WIDTH || usize::from(height) != E_INK_HEIGHT {
            return Err(HeaderError::WrongDimensions { width, height });
        }
        Ok(Self {
            format,
            width,
            height,
        })
    }

    pub const fn payload_len(&self) -> usize {
        self.format.payload_len()
    }
}

/// Rotate a binary frame clockwise into the panel's axes.
///
/// `src` and `dest` are both [`FRAMEBUFFER_BYTES`]; shorter slices are
/// rejected rather than panicking part-way through a frame.
pub fn rotate_binary(src: &[u8], dest: &mut [u8]) -> bool {
    if src.len() < FRAMEBUFFER_BYTES || dest.len() < FRAMEBUFFER_BYTES {
        return false;
    }
    dest[..FRAMEBUFFER_BYTES].fill(0);
    for dy in 0..E_INK_HEIGHT {
        for dx in 0..E_INK_WIDTH {
            let sx = dy;
            let sy = E_INK_HEIGHT - 1 - dx;
            if src[sy * BINARY_ROW_BYTES + (sx >> 3)] & (1 << (sx % 8)) != 0 {
                dest[dy * BINARY_ROW_BYTES + (dx >> 3)] |= 1 << (dx % 8);
            }
        }
    }
    true
}

/// Rotate a Gray4 frame clockwise into the panel's axes.
pub fn rotate_gray4(src: &[u8], dest: &mut [u8]) -> bool {
    if src.len() < GRAYSCALE_FRAMEBUFFER_BYTES || dest.len() < GRAYSCALE_FRAMEBUFFER_BYTES {
        return false;
    }
    dest[..GRAYSCALE_FRAMEBUFFER_BYTES].fill(0);
    for dy in 0..E_INK_HEIGHT {
        for dx in 0..E_INK_WIDTH {
            let sx = dy;
            let sy = E_INK_HEIGHT - 1 - dx;
            let byte = src[sy * GRAY4_ROW_BYTES + sx / 2];
            let level = if sx % 2 == 0 { byte >> 4 } else { byte & 0x0f };
            let slot = &mut dest[dy * GRAY4_ROW_BYTES + dx / 2];
            if dx % 2 == 0 {
                *slot = (*slot & 0x0f) | (level << 4);
            } else {
                *slot = (*slot & 0xf0) | level;
            }
        }
    }
    true
}

/// Map an 8-bit luminance (0 = black, 255 = white, as LVGL renders it) onto
/// the panel's eight physical levels. Higher is lighter, and the levels sit
/// on the even values of the nibble.
///
/// Quantisation is linear here. E-paper levels are not perceptually evenly
/// spaced, so a measured response curve belongs in this function once one
/// exists -- see `docs/plans/font-legibility-improvements.md`.
#[inline]
pub const fn level_from_luminance(luminance: u8) -> u8 {
    // 0..255 -> 0..7, rounded, then doubled onto the even values.
    (((luminance as u16 * 7) + 127) / 255) as u8 * 2
}

/// Write one Gray4 pixel into a panel buffer.
#[inline]
pub fn set_gray4(framebuffer: &mut [u8], x: usize, y: usize, level: u8) {
    if x >= E_INK_WIDTH || y >= E_INK_HEIGHT {
        return;
    }
    let slot = &mut framebuffer[y * GRAY4_ROW_BYTES + x / 2];
    if x.is_multiple_of(2) {
        *slot = (*slot & 0x0f) | ((level & 0x0f) << 4);
    } else {
        *slot = (*slot & 0xf0) | (level & 0x0f);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_bytes(format: u8, w: u16, h: u16) -> [u8; HEADER_LEN] {
        let mut b = [0u8; HEADER_LEN];
        b[0..4].copy_from_slice(&MAGIC);
        b[4] = VERSION;
        b[5] = format;
        b[6..8].copy_from_slice(&w.to_le_bytes());
        b[8..10].copy_from_slice(&h.to_le_bytes());
        b
    }

    #[test]
    fn parses_both_formats() {
        let bw = Header::parse(&header_bytes(0, 600, 600)).unwrap();
        assert_eq!(bw.format, Format::Binary);
        assert_eq!(bw.payload_len(), FRAMEBUFFER_BYTES);
        let g4 = Header::parse(&header_bytes(1, 600, 600)).unwrap();
        assert_eq!(g4.format, Format::Gray4);
        assert_eq!(g4.payload_len(), GRAYSCALE_FRAMEBUFFER_BYTES);
    }

    #[test]
    fn rejects_malformed_headers() {
        assert_eq!(Header::parse(&[]), Err(HeaderError::TooShort));
        let mut bad = header_bytes(0, 600, 600);
        bad[0] = b'X';
        assert_eq!(Header::parse(&bad), Err(HeaderError::BadMagic));
        let mut ver = header_bytes(0, 600, 600);
        ver[4] = 9;
        assert_eq!(Header::parse(&ver), Err(HeaderError::UnsupportedVersion(9)));
        let mut fmt = header_bytes(7, 600, 600);
        fmt[5] = 7;
        assert_eq!(Header::parse(&fmt), Err(HeaderError::UnknownFormat(7)));
        assert_eq!(
            Header::parse(&header_bytes(0, 400, 300)),
            Err(HeaderError::WrongDimensions {
                width: 400,
                height: 300
            })
        );
    }

    #[test]
    fn luminance_maps_onto_even_levels_lighter_is_higher() {
        assert_eq!(level_from_luminance(0), GRAY4_BLACK);
        assert_eq!(level_from_luminance(255), GRAY4_WHITE);
        for l in 0..=255u8 {
            let v = level_from_luminance(l);
            assert!(v <= GRAY4_WHITE, "level {v} out of range for {l}");
            assert_eq!(v % 2, 0, "level {v} is not an even value");
        }
        // monotonic: lighter input never produces a darker level
        let mut prev = 0;
        for l in 0..=255u8 {
            let v = level_from_luminance(l);
            assert!(v >= prev);
            prev = v;
        }
    }

    /// Rotating clockwise four times is the identity, which pins the
    /// direction without needing the panel.
    #[test]
    fn binary_rotation_has_order_four() {
        let mut a = [0u8; FRAMEBUFFER_BYTES];
        // an asymmetric mark, so a mirrored rotation would not survive
        a[0] = 0b0000_0001;
        a[BINARY_ROW_BYTES * 5 + 2] = 0b0011_0000;
        let mut b = [0u8; FRAMEBUFFER_BYTES];
        let mut c = [0u8; FRAMEBUFFER_BYTES];
        assert!(rotate_binary(&a, &mut b));
        assert!(rotate_binary(&b, &mut c));
        assert!(rotate_binary(&c, &mut b));
        assert!(rotate_binary(&b, &mut c));
        assert_eq!(&a[..], &c[..]);
    }

    #[test]
    fn gray4_rotation_has_order_four() {
        let mut a = [0u8; GRAYSCALE_FRAMEBUFFER_BYTES];
        a[0] = 0xE0;
        a[GRAY4_ROW_BYTES * 3 + 7] = 0x0A;
        let mut b = [0u8; GRAYSCALE_FRAMEBUFFER_BYTES];
        let mut c = [0u8; GRAYSCALE_FRAMEBUFFER_BYTES];
        assert!(rotate_gray4(&a, &mut b));
        assert!(rotate_gray4(&b, &mut c));
        assert!(rotate_gray4(&c, &mut b));
        assert!(rotate_gray4(&b, &mut c));
        assert_eq!(&a[..], &c[..]);
    }

    #[test]
    fn rotation_rejects_short_slices() {
        let src = [0u8; 8];
        let mut dest = [0u8; FRAMEBUFFER_BYTES];
        assert!(!rotate_binary(&src, &mut dest));
        assert!(!rotate_gray4(&src, &mut dest));
    }

    #[test]
    fn set_gray4_writes_the_right_nibble() {
        let mut fb = [0u8; GRAYSCALE_FRAMEBUFFER_BYTES];
        set_gray4(&mut fb, 0, 0, GRAY4_WHITE);
        assert_eq!(fb[0], 0xE0, "even x must land in the high nibble");
        set_gray4(&mut fb, 1, 0, 3);
        assert_eq!(fb[0], 0xE3, "odd x must land in the low nibble");
        set_gray4(&mut fb, E_INK_WIDTH, 0, GRAY4_WHITE); // out of range, ignored
        assert_eq!(fb[0], 0xE3);
    }
}
