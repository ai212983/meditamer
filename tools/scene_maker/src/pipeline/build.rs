use std::fs;

use super::{
    channels::{derive_edge_if_needed, load_channels},
    encode::encode_channels,
    output::{build_metadata, write_bundle, write_metadata},
    SceneDims,
};
use crate::cli::{parse_build_args, BuildConfig};

pub(crate) fn run_build<I>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = String>,
{
    let cfg = parse_build_args(args)?;
    run_build_with_config(cfg)
}

fn run_build_with_config(cfg: BuildConfig) -> Result<(), String> {
    let dims = validate_build_config(&cfg)?;
    prepare_output_dirs(&cfg)?;
    let mut channels = load_channels(&cfg, dims.total_px)?;
    derive_edge_if_needed(&cfg, dims, &mut channels);
    let encoded = encode_channels(&cfg, dims, &channels);
    let bundle_bytes = write_bundle(&cfg, dims.strip_count, &encoded)?;
    let meta = build_metadata(&cfg, dims.strip_count, bundle_bytes, &channels);
    write_metadata(&cfg, &meta)?;

    println!("wrote bundle: {}", cfg.out_bundle.display());
    println!("wrote metadata: {}", cfg.metadata_out.display());
    println!(
        "scene: {}x{}, strips={}, compression={}",
        cfg.width,
        cfg.height,
        dims.strip_count,
        cfg.compression.as_str()
    );
    Ok(())
}

fn validate_build_config(cfg: &BuildConfig) -> Result<SceneDims, String> {
    if cfg.width == 0 || cfg.height == 0 {
        return Err("width and height must be greater than zero".to_owned());
    }
    if cfg.strip_height == 0 {
        return Err("strip-height must be greater than zero".to_owned());
    }

    let width = cfg.width as usize;
    let height = cfg.height as usize;
    Ok(SceneDims {
        width,
        height,
        total_px: width * height,
        strip_count: div_ceil_u16(cfg.height, cfg.strip_height),
    })
}

fn prepare_output_dirs(cfg: &BuildConfig) -> Result<(), String> {
    if let Some(parent) = cfg.out_bundle.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create bundle dir: {e}"))?;
    }
    if let Some(parent) = cfg.metadata_out.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create metadata dir: {e}"))?;
    }
    Ok(())
}

pub(crate) fn run_inspect<I>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = String>,
{
    crate::inspect::run_inspect(args)
}

fn div_ceil_u16(a: u16, b: u16) -> u16 {
    ((a as u32 + b as u32 - 1) / b as u32) as u16
}
