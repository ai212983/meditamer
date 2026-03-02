use std::path::PathBuf;

#[derive(Clone, Copy, Debug)]
pub(crate) enum DitherMode {
    None,
    Bayer4,
}

impl DitherMode {
    fn from_str(raw: &str) -> Result<Self, String> {
        match raw {
            "none" => Ok(Self::None),
            "bayer4" => Ok(Self::Bayer4),
            _ => Err(format!("invalid dither '{raw}', expected none|bayer4")),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum OutputMode {
    Mono1,
    Gray3,
    Gray4,
    Gray8,
}

impl OutputMode {
    fn from_str(raw: &str) -> Result<Self, String> {
        match raw {
            "mono1" => Ok(Self::Mono1),
            "gray3" => Ok(Self::Gray3),
            "gray4" => Ok(Self::Gray4),
            "gray8" => Ok(Self::Gray8),
            _ => Err(format!(
                "invalid mode '{raw}', expected mono1|gray3|gray4|gray8"
            )),
        }
    }

    pub(crate) fn levels(self) -> u16 {
        match self {
            Self::Mono1 => 2,
            Self::Gray3 => 8,
            Self::Gray4 => 16,
            Self::Gray8 => 256,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ToneCurve {
    Linear,
    Wash,
    Filmic,
    SumiE,
}

impl ToneCurve {
    fn from_str(raw: &str) -> Result<Self, String> {
        match raw {
            "linear" => Ok(Self::Linear),
            "wash" => Ok(Self::Wash),
            "filmic" => Ok(Self::Filmic),
            "sumi-e" => Ok(Self::SumiE),
            _ => Err(format!(
                "invalid tone curve '{raw}', expected linear|wash|filmic|sumi-e"
            )),
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum RenderPreset {
    SumiE,
}

impl RenderPreset {
    fn from_str(raw: &str) -> Result<Self, String> {
        match raw {
            "sumi-e" => Ok(Self::SumiE),
            _ => Err(format!("invalid preset '{raw}', expected sumi-e")),
        }
    }
}

#[derive(Clone)]
pub(crate) struct Config {
    pub(crate) bundle: PathBuf,
    pub(crate) out: PathBuf,
    pub(crate) mode: OutputMode,
    pub(crate) dither: DitherMode,
    pub(crate) edge_strength: u8,
    pub(crate) fog_strength: u8,
    pub(crate) stroke_strength: u8,
    pub(crate) paper_strength: u8,
    pub(crate) tone_curve: ToneCurve,
    pub(crate) sun_strength: u8,
    pub(crate) sun_azimuth_deg: f32,
    pub(crate) sun_elevation_deg: f32,
    pub(crate) save_debug: Option<PathBuf>,
    pub(crate) dump_channels: Option<PathBuf>,
    pub(crate) ghost_from: Option<PathBuf>,
    pub(crate) ghost_alpha: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bundle: PathBuf::from("tools/scene_maker/out/scene.scenebundle"),
            out: PathBuf::from("tools/scene_viewer/out/render.png"),
            mode: OutputMode::Gray3,
            dither: DitherMode::Bayer4,
            edge_strength: 96,
            fog_strength: 72,
            stroke_strength: 24,
            paper_strength: 18,
            tone_curve: ToneCurve::Wash,
            sun_strength: 0,
            sun_azimuth_deg: 315.0,
            sun_elevation_deg: 35.0,
            save_debug: None,
            dump_channels: None,
            ghost_from: None,
            ghost_alpha: 0,
        }
    }
}
