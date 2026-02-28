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
        if handle_render_io_flags(&mut cfg, arg.as_str(), &mut it)? {
            continue;
        }
        if handle_render_style_flags(&mut cfg, arg.as_str(), &mut it)? {
            continue;
        }
        if handle_render_sun_flags(&mut cfg, arg.as_str(), &mut it)? {
            continue;
        }
        if handle_render_debug_ghost_flags(&mut cfg, arg.as_str(), &mut it)? {
            continue;
        }
        return Err(format!("unknown render arg '{arg}'"));
    }

    Ok(cfg)
}

fn handle_render_io_flags<I>(cfg: &mut Config, arg: &str, it: &mut I) -> Result<bool, String>
where
    I: Iterator<Item = String>,
{
    match arg {
        "--bundle" => {
            cfg.bundle = PathBuf::from(next_value("--bundle", it)?);
            Ok(true)
        }
        "--out" => {
            cfg.out = PathBuf::from(next_value("--out", it)?);
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn handle_render_style_flags<I>(cfg: &mut Config, arg: &str, it: &mut I) -> Result<bool, String>
where
    I: Iterator<Item = String>,
{
    match arg {
        "--preset" => {
            apply_preset(cfg, RenderPreset::from_str(&next_value("--preset", it)?)?);
            Ok(true)
        }
        "--mode" => {
            cfg.mode = OutputMode::from_str(&next_value("--mode", it)?)?;
            Ok(true)
        }
        "--dither" => {
            cfg.dither = DitherMode::from_str(&next_value("--dither", it)?)?;
            Ok(true)
        }
        "--edge-strength" => {
            cfg.edge_strength = parse_num(next_value("--edge-strength", it)?, "--edge-strength")?;
            Ok(true)
        }
        "--fog-strength" => {
            cfg.fog_strength = parse_num(next_value("--fog-strength", it)?, "--fog-strength")?;
            Ok(true)
        }
        "--stroke-strength" => {
            cfg.stroke_strength =
                parse_num(next_value("--stroke-strength", it)?, "--stroke-strength")?;
            Ok(true)
        }
        "--paper-strength" => {
            cfg.paper_strength =
                parse_num(next_value("--paper-strength", it)?, "--paper-strength")?;
            Ok(true)
        }
        "--tone-curve" => {
            cfg.tone_curve = ToneCurve::from_str(&next_value("--tone-curve", it)?)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn handle_render_sun_flags<I>(cfg: &mut Config, arg: &str, it: &mut I) -> Result<bool, String>
where
    I: Iterator<Item = String>,
{
    match arg {
        "--sun-strength" => {
            cfg.sun_strength = parse_num(next_value("--sun-strength", it)?, "--sun-strength")?;
            Ok(true)
        }
        "--sun-azimuth-deg" => {
            cfg.sun_azimuth_deg =
                parse_num(next_value("--sun-azimuth-deg", it)?, "--sun-azimuth-deg")?;
            Ok(true)
        }
        "--sun-elevation-deg" => {
            cfg.sun_elevation_deg = parse_num(
                next_value("--sun-elevation-deg", it)?,
                "--sun-elevation-deg",
            )?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn handle_render_debug_ghost_flags<I>(
    cfg: &mut Config,
    arg: &str,
    it: &mut I,
) -> Result<bool, String>
where
    I: Iterator<Item = String>,
{
    match arg {
        "--save-debug" => {
            cfg.save_debug = Some(PathBuf::from(next_value("--save-debug", it)?));
            Ok(true)
        }
        "--dump-channels" => {
            cfg.dump_channels = Some(PathBuf::from(next_value("--dump-channels", it)?));
            Ok(true)
        }
        "--ghost-from" => {
            cfg.ghost_from = Some(PathBuf::from(next_value("--ghost-from", it)?));
            Ok(true)
        }
        "--ghost-alpha" => {
            cfg.ghost_alpha = parse_num(next_value("--ghost-alpha", it)?, "--ghost-alpha")?;
            Ok(true)
        }
        "--help" | "-h" => {
            print_help();
            std::process::exit(0);
        }
        _ => Ok(false),
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_render_args_uses_defaults() {
        let cfg = parse_render_args(Vec::<String>::new()).expect("parse defaults");
        assert_eq!(cfg.edge_strength, 96);
        assert_eq!(cfg.fog_strength, 72);
        assert_eq!(cfg.stroke_strength, 24);
        assert!(matches!(cfg.mode, OutputMode::Gray3));
        assert!(matches!(cfg.dither, DitherMode::Bayer4));
    }

    #[test]
    fn parse_render_args_sumi_e_preset_applies_defaults() {
        let cfg = parse_render_args(vec!["--preset".to_owned(), "sumi-e".to_owned()])
            .expect("parse preset");
        assert!(matches!(cfg.mode, OutputMode::Gray3));
        assert!(matches!(cfg.dither, DitherMode::Bayer4));
        assert_eq!(cfg.edge_strength, 148);
        assert_eq!(cfg.fog_strength, 98);
        assert_eq!(cfg.stroke_strength, 54);
        assert_eq!(cfg.paper_strength, 38);
        assert_eq!(cfg.sun_strength, 136);
    }

    #[test]
    fn parse_render_args_unknown_arg_fails() {
        let err = match parse_render_args(vec!["--wat".to_owned()]) {
            Ok(_) => panic!("unknown arg should fail"),
            Err(err) => err,
        };
        assert!(err.contains("unknown render arg"));
    }

    #[test]
    fn parse_render_args_missing_value_fails() {
        let err = match parse_render_args(vec!["--mode".to_owned()]) {
            Ok(_) => panic!("missing value should fail"),
            Err(err) => err,
        };
        assert!(err.contains("missing value for --mode"));
    }
}
