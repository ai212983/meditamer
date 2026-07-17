use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum SdcardSuite {
    All,
    Baseline,
    Burst,
    Failures,
    Cutover,
    NoCard,
}

pub(super) fn suite_name(suite: &SdcardSuite) -> &'static str {
    match suite {
        SdcardSuite::All => "all",
        SdcardSuite::Baseline => "baseline",
        SdcardSuite::Burst => "burst",
        SdcardSuite::Failures => "failures",
        SdcardSuite::Cutover => "cutover",
        SdcardSuite::NoCard => "no-card",
    }
}

#[derive(Clone, Debug)]
pub struct SdcardHwOptions {
    pub build_mode: String,
    pub output_path: Option<PathBuf>,
    pub suite: SdcardSuite,
}
