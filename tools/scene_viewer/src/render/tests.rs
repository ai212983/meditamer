use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use super::*;

#[test]
fn run_render_smoke_writes_output_and_debug_images() {
    let root = unique_temp_dir("scene_viewer_render_smoke");
    let bundle_path = root.join("scene.scenebundle");
    let out_path = root.join("render/output.png");
    let debug_dir = root.join("render/debug");
    fs::create_dir_all(&root).expect("create root");
    write_test_bundle(&bundle_path, 2, 2);

    run_render(vec![
        "--bundle".to_owned(),
        bundle_path.display().to_string(),
        "--out".to_owned(),
        out_path.display().to_string(),
        "--mode".to_owned(),
        "gray3".to_owned(),
        "--save-debug".to_owned(),
        debug_dir.display().to_string(),
    ])
    .expect("run render");

    assert!(out_path.exists(), "main render should exist");
    let rendered = image::open(&out_path)
        .expect("open render image")
        .to_luma8();
    assert_eq!(rendered.width(), 2);
    assert_eq!(rendered.height(), 2);

    assert!(debug_dir.join("01_tone_base.png").exists());
    assert!(debug_dir.join("02_stylized.png").exists());
    assert!(debug_dir.join("03_quantized.png").exists());
}

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}_{}_{}", std::process::id(), stamp))
}

fn write_test_bundle(path: &Path, width: u16, height: u16) {
    let strip_height = height;
    let strip_count = 1u16;
    let channel_count = 2u16;
    let pixels = (width as usize) * (height as usize);
    let albedo: Vec<u8> = (0..pixels).map(|i| (40 + i as u8 * 20).min(255)).collect();
    let light = vec![255u8; pixels];

    let mut out = Vec::new();
    out.extend_from_slice(b"SMBNDL1\0");
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&24u16.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&strip_height.to_le_bytes());
    out.extend_from_slice(&strip_count.to_le_bytes());
    out.extend_from_slice(&channel_count.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());

    out.extend_from_slice(&[CH_ALBEDO, 8, 0, 0]);
    out.extend_from_slice(&[CH_LIGHT, 8, 0, 0]);

    let payload_start = 24 + (channel_count as usize * 4) + (channel_count as usize * 16);
    let albedo_offset = payload_start as u64;
    let light_offset = albedo_offset + albedo.len() as u64;

    out.extend_from_slice(&albedo_offset.to_le_bytes());
    out.extend_from_slice(&(albedo.len() as u32).to_le_bytes());
    out.extend_from_slice(&(albedo.len() as u32).to_le_bytes());
    out.extend_from_slice(&light_offset.to_le_bytes());
    out.extend_from_slice(&(light.len() as u32).to_le_bytes());
    out.extend_from_slice(&(light.len() as u32).to_le_bytes());

    out.extend_from_slice(&albedo);
    out.extend_from_slice(&light);
    fs::write(path, out).expect("write test bundle");
}
