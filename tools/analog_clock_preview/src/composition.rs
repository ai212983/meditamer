//! Host composition pipeline: one composed frame from the shared core.
//!
//! The core owns every lighting, dither, and bit-selection decision; this
//! module only owns host-heap buffers and call order. Legacy output flows
//! through [`crate::params::render_dithered`] untouched (byte-exact); the
//! experiment modes share one helper each for planes, candidates, and late
//! selection, so CLI snapshots, the bench, and the server render the same
//! bits. All buffers are standard host heap (`Vec`); nothing here touches
//! firmware, DRAM segments, or the network.

use analog_clock::{Candidates, CompositionMode, DitherAlgorithm, DitherRegion, Footprint};

/// One frame of composed core output: full-scene gray plus regions
/// (legacy-exact), unoccluded-dial base gray plus regions, and the typed
/// physical footprint. Built with one [`analog_clock::render_compose_row`]
/// call per row.
pub struct ComposedPlanes {
    pub full_gray: Vec<u8>,
    pub full_regions: Vec<DitherRegion>,
    pub base_gray: Vec<u8>,
    pub base_regions: Vec<DitherRegion>,
    pub footprint: Vec<Footprint>,
}

/// Composes gray, region, and footprint planes row by row through the
/// shared core. Dimensions come from validated params (64..=1200), so the
/// host heap planes are bounded; this full-frame prototype path is a host
/// convenience, not a device-memory-fit claim.
pub fn render_composed(
    params: &crate::params::PreviewParams,
    hands: &analog_clock::Hands<'_>,
) -> ComposedPlanes {
    let scene = params.scene();
    let (w, h) = (params.width as usize, params.height as usize);
    let mut full_gray = vec![0u8; w * h];
    let mut full_regions = vec![DitherRegion::Background; w * h];
    let mut base_gray = vec![0u8; w * h];
    let mut base_regions = vec![DitherRegion::Background; w * h];
    let mut footprint = vec![Footprint::NONE; w * h];
    for y in 0..params.height {
        let row = y as usize * w;
        analog_clock::render_compose_row(
            &scene,
            hands,
            y,
            &mut full_gray[row..row + w],
            &mut full_regions[row..row + w],
            &mut base_gray[row..row + w],
            &mut base_regions[row..row + w],
            &mut footprint[row..row + w],
        );
    }
    ComposedPlanes {
        full_gray,
        full_regions,
        base_gray,
        base_regions,
        footprint,
    }
}

/// Footprint flags for [`crate::comparison::analyze`]: `0` where the
/// footprint is empty, else `1` (hand) / `2` (shadow). The shadow flag only
/// marks real light blockage, never a stochastic region label.
pub fn footprint_flags(footprint: &[Footprint]) -> Vec<u8> {
    footprint
        .iter()
        .map(|fp| {
            if fp.hand() {
                1
            } else if fp.shadow() {
                2
            } else {
                0
            }
        })
        .collect()
}

fn diffusion_scratch(width: u32) -> Result<Vec<f32>, String> {
    analog_clock::regional_scratch_len(width as usize)
        .map(|n| vec![0f32; n])
        .ok_or_else(|| "validated width overflows regional scratch".to_string())
}

fn pack_bits(width: u32, height: u32) -> Result<Vec<u8>, String> {
    analog_clock::packed_bits_len(width, height)
        .map(|n| vec![0u8; n])
        .ok_or_else(|| "frame overflows packed bits".to_string())
}

fn canvas_for(width: u32, height: u32, bits: &mut [u8]) -> Result<raster::BitCanvas<'_>, String> {
    raster::BitCanvas::new(width as i32, height as i32, bits)
        .ok_or_else(|| "output buffer mismatch".to_string())
}

/// Dynamic candidate: the completed full-scene gray quantized exactly once
/// per pixel with that pixel's region profile (all four profiles), through
/// the shared regional core.
pub fn dynamic_bits(
    params: &crate::params::PreviewParams,
    planes: &ComposedPlanes,
) -> Result<Vec<u8>, String> {
    let mut errors = diffusion_scratch(params.width)?;
    let mut bits = pack_bits(params.width, params.height)?;
    {
        let mut canvas = canvas_for(params.width, params.height, &mut bits)?;
        analog_clock::dither_regions(
            params.width,
            params.height,
            &planes.full_gray,
            &planes.full_regions,
            &params.regions,
            &mut errors,
            &mut canvas,
        )
        .map_err(|e| format!("dynamic dither failed: {e}"))?;
    }
    Ok(bits)
}

/// Reference candidate: the completed full-scene gray dithered in one
/// uniform BlueNoise pass to completion (region labels ignored).
pub fn reference_bits(
    params: &crate::params::PreviewParams,
    full_gray: &[u8],
) -> Result<Vec<u8>, String> {
    flat_bits(
        params.width,
        params.height,
        full_gray,
        DitherAlgorithm::BlueNoise,
    )
}

/// One uniform algorithm over the whole gray plane to completion, through
/// the shared core. Backs both static candidates and the removal mask.
pub fn flat_bits(
    width: u32,
    height: u32,
    gray: &[u8],
    algo: DitherAlgorithm,
) -> Result<Vec<u8>, String> {
    let mut errors = diffusion_scratch(width)?;
    let mut bits = pack_bits(width, height)?;
    {
        let mut canvas = canvas_for(width, height, &mut bits)?;
        analog_clock::dither_flat(width, height, gray, algo, &mut errors, &mut canvas)
            .map_err(|e| format!("flat dither failed: {e}"))?;
    }
    Ok(bits)
}

/// Static candidates from the unoccluded-dial base gray: one complete whole
/// pass with the background method, plus a second complete pass with the
/// clock method for clock-furniture pixels when it differs (picked per
/// pixel via the base regions in [`analog_clock::compose_surface`]; the
/// clock selector is preserved, never folded into the background pass).
pub fn static_bits(
    params: &crate::params::PreviewParams,
    planes: &ComposedPlanes,
) -> Result<(Vec<u8>, Option<Vec<u8>>), String> {
    let bg = flat_bits(
        params.width,
        params.height,
        &planes.base_gray,
        params.regions.background,
    )?;
    let clock = if params.regions.clock == params.regions.background {
        None
    } else {
        Some(flat_bits(
            params.width,
            params.height,
            &planes.base_gray,
            params.regions.clock,
        )?)
    };
    Ok((bg, clock))
}

/// Removal mask: the keep-probability plane (`min(1, current/base)` in
/// quantizer tone domain via [`analog_clock::removal_keep_row`]) dithered
/// in one uniform pass with the shadows method to completion.
pub fn mask_bits(
    params: &crate::params::PreviewParams,
    planes: &ComposedPlanes,
) -> Result<Vec<u8>, String> {
    let (w, h) = (params.width as usize, params.height as usize);
    let mut keep = vec![0u8; w * h];
    for y in 0..params.height {
        let row = y as usize * w;
        analog_clock::removal_keep_row(
            &planes.base_gray[row..row + w],
            &planes.full_gray[row..row + w],
            &mut keep[row..row + w],
        );
    }
    flat_bits(params.width, params.height, &keep, params.regions.shadows)
}

/// Late-selects fully dithered packed candidates onto the surface through
/// the shared core: one binary pick per pixel by physical footprint, never
/// a stochastic shadow-strength blend of unshadowed base with already
/// shaded content (that blend under-shadows). The core merges the static
/// base wherever the footprint is empty.
pub fn select_bits(
    mode: CompositionMode,
    params: &crate::params::PreviewParams,
    planes: &ComposedPlanes,
    candidates: &Candidates<'_>,
) -> Result<Vec<u8>, String> {
    let mut bits = pack_bits(params.width, params.height)?;
    {
        let mut canvas = canvas_for(params.width, params.height, &mut bits)?;
        analog_clock::compose_surface(
            mode,
            params.width,
            params.height,
            &planes.base_regions,
            &planes.footprint,
            candidates,
            &mut canvas,
        )
        .map_err(|e| format!("compose failed: {e}"))?;
    }
    Ok(bits)
}

/// Merged static base for diagnostics: the same late selection with an
/// all-empty footprint, so the core (not host bit math) merges the static
/// candidates the way an uncovered pixel would restore them.
pub fn merged_base_bits(
    mode: CompositionMode,
    params: &crate::params::PreviewParams,
    planes: &ComposedPlanes,
    static_bg: &[u8],
    static_clock: Option<&[u8]>,
    dynamic: &[u8],
    mask: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    let empty = vec![Footprint::NONE; planes.footprint.len()];
    let mut bits = pack_bits(params.width, params.height)?;
    {
        let mut canvas = canvas_for(params.width, params.height, &mut bits)?;
        let candidates = Candidates {
            static_bg,
            static_clock,
            dynamic,
            mask,
        };
        analog_clock::compose_surface(
            mode,
            params.width,
            params.height,
            &planes.base_regions,
            &empty,
            &candidates,
            &mut canvas,
        )
        .map_err(|e| format!("base merge failed: {e}"))?;
    }
    Ok(bits)
}
