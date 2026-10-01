//! Retained indexed canvases with byte-aligned rows and checked palettes.

#![allow(unsafe_code)]

use core::{cell::UnsafeCell, mem::MaybeUninit};
use lightvgl_sys as lv;

use super::{AccessFault, UiAccessToken};

/// Palette entry used by a packed monochrome canvas (zero = ink, one = paper).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MonochromePixel {
    Ink,
    Paper,
}

impl MonochromePixel {
    const fn byte(self) -> u8 {
        match self {
            Self::Ink => 0,
            Self::Paper => 0xff,
        }
    }
}

/// BGRA palette storage, matching LVGL's indexed image format.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaletteColor {
    blue: u8,
    green: u8,
    red: u8,
    alpha: u8,
}

impl PaletteColor {
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self::rgba(red, green, blue, 255)
    }

    pub const fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            blue,
            green,
            red,
            alpha,
        }
    }
}

const fn palette_bytes(bits: usize) -> usize {
    assert!(
        matches!(bits, 1 | 2 | 4 | 8),
        "LVGL supports indexed 1/2/4/8-bit pixels"
    );
    (1 << bits) * 4
}

const fn stride(width: usize, bits: usize) -> usize {
    let _ = palette_bytes(bits);
    assert!(
        width > 0 && width <= u16::MAX as usize,
        "invalid indexed canvas width"
    );
    let alignment = lv::LV_DRAW_BUF_STRIDE_ALIGN as usize;
    assert!(alignment > 0, "invalid LVGL stride alignment");
    let bytes = (width * bits).div_ceil(8);
    let stride = bytes.div_ceil(alignment) * alignment;
    assert!(
        stride <= u16::MAX as usize,
        "indexed stride exceeds LVGL header"
    );
    stride
}

/// Required static bytes, including the full LVGL palette and configured row
/// padding. Panics for unsupported bit depths or geometry outside LVGL's header.
/// Eight shades use `bits = 4` with eight supplied palette entries.
pub const fn indexed_canvas_buffer_bytes(width: usize, height: usize, bits: usize) -> usize {
    assert!(
        height > 0 && height <= u16::MAX as usize,
        "invalid indexed canvas height"
    );
    let bytes = stride(width, bits) * height + palette_bytes(bits);
    assert!(
        bytes <= u32::MAX as usize,
        "indexed storage exceeds LVGL size"
    );
    bytes
}

pub const fn i1_canvas_buffer_bytes(width: usize, height: usize) -> usize {
    indexed_canvas_buffer_bytes(width, height, 1)
}

#[repr(C, align(4))]
struct AlignedBytes<const N: usize>([u8; N]);

/// Static indexed pixels and descriptor retained by LVGL without copying.
///
/// Supports 1, 2, 4, or 8 bits per pixel. Each row starts on a byte boundary;
/// the leftmost pixel occupies its byte's highest bits. A BGRA palette precedes
/// the pixels. Unused palette entries are initialized to the first color and
/// cannot be selected through the writer. Use a distinct buffer for each
/// independently painted, concurrently live surface.
///
/// Writes require the current UI token and must be followed by widget
/// invalidation. No heap allocation or reference to the pixel bytes is exposed.
/// LVGL's software decoder uses a temporary ARGB8888 row when drawing to L8;
/// the target must enable ARGB8888 source blending and keep RAM_LOAD disabled.
pub struct StaticIndexedCanvasBuffer<const BITS: usize, const N: usize> {
    bytes: UnsafeCell<AlignedBytes<N>>,
    descriptor: UnsafeCell<MaybeUninit<lv::lv_draw_buf_t>>,
    width: usize,
    height: usize,
    palette_len: usize,
}

impl<const BITS: usize, const N: usize> StaticIndexedCanvasBuffer<BITS, N> {
    pub const fn new(width: usize, height: usize, palette: &[PaletteColor], initial: u8) -> Self {
        assert!(
            N == indexed_canvas_buffer_bytes(width, height, BITS),
            "incorrect indexed capacity"
        );
        assert!(
            !palette.is_empty() && palette.len() <= (1 << BITS),
            "invalid palette length"
        );
        assert!(
            (initial as usize) < palette.len(),
            "initial pixel is outside palette"
        );
        const {
            assert!(
                lv::LV_DRAW_BUF_ALIGN > 0 && 4 % lv::LV_DRAW_BUF_ALIGN == 0,
                "unsupported LVGL buffer alignment"
            );
        }
        let mut bytes = [repeat_index(initial, BITS); N];
        let mut index = 0;
        while index < (1 << BITS) {
            let color = if index < palette.len() {
                palette[index]
            } else {
                palette[0]
            };
            bytes[index * 4] = color.blue;
            bytes[index * 4 + 1] = color.green;
            bytes[index * 4 + 2] = color.red;
            bytes[index * 4 + 3] = color.alpha;
            index += 1;
        }
        Self {
            bytes: UnsafeCell::new(AlignedBytes(bytes)),
            descriptor: UnsafeCell::new(MaybeUninit::uninit()),
            width,
            height,
            palette_len: palette.len(),
        }
    }

    pub const fn width(&self) -> usize {
        self.width
    }
    pub const fn height(&self) -> usize {
        self.height
    }
    pub const fn stride_bytes(&self) -> usize {
        stride(self.width, BITS)
    }
    /// Palette plus padded pixel rows; excludes descriptor/geometry metadata.
    pub const fn capacity_bytes(&self) -> usize {
        N
    }

    /// Grants closure-scoped writes without borrowing bytes retained by C.
    pub fn with_writer<R>(
        &'static self,
        token: &UiAccessToken,
        update: impl FnOnce(&mut IndexedCanvasWriter<'_, BITS, N>) -> R,
    ) -> Result<R, AccessFault> {
        token.check_current()?;
        Ok(update(&mut IndexedCanvasWriter {
            storage: self,
            token,
        }))
    }

    pub(super) fn as_mut_ptr(&'static self) -> *mut u8 {
        self.bytes.get().cast::<u8>()
    }

    pub(super) fn descriptor_ptr(&'static self) -> *mut lv::lv_draw_buf_t {
        self.descriptor.get().cast()
    }
}

// SAFETY: pixel writes and descriptor initialization require the sole UI
// capability. Neither storage nor references into it escape to safe callers.
// LVGL reads on the same UI owner, between completed writer operations.
unsafe impl<const BITS: usize, const N: usize> Sync for StaticIndexedCanvasBuffer<BITS, N> {}

const fn repeat_index(index: u8, bits: usize) -> u8 {
    let mut byte = 0;
    let mut shift = 0;
    while shift < 8 {
        byte |= index << shift;
        shift += bits;
    }
    byte
}

/// Token-authorized writes to indexed pixels, excluding palette and padding.
/// No slice is exposed, so callers may re-enter LVGL between writer calls.
/// The borrowed token prevents thread transfer; every operation rechecks its
/// epoch in case user code has ended the runtime inside the writer closure.
pub struct IndexedCanvasWriter<'a, const BITS: usize, const N: usize> {
    storage: &'a StaticIndexedCanvasBuffer<BITS, N>,
    token: &'a UiAccessToken,
}

impl<const BITS: usize, const N: usize> IndexedCanvasWriter<'_, BITS, N> {
    /// Fills pixels and row padding. Invalid indices or expired epochs do not mutate storage.
    pub fn fill(&mut self, index: u8) -> bool {
        if self.token.check_current().is_err() || index as usize >= self.storage.palette_len {
            return false;
        }
        let palette = palette_bytes(BITS);
        // SAFETY: constructor-checked N includes the palette; only pixel storage
        // and row padding are written. No reference or callback spans this write.
        unsafe {
            core::ptr::write_bytes(
                self.storage.bytes.get().cast::<u8>().add(palette),
                repeat_index(index, BITS),
                N - palette,
            );
        }
        true
    }

    /// Returns false without mutation for invalid coordinates, indices, or expired epochs.
    pub fn set(&mut self, x: usize, y: usize, index: u8) -> bool {
        if self.token.check_current().is_err()
            || x >= self.storage.width
            || y >= self.storage.height
            || index as usize >= self.storage.palette_len
        {
            return false;
        }
        let bit = x * BITS;
        let offset = palette_bytes(BITS) + y * self.storage.stride_bytes() + bit / 8;
        let shift = 8 - BITS - bit % 8;
        let mask = (((1u16 << BITS) - 1) as u8) << shift;
        // SAFETY: the checked layout and coordinates bound this byte. Supported
        // depths divide eight, so no pixel crosses a byte. No reference/callback
        // spans the read/modify/write operation.
        unsafe {
            let ptr = self.storage.bytes.get().cast::<u8>().add(offset);
            ptr.write((ptr.read() & !mask) | (index << shift));
        }
        true
    }
}

/// Black/white convenience wrapper over a one-bit indexed canvas.
/// Zero means ink, one means paper; both colors are opaque.
pub struct StaticI1CanvasBuffer<const N: usize>(StaticIndexedCanvasBuffer<1, N>);

impl<const N: usize> StaticI1CanvasBuffer<N> {
    pub const fn new(width: usize, height: usize, initial: MonochromePixel) -> Self {
        Self(StaticIndexedCanvasBuffer::new(
            width,
            height,
            &[PaletteColor::rgb(0, 0, 0), PaletteColor::rgb(255, 255, 255)],
            match initial {
                MonochromePixel::Ink => 0,
                MonochromePixel::Paper => 1,
            },
        ))
    }

    pub const fn width(&self) -> usize {
        self.0.width()
    }
    pub const fn height(&self) -> usize {
        self.0.height()
    }
    pub const fn stride_bytes(&self) -> usize {
        self.0.stride_bytes()
    }
    pub const fn capacity_bytes(&self) -> usize {
        self.0.capacity_bytes()
    }

    pub fn with_writer<R>(
        &'static self,
        token: &UiAccessToken,
        update: impl FnOnce(&mut I1CanvasWriter<'_, N>) -> R,
    ) -> Result<R, AccessFault> {
        token.check_current()?;
        Ok(update(&mut I1CanvasWriter {
            writer: IndexedCanvasWriter {
                storage: &self.0,
                token,
            },
        }))
    }

    pub(super) fn indexed(&'static self) -> &'static StaticIndexedCanvasBuffer<1, N> {
        &self.0
    }
}

pub struct I1CanvasWriter<'a, const N: usize> {
    writer: IndexedCanvasWriter<'a, 1, N>,
}

impl<const N: usize> I1CanvasWriter<'_, N> {
    pub fn fill(&mut self, pixel: MonochromePixel) -> bool {
        self.writer.fill(pixel.byte() & 1)
    }

    pub fn set(&mut self, x: usize, y: usize, pixel: MonochromePixel) -> bool {
        self.writer.set(x, y, pixel.byte() & 1)
    }
}
