fn dump_channels_if_requested(
    cfg: &Config,
    bundle: &Bundle,
    inputs: &RenderInputs<'_>,
) -> Result<(), String> {
    let Some(out_dir) = cfg.dump_channels.as_ref() else {
        return Ok(());
    };

    fs::create_dir_all(out_dir)
        .map_err(|e| format!("create dump channels dir {}: {e}", out_dir.display()))?;
    save_gray(
        &out_dir.join("albedo.png"),
        bundle.width,
        bundle.height,
        inputs.albedo,
    )?;
    save_gray(
        &out_dir.join("light.png"),
        bundle.width,
        bundle.height,
        inputs.light,
    )?;
    save_gray(
        &out_dir.join("ao.png"),
        bundle.width,
        bundle.height,
        inputs.ao,
    )?;
    save_gray(
        &out_dir.join("depth.png"),
        bundle.width,
        bundle.height,
        inputs.depth,
    )?;
    save_gray(
        &out_dir.join("edge.png"),
        bundle.width,
        bundle.height,
        inputs.edge,
    )?;
    save_gray(
        &out_dir.join("mask.png"),
        bundle.width,
        bundle.height,
        inputs.mask,
    )?;
    save_gray(
        &out_dir.join("stroke.png"),
        bundle.width,
        bundle.height,
        inputs.stroke,
    )?;
    if let Some((nx, ny)) = inputs.normal_xy {
        save_gray(
            &out_dir.join("normal_x.png"),
            bundle.width,
            bundle.height,
            nx,
        )?;
        save_gray(
            &out_dir.join("normal_y.png"),
            bundle.width,
            bundle.height,
            ny,
        )?;
    }
    Ok(())
}

fn prepare_optional_maps(
    cfg: &Config,
    bundle: &Bundle,
    inputs: &RenderInputs<'_>,
    width: usize,
    height: usize,
) -> Result<OptionalMaps, String> {
    let sun_light = if cfg.sun_strength > 0 {
        Some(build_depth_relit_map(
            inputs.depth,
            inputs.normal_xy,
            width,
            height,
            cfg.sun_azimuth_deg,
            cfg.sun_elevation_deg,
        ))
    } else {
        None
    };

    let ghost_prev = if let Some(path) = cfg.ghost_from.as_ref() {
        Some(load_grayscale_resize(path, bundle.width, bundle.height)?)
    } else {
        None
    };

    Ok(OptionalMaps {
        sun_light,
        ghost_prev,
    })
}
