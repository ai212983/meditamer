use super::{parse_render_args, DitherMode, OutputMode};

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
    let cfg =
        parse_render_args(vec!["--preset".to_owned(), "sumi-e".to_owned()]).expect("parse preset");
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
