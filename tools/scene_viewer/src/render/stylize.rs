pub(super) fn build_depth_relit_map(
    depth: &[u8],
    normal_xy: Option<(&[u8], &[u8])>,
    width: usize,
    height: usize,
    azimuth_deg: f32,
    elevation_deg: f32,
) -> Vec<u8> {
    let mut out = vec![0u8; depth.len()];
    let az = azimuth_deg.to_radians();
    let el = elevation_deg.to_radians().clamp(0.05, 1.5);
    let lx = el.cos() * az.cos();
    let ly = el.cos() * az.sin();
    let lz = el.sin();
    let side_weight = 0.82f32;
    let up_weight = 0.35f32;

    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            let x0 = x.saturating_sub(1);
            let x1 = (x + 1).min(width - 1);
            let y0 = y.saturating_sub(1);
            let y1 = (y + 1).min(height - 1);

            let (nxn, nyn, nzn) = if let Some((nx_map, ny_map)) = normal_xy {
                let nx = ((nx_map[idx] as f32) - 128.0) / 127.0;
                let ny = ((ny_map[idx] as f32) - 128.0) / 127.0;
                let nz = (1.0 - (nx * nx + ny * ny)).max(0.0).sqrt().max(0.05);
                let inv = (nx * nx + ny * ny + nz * nz).sqrt().recip();
                (nx * inv, ny * inv, nz * inv)
            } else {
                let dx = (depth[y * width + x1] as f32 - depth[y * width + x0] as f32) / 255.0;
                let dy = (depth[y1 * width + x] as f32 - depth[y0 * width + x] as f32) / 255.0;

                // Reconstruct a pseudo normal from depth and amplify lateral slope response.
                // Higher XY gain makes azimuth changes more visible in static-camera relighting.
                let nx = -dx * 11.0;
                let ny = -dy * 11.0;
                let nz = 0.34;

                let inv = (nx * nx + ny * ny + nz * nz).sqrt().recip();
                (nx * inv, ny * inv, nz * inv)
            };

            let side = (nxn * lx) + (nyn * ly);
            let up = nzn * lz;
            let lit = (side * side_weight) + (up * up_weight);
            // Add a subtle global directional ramp so azimuth is readable even on low-detail depth.
            let xf = if width > 1 {
                (x as f32 / (width - 1) as f32) * 2.0 - 1.0
            } else {
                0.0
            };
            let yf = if height > 1 {
                (y as f32 / (height - 1) as f32) * 2.0 - 1.0
            } else {
                0.0
            };
            let directional = (xf * lx) + (yf * ly);
            let shade = (0.62 + lit * 0.72 + directional * 0.28).clamp(0.10, 1.0);
            out[idx] = (shade * 255.0 + 0.5) as u8;
        }
    }

    out
}

pub(super) fn ink_brush_delta(
    idx: usize,
    x: usize,
    y: usize,
    stroke: u8,
    edge: u8,
    depth: u8,
    normal_xy: Option<(&[u8], &[u8])>,
    stroke_strength: u8,
) -> i16 {
    if stroke_strength == 0 {
        return 0;
    }

    let xf = x as f32;
    let yf = y as f32;
    let depth_f = (depth as f32) / 255.0;
    let edge_f = (edge as f32) / 255.0;
    let stroke_src = ((stroke as f32) - 128.0) / 127.0;

    let (tx, ty, nx, ny) = brush_basis(idx, normal_xy);

    // Vary stroke spacing with depth and contour strength.
    let freq_macro = 0.007 + (0.004 * (1.0 - depth_f));
    let freq_coarse = 0.016 + (0.012 * (1.0 - depth_f));
    let freq_fine = 0.058 + (0.034 * edge_f);

    let u = (xf * tx) + (yf * ty);
    let v = (xf * nx) + (yf * ny);

    let phase0 = hash01((x as i32) >> 4, (y as i32) >> 4, 0xA1B2_C3D4) * core::f32::consts::TAU;
    let phase1 = hash01((x as i32) >> 5, (y as i32) >> 5, 0x9E37_79B1) * core::f32::consts::TAU;
    let phase2 = hash01((x as i32) >> 6, (y as i32) >> 6, 0x7F4A_7C15) * core::f32::consts::TAU;
    let phase3 = hash01((x as i32) >> 7, (y as i32) >> 7, 0xC6A4_A793) * core::f32::consts::TAU;

    let line_macro = (u * freq_macro + phase2).sin();
    let line_coarse = (u * freq_coarse + phase0).sin();
    let line_fine = ((u * freq_fine) + (v * 0.011) + phase1).sin();
    let cross_wash =
        ((u * (freq_macro * 0.8) + phase2).sin()) * ((v * (freq_macro * 0.55) + phase3).cos());

    // Low-frequency patchiness prevents uniformly repeated texture.
    let patch = (hash01((x as i32) >> 5, (y as i32) >> 5, 0x85EB_CA77) * 2.0) - 1.0;
    let micro = (hash01(x as i32, y as i32, 0xC2B2_AE3D) * 2.0) - 1.0;

    let brush_mix = (line_macro * 0.24)
        + (line_coarse * 0.26)
        + (line_fine * 0.20)
        + (cross_wash * 0.18)
        + (patch * 0.08)
        + (micro * 0.04);
    let signal = ((stroke_src * 0.48) + (brush_mix * 0.52)).clamp(-1.0, 1.0);

    // Deliberately stylized regime: allow visible brush dominance that can bend silhouette perception.
    let strength_f = (stroke_strength as f32) / 255.0;
    let chaos_boost = 1.0 + (2.8 * strength_f.powf(1.05));
    let amp = (stroke_strength as f32)
        * (0.82 + (1.35 * edge_f) + (0.70 * (1.0 - depth_f)))
        * chaos_boost;
    let delta = (signal * amp).round() as i16;
    delta.clamp(-208, 208)
}

fn brush_basis(idx: usize, normal_xy: Option<(&[u8], &[u8])>) -> (f32, f32, f32, f32) {
    if let Some((nx_map, ny_map)) = normal_xy {
        let nx = ((nx_map[idx] as f32) - 128.0) / 127.0;
        let ny = ((ny_map[idx] as f32) - 128.0) / 127.0;
        let nlen = (nx * nx + ny * ny).sqrt();
        if nlen > 0.03 {
            let nnx = nx / nlen;
            let nny = ny / nlen;
            // Tangent is perpendicular to normal.
            return (-nny, nnx, nnx, nny);
        }
    }

    // Fallback orientation if normals are unavailable.
    (1.0, 0.0, 0.0, 1.0)
}

fn hash01(x: i32, y: i32, seed: u32) -> f32 {
    let mut v = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77) ^ seed;
    v ^= v >> 16;
    v = v.wrapping_mul(0x7FEB_352D);
    v ^= v >> 15;
    v = v.wrapping_mul(0x846C_A68B);
    v ^= v >> 16;
    (v as f32) / (u32::MAX as f32)
}

pub(super) fn paper_noise_u8(x: i32, y: i32) -> u8 {
    let mut v = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
    v ^= v >> 15;
    v = v.wrapping_mul(0xC2B2_AE3D);
    v ^= v >> 13;
    (v & 0xFF) as u8
}
