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
