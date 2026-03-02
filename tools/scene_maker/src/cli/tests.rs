#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{parse_build_args, Compression};

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
