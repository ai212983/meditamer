use image::{GrayImage, ImageBuffer};
use std::path::Path;

pub(crate) fn load_grayscale_resize(path: &Path, width: u16, height: u16) -> Result<Vec<u8>, String> {
    let img = image::open(path)
        .map_err(|e| format!("open ghost image {}: {e}", path.display()))?
        .to_luma8();

    let out = if img.width() == width as u32 && img.height() == height as u32 {
        img
    } else {
        image::imageops::resize(
            &img,
            width as u32,
            height as u32,
            image::imageops::FilterType::CatmullRom,
        )
    };

    Ok(out.into_raw())
}

pub(crate) fn save_gray(
    path: &Path,
    width: u16,
    height: u16,
    pixels: &[u8],
) -> Result<(), String> {
    let img: GrayImage = ImageBuffer::from_vec(width as u32, height as u32, pixels.to_vec())
        .ok_or_else(|| "buffer size mismatch for gray image".to_owned())?;
    img.save(path)
        .map_err(|e| format!("save {}: {e}", path.display()))
}
