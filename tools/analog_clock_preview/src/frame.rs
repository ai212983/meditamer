//! Host-owned one-entry frame cache and the single render entry every
//! frontend (CLI snapshots, bench, server) shares.
//!
//! The cache keys on exact base inputs: size, background/clock methods,
//! and full byte equality of the base gray plus base region planes. The
//! base rows are time-invariant (unoccluded dial, no hands), so a time
//! step is a cache hit: only the static diffusions are reused, the base
//! rows are still re-rendered every frame for the exact key check. Any
//! base input change (light, dial size, methods, dimensions) invalidates
//! the one entry. Cost saved is the two static diffusions, not the base
//! render. The cached entry holds base_gray (N bytes) + base_regions as
//! u8 (N bytes) + static_bg (ceil(N/8)) + optional static_clock
//! (ceil(N/8)), N = width*height: at most 810000 B (~791 KiB) at
//! 600x600, at most 270000 B at 400x300, plus Vec metadata. This is a
//! host-only memory/speed convenience (one bounded host-heap entry),
//! never a device-memory-fit claim and not a performance promise.
//!
//! The cache lives in a host-owned [`FrameCache`] (renderer struct or
//! server loop), never a global static, and never grows: one entry,
//! replaced on any base input change. All buffers are host heap.

use analog_clock::{Candidates, CompositionMode, DitherAlgorithm, DitherRegion};

use crate::comparison::ComparisonMetrics;
use crate::composition::{self, ComposedPlanes};

/// One cached static entry: the exact base inputs plus their completed
/// diffusions. Reused only on full byte equality of both base planes.
struct CachedBase {
    width: u32,
    height: u32,
    background: DitherAlgorithm,
    clock: DitherAlgorithm,
    base_gray: Vec<u8>,
    base_regions: Vec<DitherRegion>,
    static_bg: Vec<u8>,
    static_clock: Option<Vec<u8>>,
}

/// Bounded one-entry static cache, host-owned (never global).
#[derive(Default)]
pub struct FrameCache {
    entry: Option<CachedBase>,
}

/// Cached static diffusions plus whether they were reused.
type StaticReuse = (Vec<u8>, Option<Vec<u8>>, bool);

impl FrameCache {
    pub fn new() -> Self {
        Self { entry: None }
    }

    /// Reuses the cached static diffusions on an exact base-input hit,
    /// else diffuses and stores the one entry. Returns the static planes
    /// plus whether they were reused.
    fn statics(
        &mut self,
        params: &crate::params::PreviewParams,
        planes: &ComposedPlanes,
    ) -> Result<StaticReuse, String> {
        let hit = self.entry.as_ref().is_some_and(|e| {
            e.width == params.width
                && e.height == params.height
                && e.background == params.regions.background
                && e.clock == params.regions.clock
                && e.base_gray == planes.base_gray
                && e.base_regions == planes.base_regions
        });
        if hit {
            let e = self.entry.as_ref().expect("hit implies entry");
            return Ok((e.static_bg.clone(), e.static_clock.clone(), true));
        }
        let (bg, clock) = composition::static_bits(params, planes)?;
        self.entry = Some(CachedBase {
            width: params.width,
            height: params.height,
            background: params.regions.background,
            clock: params.regions.clock,
            base_gray: planes.base_gray.clone(),
            base_regions: planes.base_regions.clone(),
            static_bg: bg.clone(),
            static_clock: clock.clone(),
        });
        Ok((bg, clock, false))
    }
}

/// One rendered time: current gray for metrics/PNG, composed bits,
/// footprint flags (`0` empty else `1`/`2`), merged static base bits for
/// disocclusion checks, and whether the static diffusions were reused.
pub struct FrameBits {
    pub gray: Vec<u8>,
    pub bits: Vec<u8>,
    pub footprint: Vec<u8>,
    pub base_bits: Vec<u8>,
    pub base_reused: bool,
}

/// Renders one time through the requested composition. Legacy flows
/// through [`crate::params::render_dithered`] untouched (byte-exact);
/// every other mode shares the [`crate::composition`] helpers.
pub fn render_frame(
    cache: &mut FrameCache,
    params: &crate::params::PreviewParams,
    hands: &analog_clock::Hands<'_>,
) -> Result<FrameBits, String> {
    match params.composition {
        CompositionMode::Legacy => {
            let bits = crate::params::render_dithered(params, hands);
            if !params.diagnostics && params.mode != crate::params::OutputMode::Changes {
                return Ok(FrameBits {
                    gray: crate::params::render_gray(params, hands),
                    bits,
                    footprint: Vec::new(),
                    base_bits: Vec::new(),
                    base_reused: false,
                });
            }
            // Diagnostics (or the changes view) needs the footprint and a
            // stationary base: the base planes regionally dithered with the
            // current profiles (the legacy-style stationary candidate).
            let planes = composition::render_composed(params, hands);
            let gray = planes.full_gray.clone();
            let footprint = composition::footprint_flags(&planes.footprint);
            let base_bits = regional_base_bits(params, &planes)?;
            Ok(FrameBits {
                gray,
                bits,
                footprint,
                base_bits,
                base_reused: false,
            })
        }
        CompositionMode::Reference => {
            let planes = composition::render_composed(params, hands);
            let dynamic = composition::reference_bits(params, &planes.full_gray)?;
            let candidates = Candidates {
                static_bg: &dynamic,
                static_clock: None,
                dynamic: &dynamic,
                mask: None,
            };
            let bits =
                composition::select_bits(CompositionMode::Reference, params, &planes, &candidates)?;
            let (footprint, base_bits) = if comparison_needed(params) {
                let base_bits = composition::flat_bits(
                    params.width,
                    params.height,
                    &planes.base_gray,
                    analog_clock::DitherAlgorithm::BlueNoise,
                )?;
                (composition::footprint_flags(&planes.footprint), base_bits)
            } else {
                (Vec::new(), Vec::new())
            };
            Ok(FrameBits {
                gray: planes.full_gray,
                bits,
                footprint,
                base_bits,
                base_reused: false,
            })
        }
        CompositionMode::Replacement | CompositionMode::Removal => {
            let planes = composition::render_composed(params, hands);
            let (static_bg, static_clock, reused) = cache.statics(params, &planes)?;
            let dynamic = composition::dynamic_bits(params, &planes)?;
            let mask = if params.composition == CompositionMode::Removal {
                Some(composition::mask_bits(params, &planes)?)
            } else {
                None
            };
            let candidates = Candidates {
                static_bg: &static_bg,
                static_clock: static_clock.as_deref(),
                dynamic: &dynamic,
                mask: mask.as_deref(),
            };
            let bits = composition::select_bits(params.composition, params, &planes, &candidates)?;
            let base_bits = if comparison_needed(params) {
                composition::merged_base_bits(
                    params.composition,
                    params,
                    &planes,
                    &static_bg,
                    static_clock.as_deref(),
                    &dynamic,
                    mask.as_deref(),
                )?
            } else {
                Vec::new()
            };
            let footprint = if comparison_needed(params) {
                composition::footprint_flags(&planes.footprint)
            } else {
                Vec::new()
            };
            Ok(FrameBits {
                gray: planes.full_gray,
                bits,
                footprint,
                base_bits,
                base_reused: reused,
            })
        }
    }
}

/// Legacy-style stationary base candidate for diagnostics: the base gray
/// regionally dithered with the current profiles (same shared pass as the
/// dynamic candidate, over the static planes).
fn regional_base_bits(
    params: &crate::params::PreviewParams,
    planes: &ComposedPlanes,
) -> Result<Vec<u8>, String> {
    let need = analog_clock::packed_bits_len(params.width, params.height)
        .ok_or_else(|| "frame overflows packed bits".to_string())?;
    let scratch = analog_clock::regional_scratch_len(params.width as usize)
        .ok_or_else(|| "validated width overflows regional scratch".to_string())?;
    let mut errors = vec![0f32; scratch];
    let mut bits = vec![0u8; need];
    {
        let mut canvas =
            raster::BitCanvas::new(params.width as i32, params.height as i32, &mut bits)
                .ok_or_else(|| "output buffer mismatch".to_string())?;
        analog_clock::dither_regions(
            params.width,
            params.height,
            &planes.base_gray,
            &planes.base_regions,
            &params.regions,
            &mut errors,
            &mut canvas,
        )
        .map_err(|e| format!("base dither failed: {e}"))?;
    }
    Ok(bits)
}

fn comparison_needed(params: &crate::params::PreviewParams) -> bool {
    params.diagnostics || params.mode == crate::params::OutputMode::Changes
}

/// Current frame plus, when the mode or the diagnostics flag asks for it,
/// the reference-time frame rendered with the same scene, profiles, and
/// settings (only the time differs), with comparison metrics.
pub struct FramePair {
    pub current: FrameBits,
    pub previous: Option<FrameBits>,
    pub metrics: Option<ComparisonMetrics>,
}

pub fn render_pair(
    cache: &mut FrameCache,
    params: &crate::params::PreviewParams,
    hands: &analog_clock::Hands<'_>,
) -> Result<FramePair, String> {
    let current = render_frame(cache, params, hands)?;
    if !comparison_needed(params) {
        return Ok(FramePair {
            current,
            previous: None,
            metrics: None,
        });
    }
    let mut prev_params = params.clone();
    prev_params.hours = params.compare_hours;
    prev_params.minutes = params.compare_minutes;
    let previous = render_frame(cache, &prev_params, hands)?;
    let metrics = crate::comparison::analyze(
        params.width,
        params.height,
        &current.gray,
        &current.bits,
        &previous.bits,
        &current.footprint,
        &previous.footprint,
        &current.base_bits,
    )?;
    Ok(FramePair {
        current,
        previous: Some(previous),
        metrics: Some(metrics),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real_hands() -> crate::assets::DecodedHands {
        let hands = crate::assets::load_hands(&crate::assets::default_asset_dir())
            .expect("repo sprite assets decode");
        assert!(hands.as_hands().validate(), "repo assets validate");
        hands
    }

    fn borrowed(hands: &crate::assets::DecodedHands) -> analog_clock::Hands<'_> {
        crate::params::hands(&crate::params::PreviewParams::default(), hands)
    }

    fn sized(w: u32, h: u32, composition: CompositionMode) -> crate::params::PreviewParams {
        let mut params = crate::params::PreviewParams::default();
        params.apply("w", &w.to_string()).expect("w");
        params.apply("h", &h.to_string()).expect("h");
        params.apply("time", "10:09").expect("time");
        params.apply("compare_time", "10:10").expect("compare_time");
        params.apply("diagnostics", "1").expect("diagnostics");
        params.composition = composition;
        params
    }

    #[test]
    fn replacement_and_removal_change_only_inside_footprint() {
        let hands = real_hands();
        let borrowed = borrowed(&hands);
        for (w, h) in [(600u32, 600u32), (400u32, 300u32)] {
            for mode in [CompositionMode::Replacement, CompositionMode::Removal] {
                let params = sized(w, h, mode);
                let pair =
                    render_pair(&mut FrameCache::new(), &params, &borrowed).expect("render pair");
                let m = pair.metrics.expect("diagnostics metrics");
                assert!(
                    m.changed_total > 0,
                    "{w}x{h} {mode:?}: minute step changed nothing"
                );
                assert_eq!(
                    m.changed_outside, 0,
                    "{w}x{h} {mode:?}: changes outside footprint"
                );
                assert_eq!(
                    m.disocclusion_mismatches, 0,
                    "{w}x{h} {mode:?}: stale base restore"
                );
            }
        }
    }

    #[test]
    fn rerender_is_deterministic_and_hits_cache() {
        let hands = real_hands();
        let borrowed = borrowed(&hands);
        let params = sized(400, 300, CompositionMode::Replacement);
        let mut cache = FrameCache::new();
        let first = render_pair(&mut cache, &params, &borrowed).expect("first");
        let second = render_pair(&mut cache, &params, &borrowed).expect("second");
        assert!(second.current.base_reused, "repeated base must hit");
        assert_eq!(
            first.current.bits, second.current.bits,
            "bits not deterministic"
        );
        assert_eq!(
            first.current.gray, second.current.gray,
            "gray not deterministic"
        );
    }

    fn miss_after(label: &str, mutate: impl Fn(&mut crate::params::PreviewParams)) {
        let hands = real_hands();
        let borrowed = borrowed(&hands);
        let base = sized(400, 300, CompositionMode::Replacement);
        let mut cache = FrameCache::new();
        let first = render_frame(&mut cache, &base, &borrowed).expect("base");
        assert!(!first.base_reused, "cold cache must miss");
        let rerun = render_frame(&mut cache, &base, &borrowed).expect("rerun");
        assert!(rerun.base_reused, "same inputs must hit");
        let mut changed = base.clone();
        mutate(&mut changed);
        let next = render_frame(&mut cache, &changed, &borrowed).expect(label);
        assert!(!next.base_reused, "{label} must invalidate");
    }

    #[test]
    fn base_inputs_invalidate_the_entry() {
        miss_after("light", |p| {
            p.apply("light_x", "0.0").expect("light");
        });
        miss_after("method", |p| {
            p.apply("clock_dither", "none").expect("method");
        });
        miss_after("size", |p| {
            p.apply("w", "401").expect("size");
        });
    }

    #[test]
    fn legacy_frame_matches_direct_functions() {
        let hands = real_hands();
        let borrowed = borrowed(&hands);
        let params = sized(400, 300, CompositionMode::Legacy);
        let frame = render_frame(&mut FrameCache::new(), &params, &borrowed).expect("legacy");
        assert_eq!(frame.gray, crate::params::render_gray(&params, &borrowed));
        assert_eq!(
            frame.bits,
            crate::params::render_dithered(&params, &borrowed)
        );
    }

    #[test]
    fn gray_is_identical_every_composition() {
        let hands = real_hands();
        let borrowed = borrowed(&hands);
        for mode in [
            CompositionMode::Legacy,
            CompositionMode::Reference,
            CompositionMode::Replacement,
            CompositionMode::Removal,
        ] {
            let params = sized(400, 300, mode);
            let frame = render_frame(&mut FrameCache::new(), &params, &borrowed).expect("render");
            assert_eq!(
                frame.gray,
                crate::params::render_gray(&params, &borrowed),
                "{mode:?}: gray must not depend on composition"
            );
        }
    }

    /// Asset-mode params: the real loader's dial attached, so the scene
    /// borrows the supplied image on both native sizes.
    fn dialed(
        w: u32,
        h: u32,
        composition: CompositionMode,
        hands: &crate::assets::DecodedHands,
    ) -> crate::params::PreviewParams {
        let mut params = sized(w, h, composition);
        params.dial = hands.dial.clone();
        params
    }

    #[test]
    fn dial_gray_is_identical_every_composition() {
        use analog_clock::DitherRegion;
        let hands = real_hands();
        assert!(
            hands.dial.as_ref().is_some_and(|d| d.as_dial().validate()),
            "supplied dial must decode valid"
        );
        let borrowed = borrowed(&hands);
        for (w, h) in [(600u32, 600u32), (400u32, 300u32)] {
            for mode in [
                CompositionMode::Legacy,
                CompositionMode::Reference,
                CompositionMode::Replacement,
                CompositionMode::Removal,
            ] {
                let params = dialed(w, h, mode, &hands);
                let frame =
                    render_frame(&mut FrameCache::new(), &params, &borrowed).expect("render");
                assert_eq!(
                    frame.gray,
                    crate::params::render_gray(&params, &borrowed),
                    "{w}x{h} {mode:?}: dial gray must not depend on composition"
                );
            }
            // With the supplied image the baked marks are Background: no
            // pixel may report the Clock region.
            let params = dialed(w, h, CompositionMode::Legacy, &hands);
            let (_, regions) = crate::params::render_planes(&params, &borrowed);
            assert!(
                !regions.contains(&DitherRegion::Clock),
                "{w}x{h}: dial image must leave the Clock region empty"
            );
        }
    }

    #[test]
    fn dial_changes_only_inside_footprint() {
        let hands = real_hands();
        let borrowed = borrowed(&hands);
        for (w, h) in [(600u32, 600u32), (400u32, 300u32)] {
            for mode in [CompositionMode::Replacement, CompositionMode::Removal] {
                let params = dialed(w, h, mode, &hands);
                let pair =
                    render_pair(&mut FrameCache::new(), &params, &borrowed).expect("render pair");
                let m = pair.metrics.expect("diagnostics metrics");
                assert!(
                    m.changed_total > 0,
                    "{w}x{h} {mode:?}: dial minute step changed nothing"
                );
                assert_eq!(
                    m.changed_outside, 0,
                    "{w}x{h} {mode:?}: dial changes outside footprint"
                );
                assert_eq!(
                    m.disocclusion_mismatches, 0,
                    "{w}x{h} {mode:?}: dial stale base restore"
                );
            }
        }
    }

    #[test]
    fn dial_image_changes_the_picture() {
        let hands = real_hands();
        let borrowed = borrowed(&hands);
        // Attaching the supplied image must move the picture (and removing
        // it must restore the procedural bytes exactly).
        let mut params = sized(600, 600, CompositionMode::Legacy);
        let procedural = crate::params::render_gray(&params, &borrowed);
        params.dial = hands.dial.clone();
        let supplied = crate::params::render_gray(&params, &borrowed);
        assert_ne!(supplied, procedural, "dial image had no effect");
        params.dial = None;
        assert_eq!(
            crate::params::render_gray(&params, &borrowed),
            procedural,
            "removing the dial must restore procedural bytes"
        );
    }
}
