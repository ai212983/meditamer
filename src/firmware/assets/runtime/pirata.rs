use embedded_graphics::{pixelcolor::BinaryColor, prelude::*};

#[path = "pirata_specs.rs"]
mod pirata_specs;

use super::AssetLoadError;
use pirata_specs::{GlyphSpec, PIRATA_GLYPH_SPECS};

#[cfg(feature = "psram-alloc")]
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};

#[cfg(feature = "psram-alloc")]
use crate::firmware::psram;
#[cfg(feature = "psram-alloc")]
use crate::firmware::types::SdAssetReadResultCode;

pub(crate) const PIRATA_TIME_SPACING: i32 = 3;

#[cfg(feature = "psram-alloc")]
struct PirataCachedGlyph {
    width: u16,
    height: u16,
    data_len: usize,
    data: psram::LargeByteBuffer,
}

#[cfg(feature = "psram-alloc")]
struct PirataClockCache {
    glyphs: [Option<PirataCachedGlyph>; PIRATA_GLYPH_SPECS.len()],
}

#[cfg(feature = "psram-alloc")]
static PIRATA_CLOCK_CACHE: Mutex<CriticalSectionRawMutex, Option<PirataClockCache>> =
    Mutex::new(None);

pub(crate) async fn draw_pirata_time_centered<T>(
    display: &mut T,
    text: &str,
    center: Point,
) -> Result<(), AssetLoadError>
where
    T: DrawTarget<Color = BinaryColor>,
{
    #[cfg(feature = "psram-alloc")]
    {
        if ensure_pirata_cache_loaded().await.is_ok() {
            return draw_pirata_time_centered_cached(display, text, center).await;
        }
    }

    draw_pirata_time_centered_uncached(display, text, center).await
}

async fn draw_pirata_time_centered_uncached<T>(
    display: &mut T,
    text: &str,
    center: Point,
) -> Result<(), AssetLoadError>
where
    T: DrawTarget<Color = BinaryColor>,
{
    let (specs, count, total_width, max_height) = collect_pirata_layout(text);
    if count == 0 {
        return Ok(());
    }

    let mut x = center.x - total_width / 2;
    let top = center.y - max_height / 2;

    for spec in specs.iter().take(count).flatten() {
        let (payload, payload_len) = super::sd_asset_read_roundtrip(spec.path).await?;
        if payload_len != spec.bytes_len {
            return Err(AssetLoadError::SizeMismatch);
        }

        let glyph_top = top + (max_height - spec.height as i32) / 2;
        draw_glyph_data(
            display,
            spec.width,
            spec.height,
            &payload[..payload_len],
            Point::new(x, glyph_top),
        );
        x += spec.width as i32 + PIRATA_TIME_SPACING;
    }

    Ok(())
}

#[cfg(feature = "psram-alloc")]
async fn ensure_pirata_cache_loaded() -> Result<(), AssetLoadError> {
    {
        let guard = PIRATA_CLOCK_CACHE.lock().await;
        if guard.is_some() {
            return Ok(());
        }
    }

    let mut cache = PirataClockCache {
        glyphs: core::array::from_fn(|_| None),
    };

    for (index, spec) in PIRATA_GLYPH_SPECS.iter().enumerate() {
        let (payload, payload_len) = super::sd_asset_read_roundtrip(spec.path).await?;
        if payload_len != spec.bytes_len {
            return Err(AssetLoadError::SizeMismatch);
        }

        let mut buffer = psram::alloc_large_byte_buffer(payload_len)
            .map_err(|_| AssetLoadError::Device(SdAssetReadResultCode::OperationFailed))?;
        buffer.as_mut_slice()[..payload_len].copy_from_slice(&payload[..payload_len]);

        cache.glyphs[index] = Some(PirataCachedGlyph {
            width: spec.width,
            height: spec.height,
            data_len: payload_len,
            data: buffer,
        });
    }

    let mut guard = PIRATA_CLOCK_CACHE.lock().await;
    if guard.is_none() {
        *guard = Some(cache);
    }

    Ok(())
}

#[cfg(feature = "psram-alloc")]
async fn draw_pirata_time_centered_cached<T>(
    display: &mut T,
    text: &str,
    center: Point,
) -> Result<(), AssetLoadError>
where
    T: DrawTarget<Color = BinaryColor>,
{
    let (specs, count, total_width, max_height) = collect_pirata_layout(text);
    if count == 0 {
        return Ok(());
    }

    let guard = PIRATA_CLOCK_CACHE.lock().await;
    let cache = guard.as_ref().ok_or(AssetLoadError::Device(
        SdAssetReadResultCode::OperationFailed,
    ))?;

    let mut x = center.x - total_width / 2;
    let top = center.y - max_height / 2;

    for spec in specs.iter().take(count).flatten() {
        let index = pirata_spec_index(spec.glyph).ok_or(AssetLoadError::Device(
            SdAssetReadResultCode::OperationFailed,
        ))?;
        let glyph = cache.glyphs[index].as_ref().ok_or(AssetLoadError::Device(
            SdAssetReadResultCode::OperationFailed,
        ))?;

        if glyph.data_len != spec.bytes_len {
            return Err(AssetLoadError::SizeMismatch);
        }

        let glyph_top = top + (max_height - glyph.height as i32) / 2;
        draw_glyph_data(
            display,
            glyph.width,
            glyph.height,
            &glyph.data.as_slice()[..glyph.data_len],
            Point::new(x, glyph_top),
        );
        x += glyph.width as i32 + PIRATA_TIME_SPACING;
    }

    Ok(())
}

fn collect_pirata_layout(text: &str) -> ([Option<&'static GlyphSpec>; 5], usize, i32, i32) {
    let mut specs: [Option<&GlyphSpec>; 5] = [None, None, None, None, None];
    let mut count = 0usize;
    let mut total_width = 0i32;
    let mut max_height = 0i32;

    for ch in text.chars() {
        if let Some(spec) = pirata_spec_for(ch) {
            if count < specs.len() {
                specs[count] = Some(spec);
                count += 1;
                total_width += spec.width as i32;
                max_height = max_height.max(spec.height as i32);
            }
        }
    }

    if count > 0 {
        total_width += (count as i32 - 1) * PIRATA_TIME_SPACING;
    }

    (specs, count, total_width, max_height)
}

fn pirata_spec_for(ch: char) -> Option<&'static GlyphSpec> {
    PIRATA_GLYPH_SPECS.iter().find(|spec| spec.glyph == ch)
}

#[cfg(feature = "psram-alloc")]
fn pirata_spec_index(ch: char) -> Option<usize> {
    PIRATA_GLYPH_SPECS.iter().position(|spec| spec.glyph == ch)
}

fn draw_glyph_data<T>(display: &mut T, width: u16, height: u16, data: &[u8], top_left: Point)
where
    T: DrawTarget<Color = BinaryColor>,
{
    let bytes_per_row = (width as usize).div_ceil(8);
    if data.len() < bytes_per_row.saturating_mul(height as usize) {
        return;
    }

    for y in 0..height as i32 {
        let row = y as usize * bytes_per_row;
        for x in 0..width as i32 {
            let byte = data[row + (x as usize / 8)];
            if (byte & (1 << (x as usize % 8))) != 0 {
                let _ = display.draw_iter(core::iter::once(Pixel(
                    Point::new(top_left.x + x, top_left.y + y),
                    BinaryColor::On,
                )));
            }
        }
    }
}
