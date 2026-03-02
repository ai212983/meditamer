fn save_render_outputs(
    cfg: &Config,
    bundle: &Bundle,
    buffers: &RenderBuffers,
    optional: &OptionalMaps,
) -> Result<(), String> {
    if let Some(parent) = cfg.out.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("create output dir {}: {e}", parent.display()))?;
    }
    save_gray(&cfg.out, bundle.width, bundle.height, &buffers.quantized)?;
    println!("wrote {}", cfg.out.display());

    if let Some(debug_dir) = cfg.save_debug.as_ref() {
        fs::create_dir_all(debug_dir)
            .map_err(|e| format!("create debug dir {}: {e}", debug_dir.display()))?;
        save_gray(
            &debug_dir.join("01_tone_base.png"),
            bundle.width,
            bundle.height,
            &buffers.tone_base,
        )?;
        save_gray(
            &debug_dir.join("02_stylized.png"),
            bundle.width,
            bundle.height,
            &buffers.stylized,
        )?;
        save_gray(
            &debug_dir.join("03_quantized.png"),
            bundle.width,
            bundle.height,
            &buffers.quantized,
        )?;
        if let Some(sun_map) = optional.sun_light.as_ref() {
            save_gray(
                &debug_dir.join("00_sun_relight.png"),
                bundle.width,
                bundle.height,
                sun_map,
            )?;
        }
    }

    Ok(())
}
