//! Small vocabulary [`super::wifi`] and [`super::listener`] share with their
//! callers.

#[derive(Clone, Copy)]
pub enum WifiScanPhase {
    Active,
    Passive,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NetPipelineGate {
    WifiDown,
    LinkDown,
    NoIpv4,
}
