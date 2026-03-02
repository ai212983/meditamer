fn render_frame(
    cfg: &Config,
    width: usize,
    height: usize,
    inputs: &RenderInputs<'_>,
    optional: &OptionalMaps,
) -> RenderBuffers {
    let total = width * height;
    let tone_lut = build_tone_lut(cfg.tone_curve);
    let mut tone_base = vec![0u8; total];
    let mut stylized = vec![0u8; total];
    let mut quantized = vec![0u8; total];

    for y in 0..height {
        for x in 0..width {
            let i = y * width + x;
            let (base, stylized_px, quantized_px) =
                render_pixel(cfg, x, y, i, inputs, optional, &tone_lut);
            tone_base[i] = base;
            stylized[i] = stylized_px;
            quantized[i] = quantized_px;
        }
    }

    RenderBuffers {
        tone_base,
        stylized,
        quantized,
    }
}

fn render_pixel(
    cfg: &Config,
    x: usize,
    y: usize,
    i: usize,
    inputs: &RenderInputs<'_>,
    optional: &OptionalMaps,
    tone_lut: &[u8; 256],
) -> (u8, u8, u8) {
    let light_shaded = if let Some(sun_map) = optional.sun_light.as_ref() {
        mix_u8(inputs.light[i], sun_map[i], cfg.sun_strength)
    } else {
        inputs.light[i]
    };
    let base = mul8(mul8(inputs.albedo[i], light_shaded), inputs.ao[i]);

    let fog = mul8(inputs.depth[i], cfg.fog_strength);
    let fogged = mix_u8(base, 255, fog);

    let dark = mul8(inputs.edge[i], cfg.edge_strength);
    let edged = fogged.saturating_sub(dark);

    let stroke_delta = ink_brush_delta(
        i,
        x,
        y,
        inputs.stroke[i],
        inputs.edge[i],
        inputs.depth[i],
        inputs.normal_xy,
        cfg.stroke_strength,
    );
    let stroked = clamp_i16_to_u8((edged as i16) + stroke_delta);

    let paper_delta =
        ((paper_noise_u8(x as i32, y as i32) as i16) - 128) * (cfg.paper_strength as i16) / 255;
    let papered = clamp_i16_to_u8((stroked as i16) + paper_delta);
    let curved = tone_lut[papered as usize];
    let masked = mix_u8(255, curved, inputs.mask[i]);

    let stylized = if let Some(prev) = optional.ghost_prev.as_ref() {
        mix_u8(masked, prev[i], cfg.ghost_alpha)
    } else {
        masked
    };
    let quantized = quantize_u8(stylized, x as i32, y as i32, cfg.mode, cfg.dither);
    (base, stylized, quantized)
}
