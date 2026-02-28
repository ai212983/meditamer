use std::{
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use image::{GrayImage, ImageBuffer};
use serde_json::Value;

use super::*;
use crate::format::read_header;

#[test]
fn run_build_smoke_writes_bundle_and_metadata() {
    let root = unique_temp_dir("scene_maker_build_smoke");
    let input_dir = root.join("input");
    let bundle_path = root.join("out/scene.scenebundle");
    fs::create_dir_all(&input_dir).expect("create input dir");

    write_gray_png(&input_dir.join("albedo.png"), 2, 2, &[10, 20, 30, 40]);
    write_gray_png(&input_dir.join("light.png"), 2, 2, &[200, 210, 220, 230]);

    run_build(vec![
        "--input".to_owned(),
        input_dir.display().to_string(),
        "--out".to_owned(),
        bundle_path.display().to_string(),
        "--width".to_owned(),
        "2".to_owned(),
        "--height".to_owned(),
        "2".to_owned(),
        "--strip-height".to_owned(),
        "2".to_owned(),
        "--derive-edge".to_owned(),
        "false".to_owned(),
    ])
    .expect("run build");

    let metadata_path = bundle_path.with_extension("scenebundle.json");
    assert!(bundle_path.exists(), "bundle should be written");
    assert!(metadata_path.exists(), "metadata should be written");

    let mut file = fs::File::open(&bundle_path).expect("open bundle");
    let (width, height, strip_height, strip_count, channel_count) =
        read_header(&mut file).expect("read header");
    assert_eq!(width, 2);
    assert_eq!(height, 2);
    assert_eq!(strip_height, 2);
    assert_eq!(strip_count, 1);
    assert_eq!(channel_count, CHANNELS.len() as u16);

    let meta_raw = fs::read_to_string(&metadata_path).expect("read metadata");
    let meta: Value = serde_json::from_str(&meta_raw).expect("parse metadata");
    assert_eq!(meta["width"], 2);
    assert_eq!(meta["height"], 2);
    assert_eq!(meta["strip_count"], 1);
}

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("{prefix}_{}_{}", std::process::id(), stamp))
}

fn write_gray_png(path: &Path, width: u32, height: u32, pixels: &[u8]) {
    let img: GrayImage =
        ImageBuffer::from_vec(width, height, pixels.to_vec()).expect("build image");
    img.save(path).expect("save test image");
}
