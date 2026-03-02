use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum SdcardSuite {
    All,
    Baseline,
    Burst,
    Failures,
}

pub(super) fn suite_name(suite: &SdcardSuite) -> &'static str {
    match suite {
        SdcardSuite::All => "all",
        SdcardSuite::Baseline => "baseline",
        SdcardSuite::Burst => "burst",
        SdcardSuite::Failures => "failures",
    }
}

#[derive(Clone, Debug)]
pub struct SdcardHwOptions {
    pub build_mode: String,
    pub output_path: Option<PathBuf>,
    pub suite: SdcardSuite,
}
