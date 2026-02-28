use std::path::PathBuf;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Compression {
    None,
    Rle,
}

impl Compression {
    pub(crate) fn as_u8(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Rle => 1,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Rle => "rle",
        }
    }

    pub(crate) fn from_str(raw: &str) -> Result<Self, String> {
        match raw {
            "none" => Ok(Self::None),
            "rle" => Ok(Self::Rle),
            _ => Err(format!("invalid compression '{raw}', expected none|rle")),
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub(crate) enum ChannelId {
    Albedo = 1,
    Light = 2,
    Ao = 3,
    Depth = 4,
    Edge = 5,
    Mask = 6,
    Stroke = 7,
    NormalX = 8,
    NormalY = 9,
}

#[derive(Clone, Copy)]
pub(crate) struct ChannelTemplate {
    pub(crate) id: ChannelId,
    pub(crate) name: &'static str,
    pub(crate) required: bool,
    pub(crate) default_value: u8,
}

pub(crate) const CHANNELS: [ChannelTemplate; 9] = [
    ChannelTemplate {
        id: ChannelId::Albedo,
        name: "albedo",
        required: true,
        default_value: 255,
    },
    ChannelTemplate {
        id: ChannelId::Light,
        name: "light",
        required: true,
        default_value: 255,
    },
    ChannelTemplate {
        id: ChannelId::Ao,
        name: "ao",
        required: false,
        default_value: 255,
    },
    ChannelTemplate {
        id: ChannelId::Depth,
        name: "depth",
        required: false,
        default_value: 0,
    },
    ChannelTemplate {
        id: ChannelId::Edge,
        name: "edge",
        required: false,
        default_value: 0,
    },
    ChannelTemplate {
        id: ChannelId::Mask,
        name: "mask",
        required: false,
        default_value: 255,
    },
    ChannelTemplate {
        id: ChannelId::Stroke,
        name: "stroke",
        required: false,
        default_value: 128,
    },
    ChannelTemplate {
        id: ChannelId::NormalX,
        name: "normal_x",
        required: false,
        default_value: 128,
    },
    ChannelTemplate {
        id: ChannelId::NormalY,
        name: "normal_y",
        required: false,
        default_value: 128,
    },
];

#[derive(Clone)]
pub(crate) struct BuildConfig {
    pub(crate) input_dir: PathBuf,
    pub(crate) out_bundle: PathBuf,
    pub(crate) metadata_out: PathBuf,
    pub(crate) width: u16,
    pub(crate) height: u16,
    pub(crate) strip_height: u16,
    pub(crate) compression: Compression,
    pub(crate) derive_edge: bool,
    pub(crate) albedo: Option<PathBuf>,
    pub(crate) light: Option<PathBuf>,
    pub(crate) ao: Option<PathBuf>,
    pub(crate) depth: Option<PathBuf>,
    pub(crate) edge: Option<PathBuf>,
    pub(crate) mask: Option<PathBuf>,
    pub(crate) stroke: Option<PathBuf>,
    pub(crate) normal_x: Option<PathBuf>,
    pub(crate) normal_y: Option<PathBuf>,
}

impl Default for BuildConfig {
    fn default() -> Self {
        Self {
            input_dir: PathBuf::from("tools/scene_maker/input"),
            out_bundle: PathBuf::from("tools/scene_maker/out/scene.scenebundle"),
            metadata_out: PathBuf::from("tools/scene_maker/out/scene.scenebundle.json"),
            width: 600,
            height: 600,
            strip_height: 32,
            compression: Compression::Rle,
            derive_edge: true,
            albedo: None,
            light: None,
            ao: None,
            depth: None,
            edge: None,
            mask: None,
            stroke: None,
            normal_x: None,
            normal_y: None,
        }
    }
}

pub(crate) fn parse_build_args<I>(args: I) -> Result<BuildConfig, String>
where
    I: IntoIterator<Item = String>,
{
    let mut cfg = BuildConfig::default();
    let mut it = args.into_iter();

    while let Some(arg) = it.next() {
        if handle_build_output_flags(&mut cfg, arg.as_str(), &mut it)? {
            continue;
        }
        if handle_build_dimension_flags(&mut cfg, arg.as_str(), &mut it)? {
            continue;
        }
        if handle_build_channel_override_flags(&mut cfg, arg.as_str(), &mut it)? {
            continue;
        }
        if handle_build_misc_flags(&mut cfg, arg.as_str(), &mut it)? {
            continue;
        }

        return Err(format!("unknown arg for build: {arg}"));
    }

    Ok(cfg)
}

fn handle_build_output_flags<I>(
    cfg: &mut BuildConfig,
    arg: &str,
    it: &mut I,
) -> Result<bool, String>
where
    I: Iterator<Item = String>,
{
    match arg {
        "--input" => {
            cfg.input_dir = PathBuf::from(next_value("--input", it)?);
            Ok(true)
        }
        "--out" => {
            cfg.out_bundle = PathBuf::from(next_value("--out", it)?);
            cfg.metadata_out = cfg.out_bundle.with_extension("scenebundle.json");
            Ok(true)
        }
        "--metadata" => {
            cfg.metadata_out = PathBuf::from(next_value("--metadata", it)?);
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn handle_build_dimension_flags<I>(
    cfg: &mut BuildConfig,
    arg: &str,
    it: &mut I,
) -> Result<bool, String>
where
    I: Iterator<Item = String>,
{
    match arg {
        "--width" => {
            cfg.width = parse_num(next_value("--width", it)?, "--width")?;
            Ok(true)
        }
        "--height" => {
            cfg.height = parse_num(next_value("--height", it)?, "--height")?;
            Ok(true)
        }
        "--strip-height" => {
            cfg.strip_height = parse_num(next_value("--strip-height", it)?, "--strip-height")?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn handle_build_channel_override_flags<I>(
    cfg: &mut BuildConfig,
    arg: &str,
    it: &mut I,
) -> Result<bool, String>
where
    I: Iterator<Item = String>,
{
    let path = match arg {
        "--albedo" => Some((&mut cfg.albedo, "--albedo")),
        "--light" => Some((&mut cfg.light, "--light")),
        "--ao" => Some((&mut cfg.ao, "--ao")),
        "--depth" => Some((&mut cfg.depth, "--depth")),
        "--edge" => Some((&mut cfg.edge, "--edge")),
        "--mask" => Some((&mut cfg.mask, "--mask")),
        "--stroke" => Some((&mut cfg.stroke, "--stroke")),
        "--normal-x" => Some((&mut cfg.normal_x, "--normal-x")),
        "--normal-y" => Some((&mut cfg.normal_y, "--normal-y")),
        _ => None,
    };

    if let Some((slot, flag)) = path {
        *slot = Some(PathBuf::from(next_value(flag, it)?));
        return Ok(true);
    }

    Ok(false)
}

fn handle_build_misc_flags<I>(cfg: &mut BuildConfig, arg: &str, it: &mut I) -> Result<bool, String>
where
    I: Iterator<Item = String>,
{
    match arg {
        "--compression" => {
            cfg.compression = Compression::from_str(&next_value("--compression", it)?)?;
            Ok(true)
        }
        "--derive-edge" => {
            cfg.derive_edge = parse_bool(&next_value("--derive-edge", it)?)?;
            Ok(true)
        }
        "-h" | "--help" => {
            print_help();
            std::process::exit(0);
        }
        _ => Ok(false),
    }
}

pub(crate) struct ExplicitChannelPaths {
    pub(crate) albedo: Option<PathBuf>,
    pub(crate) light: Option<PathBuf>,
    pub(crate) ao: Option<PathBuf>,
    pub(crate) depth: Option<PathBuf>,
    pub(crate) edge: Option<PathBuf>,
    pub(crate) mask: Option<PathBuf>,
    pub(crate) stroke: Option<PathBuf>,
    pub(crate) normal_x: Option<PathBuf>,
    pub(crate) normal_y: Option<PathBuf>,
}

impl ExplicitChannelPaths {
    pub(crate) fn lookup(&self, name: &str) -> Option<PathBuf> {
        match name {
            "albedo" => self.albedo.clone(),
            "light" => self.light.clone(),
            "ao" => self.ao.clone(),
            "depth" => self.depth.clone(),
            "edge" => self.edge.clone(),
            "mask" => self.mask.clone(),
            "stroke" => self.stroke.clone(),
            "normal_x" => self.normal_x.clone(),
            "normal_y" => self.normal_y.clone(),
            _ => None,
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

pub(crate) fn parse_bool(raw: &str) -> Result<bool, String> {
    match raw {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(format!("invalid bool '{raw}', expected true|false")),
    }
}

pub(crate) fn print_help() {
    println!(
        "scene_maker\n\n
actions:\n  build   Pack pre-baked map images into a strip-major .scenebundle\n  inspect Inspect bundle metadata/compression summary\n\n
action: build\n  --input DIR            Input directory (default: tools/scene_maker/input)\n  --out FILE             Output bundle path (default: tools/scene_maker/out/scene.scenebundle)\n  --metadata FILE        Output metadata json path\n  --width N              Target width (default: 600)\n  --height N             Target height (default: 600)\n  --strip-height N       Strip height in rows (default: 32)\n  --compression MODE     none|rle (default: rle)\n  --derive-edge BOOL     true|false (default: true)\n  --albedo FILE          Override albedo map path\n  --light FILE           Override light map path\n  --ao FILE              Override ao map path\n  --depth FILE           Override depth map path\n  --edge FILE            Override edge map path\n  --mask FILE            Override mask map path\n  --stroke FILE          Override stroke map path\n  --normal-x FILE        Override normal_x map path\n  --normal-y FILE        Override normal_y map path\n\n  If overrides are not set, files are discovered in --input using names:\n  albedo/light/ao/depth/edge/mask/stroke/normal_x/normal_y + extension .png\n\n
action: inspect\n  --bundle FILE          Bundle to inspect (default: tools/scene_maker/out/scene.scenebundle)"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_build_args_uses_defaults() {
        let cfg = parse_build_args(Vec::<String>::new()).expect("parse defaults");
        assert_eq!(cfg.width, 600);
        assert_eq!(cfg.height, 600);
        assert_eq!(cfg.strip_height, 32);
        assert!(matches!(cfg.compression, Compression::Rle));
        assert!(cfg.derive_edge);
    }

    #[test]
    fn parse_build_args_out_updates_default_metadata_path() {
        let cfg = parse_build_args(vec![
            "--out".to_owned(),
            "tmp/test_bundle.scenebundle".to_owned(),
        ])
        .expect("parse --out");
        assert_eq!(cfg.out_bundle, PathBuf::from("tmp/test_bundle.scenebundle"));
        assert_eq!(
            cfg.metadata_out,
            PathBuf::from("tmp/test_bundle.scenebundle.json")
        );
    }

    #[test]
    fn parse_build_args_unknown_arg_fails() {
        let err = match parse_build_args(vec!["--nope".to_owned()]) {
            Ok(_) => panic!("unknown arg should fail"),
            Err(err) => err,
        };
        assert!(err.contains("unknown arg for build"));
    }

    #[test]
    fn parse_build_args_missing_value_fails() {
        let err = match parse_build_args(vec!["--width".to_owned()]) {
            Ok(_) => panic!("missing value should fail"),
            Err(err) => err,
        };
        assert!(err.contains("missing value for --width"));
    }
}
