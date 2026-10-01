//! `export-maps`: pack the canonical source maps into one SD image.
//!
//! ```text
//! analog_clock_preview export-maps --out-dir DIR [--assets DIR]
//! ```
//!
//! Loads the canonical hands/dial once via the existing loader, verifies
//! dimensions and that both diffuse albedos are grayscale (R == G == B per
//! pixel; colored input is an error, never silently dropped), then appends
//! the payload in canonical order with a [`clock_assets`] header. Writes
//! atomically (`MAPS.BIN.tmp` + rename) to `<out-dir>/CLOCK/MAPS.BIN` — the
//! SD target is `/assets/CLOCK/MAPS.BIN`, so `hostctl upload --src DIR
//! --dst /assets` keeps working — plus a small manifest with relative
//! asset names and header/sizes/pivots only. No rendered dial-position
//! output is produced here; the device rotates and lights at runtime.

use std::path::Path;

use super::assets::DecodedHands;

/// Flags `export-maps` accepts: nothing else.
const EXPORT_MAPS_FLAGS: &[&str] = &["--out-dir", "--assets"];

/// Source PNG names relative to the assets dir (manifest only).
const SOURCE_NAMES: &[&str] = &[
    "dial_durer_grayscale.png",
    "hour_hand_d.png",
    "hour_hand_n.png",
    "hour_hand_s.png",
    "minute_hand_d.png",
    "minute_hand_n.png",
    "minute_hand_s.png",
];

/// Reduces one RGB triple stream to its single gray channel, rejecting
/// colored input instead of silently dropping color.
fn gray_channel(rgb: &[u8], label: &str) -> Result<Vec<u8>, String> {
    if !rgb.len().is_multiple_of(3) {
        return Err(format!("{label} albedo length is not a multiple of 3"));
    }
    let mut gray = Vec::with_capacity(rgb.len() / 3);
    for triple in rgb.chunks_exact(3) {
        if triple[0] != triple[1] || triple[0] != triple[2] {
            return Err(format!(
                "{label} albedo is not grayscale; refusing to drop color"
            ));
        }
        gray.push(triple[0]);
    }
    Ok(gray)
}

/// Builds the canonical payload from already-loaded hands: dial gray, then
/// hour gray/alpha/normal-RGB/spec, then minute gray/alpha/normal-RGB/spec.
fn build_payload(hands: &DecodedHands) -> Result<Vec<u8>, String> {
    let dial = hands
        .dial
        .as_ref()
        .ok_or_else(|| "no dial artwork loaded".to_string())?;
    if dial.width != 600 || dial.height != 600 {
        return Err(format!(
            "dial is {}x{}, expected 600x600",
            dial.width, dial.height
        ));
    }
    if hands.hour.width != 245 || hands.hour.height != 810 {
        return Err(format!(
            "hour hand is {}x{}, expected 245x810",
            hands.hour.width, hands.hour.height
        ));
    }
    if hands.minute.width != 156 || hands.minute.height != 1014 {
        return Err(format!(
            "minute hand is {}x{}, expected 156x1014",
            hands.minute.width, hands.minute.height
        ));
    }
    let hour_gray = gray_channel(&hands.hour.albedo, "hour")?;
    let minute_gray = gray_channel(&hands.minute.albedo, "minute")?;
    let mut payload = Vec::with_capacity(clock_assets::PAYLOAD_LEN);
    payload.extend_from_slice(&dial.pixels);
    payload.extend_from_slice(&hour_gray);
    payload.extend_from_slice(&hands.hour.alpha);
    payload.extend_from_slice(&hands.hour.normal);
    payload.extend_from_slice(&hands.hour.spec);
    payload.extend_from_slice(&minute_gray);
    payload.extend_from_slice(&hands.minute.alpha);
    payload.extend_from_slice(&hands.minute.normal);
    payload.extend_from_slice(&hands.minute.spec);
    if payload.len() != clock_assets::PAYLOAD_LEN {
        return Err(format!(
            "payload is {} bytes, expected {}",
            payload.len(),
            clock_assets::PAYLOAD_LEN
        ));
    }
    Ok(payload)
}

fn manifest_text() -> String {
    let mut text = String::from("# analog clock source-map pack manifest\n");
    for name in SOURCE_NAMES {
        text.push_str(&format!("source: {name}\n"));
    }
    text.push_str(&format!(
        "magic: MCLKMAP1\nheader_len: {}\npayload_len: {}\nfile_len: {}\n",
        clock_assets::HEADER_LEN,
        clock_assets::PAYLOAD_LEN,
        clock_assets::FILE_LEN
    ));
    text.push_str(&format!(
        "dial: 600x600 gray\nhour: 245x810 gray+alpha+normal-rgb+spec pivot {:?}\nminute: 156x1014 gray+alpha+normal-rgb+spec pivot {:?}\nsd_target: /assets/CLOCK/MAPS.BIN\n",
        analog_clock::HOUR_PIVOT,
        analog_clock::MINUTE_PIVOT
    ));
    text
}

pub(crate) fn cmd_export_maps(args: &[String], assets_dir: &Path) -> Result<(), String> {
    super::reject_unknown_flags(args, EXPORT_MAPS_FLAGS)?;
    let out_dir = super::flag(args, "--out-dir").ok_or_else(|| super::usage().to_string())?;
    let hands = super::load_assets(assets_dir)?;
    let payload = build_payload(&hands)?;
    let header =
        clock_assets::encode_header(&payload).map_err(|_| "payload length mismatch".to_string())?;

    let dir = Path::new(&out_dir).join("CLOCK");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    // Atomic write: temp file in the same directory, then rename.
    let dest = dir.join("MAPS.BIN");
    let tmp = dir.join("MAPS.BIN.tmp");
    let mut file = Vec::with_capacity(clock_assets::FILE_LEN);
    file.extend_from_slice(&header);
    file.extend_from_slice(&payload);
    std::fs::write(&tmp, &file).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &dest).map_err(|e| format!("cannot rename {}: {e}", dest.display()))?;
    let manifest = dir.join("MANIFEST.txt");
    std::fs::write(&manifest, manifest_text())
        .map_err(|e| format!("cannot write {}: {e}", manifest.display()))?;
    println!("wrote {} ({} bytes)", dest.display(), file.len());
    println!("wrote {}", manifest.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray_hand(w: u16, h: u16, v: u8) -> crate::assets::DecodedHand {
        let n = w as usize * h as usize;
        crate::assets::DecodedHand {
            width: w,
            height: h,
            albedo: (0..n).flat_map(|_| [v, v, v]).collect(),
            alpha: vec![7; n],
            normal: (0..n).flat_map(|_| [128, 200, 64]).collect(),
            spec: vec![9; n],
        }
    }

    fn canonical_hands(gray: u8) -> DecodedHands {
        DecodedHands {
            hour: gray_hand(245, 810, gray),
            minute: gray_hand(156, 1014, gray),
            dial: Some(std::sync::Arc::new(crate::assets::DecodedDial {
                width: 600,
                height: 600,
                pixels: vec![3; 600 * 600],
            })),
        }
    }

    #[test]
    fn single_gray_channel_is_lossless_and_rest_verbatim() {
        let hands = canonical_hands(123);
        let payload = build_payload(&hands).expect("build");
        assert_eq!(payload.len(), clock_assets::PAYLOAD_LEN);
        // Gray channel equals the (identical) RGB channels.
        assert_eq!(
            &payload[clock_assets::OFF_HOUR_ALBEDO..clock_assets::OFF_HOUR_ALPHA],
            &vec![123u8; clock_assets::HOUR_PIXELS][..]
        );
        assert_eq!(
            &payload[clock_assets::OFF_MINUTE_ALBEDO..clock_assets::OFF_MINUTE_ALPHA],
            &vec![123u8; clock_assets::MINUTE_PIXELS][..]
        );
        // Alpha, normal RGB, and spec bytes are preserved exactly.
        assert!(
            payload[clock_assets::OFF_HOUR_ALPHA..clock_assets::OFF_HOUR_NORMAL]
                .iter()
                .all(|&b| b == 7)
        );
        assert_eq!(
            &payload[clock_assets::OFF_HOUR_NORMAL..clock_assets::OFF_HOUR_NORMAL + 3],
            &[128, 200, 64]
        );
        assert!(
            payload[clock_assets::OFF_HOUR_SPEC..clock_assets::OFF_MINUTE_ALBEDO]
                .iter()
                .all(|&b| b == 9)
        );
        // Header round-trips through the shared decoder.
        let header = clock_assets::encode_header(&payload).expect("header");
        let mut file = Vec::with_capacity(clock_assets::FILE_LEN);
        file.extend_from_slice(&header);
        file.extend_from_slice(&payload);
        let assets = clock_assets::decode(&file).expect("decode");
        assert_eq!(assets.payload(), &payload[..]);
    }

    #[test]
    fn colored_albedo_is_rejected() {
        let mut hands = canonical_hands(50);
        hands.hour.albedo[0] = 51; // R != G: colored pixel.
        assert!(build_payload(&hands).is_err());
        let mut hands = canonical_hands(50);
        let last = hands.minute.albedo.len() - 1;
        hands.minute.albedo[last] = 49; // B != R: colored pixel.
        assert!(build_payload(&hands).is_err());
    }

    #[test]
    fn wrong_dimensions_rejected() {
        let mut hands = canonical_hands(1);
        hands.hour.width = 100;
        assert!(build_payload(&hands).is_err());
    }
}
