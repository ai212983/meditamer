fn explicit_channel_paths(cfg: &BuildConfig) -> ExplicitChannelPaths {
    ExplicitChannelPaths {
        albedo: cfg.albedo.clone(),
        light: cfg.light.clone(),
        ao: cfg.ao.clone(),
        depth: cfg.depth.clone(),
        edge: cfg.edge.clone(),
        mask: cfg.mask.clone(),
        stroke: cfg.stroke.clone(),
        normal_x: cfg.normal_x.clone(),
        normal_y: cfg.normal_y.clone(),
    }
}

fn load_channels(cfg: &BuildConfig, total_px: usize) -> Result<Vec<ChannelData>, String> {
    let explicit = explicit_channel_paths(cfg);
    let mut channels = Vec::with_capacity(CHANNELS.len());
    for template in CHANNELS {
        channels.push(load_channel(cfg, &explicit, template, total_px)?);
    }
    Ok(channels)
}

fn load_channel(
    cfg: &BuildConfig,
    explicit: &ExplicitChannelPaths,
    template: ChannelTemplate,
    total_px: usize,
) -> Result<ChannelData, String> {
    let requested = explicit.lookup(template.name);
    let resolved = resolve_channel_path(&cfg.input_dir, template.name, requested.as_deref());

    match resolved {
        Some(path) => {
            let img = load_grayscale_resized(&path, cfg.width, cfg.height)?;
            Ok(ChannelData {
                template,
                source: path.display().to_string(),
                pixels: img,
            })
        }
        None if template.required => Err(format!(
            "missing required map '{}'; expected {}.png",
            template.name, template.name
        )),
        None => Ok(ChannelData {
            template,
            source: "generated-default".to_owned(),
            pixels: vec![template.default_value; total_px],
        }),
    }
}

fn derive_edge_if_needed(cfg: &BuildConfig, dims: SceneDims, channels: &mut [ChannelData]) {
    if !cfg.derive_edge {
        return;
    }

    let edge_idx = channel_index(ChannelId::Edge);
    if channels[edge_idx].source != "generated-default" {
        return;
    }

    let depth_idx = channel_index(ChannelId::Depth);
    let depth_non_default = channels[depth_idx].source != "generated-default";
    let source_pixels = if depth_non_default {
        &channels[depth_idx].pixels
    } else {
        &channels[channel_index(ChannelId::Albedo)].pixels
    };
    channels[edge_idx].pixels = sobel_edges(source_pixels, dims.width, dims.height);
    channels[edge_idx].source = if depth_non_default {
        "derived-from-depth".to_owned()
    } else {
        "derived-from-albedo".to_owned()
    };
}

fn resolve_channel_path(input_dir: &Path, name: &str, explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        if path.exists() {
            return Some(path.to_path_buf());
        }
        return None;
    }

    let exts = ["png"];
    for ext in exts {
        let candidate = input_dir.join(format!("{name}.{ext}"));
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn load_grayscale_resized(path: &Path, width: u16, height: u16) -> Result<Vec<u8>, String> {
    let img = image::open(path)
        .map_err(|e| format!("open image {}: {e}", path.display()))?
        .to_luma8();

    let out = if img.width() == width as u32 && img.height() == height as u32 {
        img
    } else {
        image::imageops::resize(&img, width as u32, height as u32, FilterType::CatmullRom)
    };

    Ok(out.into_raw())
}

fn sobel_edges(src: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut out = vec![0u8; src.len()];

    for y in 0..height {
        for x in 0..width {
            let p = |ix: i32, iy: i32| -> i32 {
                let xx = ix.clamp(0, (width as i32) - 1) as usize;
                let yy = iy.clamp(0, (height as i32) - 1) as usize;
                src[yy * width + xx] as i32
            };

            let x_i = x as i32;
            let y_i = y as i32;

            let gx = -p(x_i - 1, y_i - 1) + p(x_i + 1, y_i - 1) - 2 * p(x_i - 1, y_i)
                + 2 * p(x_i + 1, y_i)
                - p(x_i - 1, y_i + 1)
                + p(x_i + 1, y_i + 1);

            let gy = -p(x_i - 1, y_i - 1) - 2 * p(x_i, y_i - 1) - p(x_i + 1, y_i - 1)
                + p(x_i - 1, y_i + 1)
                + 2 * p(x_i, y_i + 1)
                + p(x_i + 1, y_i + 1);

            let mag = ((gx.abs() + gy.abs()) / 6).clamp(0, 255) as u8;
            out[y * width + x] = mag;
        }
    }

    out
}

fn channel_index(id: ChannelId) -> usize {
    CHANNELS
        .iter()
        .position(|ch| ch.id as u8 == id as u8)
        .expect("channel id present")
}
