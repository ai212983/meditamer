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

pub(crate) fn parse_render_args<I>(args: I) -> Result<Config, String>
where
    I: IntoIterator<Item = String>,
{
    let mut cfg = Config::default();
    let mut it = args.into_iter();

    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--preset" => apply_preset(
                &mut cfg,
                RenderPreset::from_str(&next_value("--preset", &mut it)?)?,
            ),
            "--bundle" => cfg.bundle = PathBuf::from(next_value("--bundle", &mut it)?),
            "--out" => cfg.out = PathBuf::from(next_value("--out", &mut it)?),
            "--mode" => cfg.mode = OutputMode::from_str(&next_value("--mode", &mut it)?)?,
            "--dither" => cfg.dither = DitherMode::from_str(&next_value("--dither", &mut it)?)?,
            "--edge-strength" => {
                cfg.edge_strength =
                    parse_num(next_value("--edge-strength", &mut it)?, "--edge-strength")?
            }
            "--fog-strength" => {
                cfg.fog_strength =
                    parse_num(next_value("--fog-strength", &mut it)?, "--fog-strength")?
            }
            "--stroke-strength" => {
                cfg.stroke_strength = parse_num(
                    next_value("--stroke-strength", &mut it)?,
                    "--stroke-strength",
                )?
            }
            "--paper-strength" => {
                cfg.paper_strength =
                    parse_num(next_value("--paper-strength", &mut it)?, "--paper-strength")?
            }
            "--tone-curve" => {
                cfg.tone_curve = ToneCurve::from_str(&next_value("--tone-curve", &mut it)?)?
            }
            "--sun-strength" => {
                cfg.sun_strength =
                    parse_num(next_value("--sun-strength", &mut it)?, "--sun-strength")?
            }
            "--sun-azimuth-deg" => {
                cfg.sun_azimuth_deg = parse_num(
                    next_value("--sun-azimuth-deg", &mut it)?,
                    "--sun-azimuth-deg",
                )?
            }
            "--sun-elevation-deg" => {
                cfg.sun_elevation_deg = parse_num(
                    next_value("--sun-elevation-deg", &mut it)?,
                    "--sun-elevation-deg",
                )?
            }
            "--save-debug" => {
                cfg.save_debug = Some(PathBuf::from(next_value("--save-debug", &mut it)?))
            }
            "--dump-channels" => {
                cfg.dump_channels = Some(PathBuf::from(next_value("--dump-channels", &mut it)?))
            }
            "--ghost-from" => {
                cfg.ghost_from = Some(PathBuf::from(next_value("--ghost-from", &mut it)?))
            }
            "--ghost-alpha" => {
                cfg.ghost_alpha = parse_num(next_value("--ghost-alpha", &mut it)?, "--ghost-alpha")?
            }
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            _ => return Err(format!("unknown render arg '{arg}'")),
        }
    }

    Ok(cfg)
}

pub(crate) fn mode_name(mode: OutputMode) -> &'static str {
    match mode {
        OutputMode::Mono1 => "mono1",
        OutputMode::Gray3 => "gray3",
        OutputMode::Gray4 => "gray4",
        OutputMode::Gray8 => "gray8",
    }
}

fn apply_preset(cfg: &mut Config, preset: RenderPreset) {
    match preset {
        RenderPreset::SumiE => {
            cfg.mode = OutputMode::Gray3;
            cfg.dither = DitherMode::Bayer4;
            cfg.edge_strength = 148;
            cfg.fog_strength = 98;
            cfg.stroke_strength = 54;
            cfg.paper_strength = 38;
            cfg.tone_curve = ToneCurve::SumiE;
            if cfg.sun_strength == 0 {
                cfg.sun_strength = 136;
            }
        }
    }
}

pub(crate) fn next_value<I>(flag: &str, it: &mut I) -> Result<String, String>
where
    I: Iterator<Item = String>,
{
    it.next().ok_or_else(|| format!("missing value for {flag}"))
}

fn parse_num<T>(raw: String, name: &str) -> Result<T, String>
where
    T: core::str::FromStr,
{
    raw.parse::<T>()
        .map_err(|_| format!("invalid numeric value for {name}: {raw}"))
}

pub(crate) fn print_help() {
    println!(
        "scene_viewer\n\n
actions:\n  render   Render a .scenebundle into a grayscale PNG using device-like compositing\n  inspect  Print bundle summary\n\n
action: render\n  --bundle FILE           Input bundle (default: tools/scene_maker/out/scene.scenebundle)\n  --out FILE              Output PNG (default: tools/scene_viewer/out/render.png)\n  --mode MODE             mono1|gray3|gray4|gray8 (default: gray3)\n  --dither MODE           none|bayer4 (default: bayer4)\n  --edge-strength N       0..255 (default: 96)\n  --fog-strength N        0..255 (default: 72)\n  --stroke-strength N     0..255 (default: 24)\n  --tone-curve MODE       linear|wash|filmic (default: wash)\n  --save-debug DIR        Save intermediates (tone base / stylized / quantized)\n  --dump-channels DIR     Save decoded source channels\n  --ghost-from FILE       Prior rendered frame for ghosting simulation\n  --ghost-alpha N         0..255 blend amount from prior frame (default: 0)\n\n
action: inspect\n  --bundle FILE           Bundle path"
    );
}
