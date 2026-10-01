//! PNG decoding into the borrowed planes the shared core renders from.
//!
//! Only the host tool touches files: it verifies the canonical sprite
//! dimensions (hour 245x810, minute 156x1014), splits diffuse RGBA into
//! RGB albedo plus alpha, and reads normal (RGB) and specular (gray) as
//! data. Source assets stay unchanged on disk.

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Owned grayscale dial artwork the core borrows as [`analog_clock::DialMap`].
/// Decoded once from `dial_durer_grayscale.png`; shared by `Arc` so every
/// render path (CLI snapshots, bench, server) borrows the same pixels.
#[derive(Clone, Debug)]
pub struct DecodedDial {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl DecodedDial {
    pub fn as_dial(&self) -> analog_clock::DialMap<'_> {
        analog_clock::DialMap {
            width: self.width,
            height: self.height,
            pixels: &self.pixels,
        }
    }
}

/// Owned decoded planes for one hand, in the layout `HandMaps` borrows.
#[derive(Clone, Debug)]
pub struct DecodedHand {
    pub width: u16,
    pub height: u16,
    pub albedo: Vec<u8>,
    pub alpha: Vec<u8>,
    pub normal: Vec<u8>,
    pub spec: Vec<u8>,
}

/// Both hands' owned planes. Borrow via [`DecodedHands::as_hands`].
/// `dial` is `None` only for tiny test stand-ins; the real loader always
/// fills it, and callers attach it to params (never to firmware).
#[derive(Clone, Debug)]
pub struct DecodedHands {
    pub hour: DecodedHand,
    pub minute: DecodedHand,
    pub dial: Option<Arc<DecodedDial>>,
}

fn borrow_hand(hand: &DecodedHand, pivot: (f32, f32)) -> analog_clock::HandMaps<'_> {
    analog_clock::HandMaps {
        width: hand.width,
        height: hand.height,
        albedo: &hand.albedo,
        alpha: &hand.alpha,
        normal: &hand.normal,
        spec: &hand.spec,
        pivot_x: pivot.0,
        pivot_y: pivot.1,
    }
}

/// Falls back to the sprite center when the canonical pivot lies outside
/// the decoded dimensions, so small test stand-ins borrow in-bounds pivots
/// instead of the full-size canonical values.
fn fitting_pivot(hand: &DecodedHand, canonical: (f32, f32)) -> (f32, f32) {
    let (w, h) = (f32::from(hand.width), f32::from(hand.height));
    if canonical.0 >= 0.0 && canonical.0 < w && canonical.1 > 0.0 && canonical.1 < h {
        canonical
    } else {
        (w / 2.0, h / 2.0)
    }
}

impl DecodedHands {
    pub fn as_hands(&self) -> analog_clock::Hands<'_> {
        analog_clock::Hands {
            hour: borrow_hand(
                &self.hour,
                fitting_pivot(&self.hour, analog_clock::HOUR_PIVOT),
            ),
            minute: borrow_hand(
                &self.minute,
                fitting_pivot(&self.minute, analog_clock::MINUTE_PIVOT),
            ),
        }
    }
}

/// Repository assets directory by default: manifest-relative, so the
/// command works from any working directory and no absolute path is baked
/// in anywhere. `--assets` overrides it for experiments.
pub fn default_asset_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn load_hand(dir: &Path, stem: &str, expect_w: u32, expect_h: u32) -> Result<DecodedHand, String> {
    let diffuse = image::open(dir.join(format!("{stem}_d.png")))
        .map_err(|e| format!("cannot open {stem}_d.png: {e}"))?
        .to_rgba8();
    let normal = image::open(dir.join(format!("{stem}_n.png")))
        .map_err(|e| format!("cannot open {stem}_n.png: {e}"))?
        .to_rgb8();
    let spec = image::open(dir.join(format!("{stem}_s.png")))
        .map_err(|e| format!("cannot open {stem}_s.png: {e}"))?
        .to_luma8();
    for (w, h, name) in [
        (diffuse.width(), diffuse.height(), "diffuse"),
        (normal.width(), normal.height(), "normal"),
        (spec.width(), spec.height(), "specular"),
    ] {
        if w != expect_w || h != expect_h {
            return Err(format!(
                "{stem} {name} is {w}x{h}, expected {expect_w}x{expect_h}"
            ));
        }
    }
    let n = expect_w as usize * expect_h as usize;
    let mut albedo = Vec::with_capacity(n * 3);
    let mut alpha = Vec::with_capacity(n);
    for px in diffuse.pixels() {
        albedo.extend_from_slice(&px.0[..3]);
        alpha.push(px.0[3]);
    }
    Ok(DecodedHand {
        width: expect_w as u16,
        height: expect_h as u16,
        albedo,
        alpha,
        normal: normal.into_raw(),
        spec: spec.into_raw(),
    })
}

/// Loads the supplied Durer dial artwork once (`dial_durer_grayscale.png`,
/// read as gray): a 600x600 square, anything else is an error.
fn load_dial(dir: &Path) -> Result<DecodedDial, String> {
    let img = image::open(dir.join("dial_durer_grayscale.png"))
        .map_err(|e| format!("cannot open dial_durer_grayscale.png: {e}"))?
        .to_luma8();
    let (w, h) = (img.width(), img.height());
    if w != 600 || h != 600 {
        return Err(format!(
            "dial_durer_grayscale.png is {w}x{h}, expected 600x600"
        ));
    }
    Ok(DecodedDial {
        width: w,
        height: h,
        pixels: img.into_raw(),
    })
}

/// Loads and validates both hands from `dir` (`hour_hand_{d,n,s}.png`,
/// `minute_hand_{d,n,s}.png`) plus the supplied dial artwork.
pub fn load_hands(dir: &Path) -> Result<DecodedHands, String> {
    Ok(DecodedHands {
        hour: load_hand(dir, "hour_hand", 245, 810)?,
        minute: load_hand(dir, "minute_hand", 156, 1014)?,
        dial: Some(Arc::new(load_dial(dir)?)),
    })
}
