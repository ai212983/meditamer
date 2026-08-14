use crate::firmware::app_state::{AppStateCommand, DiagKind, DiagTargets};
use crate::firmware::scheduling::SchedulerProfile;
#[cfg(feature = "asset-upload-http")]
use crate::firmware::types::NetConfigSet;
use crate::firmware::types::{SD_PATH_MAX, SD_WRITE_MAX};

#[derive(Clone, Copy)]
pub(in crate::firmware::serial) enum SerialCommand {
    Ping,
    FirmwareStatus,
    FirmwarePrepare,
    FirmwareBegin {
        image_len: u32,
        digest: [u8; 32],
        signature: [u8; 64],
    },
    FirmwareChunk {
        offset: u32,
        bytes: [u8; crate::firmware::update::LEGACY_CHUNK_MAX],
        len: u16,
    },
    FirmwareStream {
        baud: u32,
    },
    FirmwareFinish,
    FirmwareActivate,
    FirmwareAbort,
    UiCycleStep,
    #[cfg(feature = "ui-provider-fixture")]
    UiProviderFixtureStep,
    Repaint,
    Metrics,
    TouchSchedReset,
    Scheduler {
        operation: SchedulerOperation,
    },
    MetricsNet,
    TelemetryStatus,
    TelemetrySet {
        operation: TelemetrySetOperation,
    },
    StackStatus,
    AllocatorStatus,
    AllocatorAllocProbe {
        bytes: u32,
    },
    #[cfg(feature = "ble-foundation")]
    BleProbeStart,
    #[cfg(feature = "ble-foundation")]
    BleProbeStatus,
    #[cfg(feature = "ble-foundation")]
    BlePhase1sStart {
        boot_generation: u32,
        epoch: u32,
    },
    #[cfg(feature = "ble-foundation")]
    BlePhase1sStatus,
    #[cfg(feature = "ble-foundation")]
    RadioHandoffAcquire {
        boot_generation: u32,
        epoch: u32,
    },
    #[cfg(feature = "ble-foundation")]
    RadioHandoffRelease {
        boot_generation: u32,
        epoch: u32,
    },
    #[cfg(feature = "ble-foundation")]
    RadioHandoffStatus,
    Probe,
    RwVerify {
        lba: u32,
    },
    FatList {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    FatRead {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    FatWrite {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
        data: [u8; SD_WRITE_MAX],
        data_len: u16,
    },
    FatStat {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    FatMkdir {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    FatRemove {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
    },
    FatRename {
        src_path: [u8; SD_PATH_MAX],
        src_path_len: u8,
        dst_path: [u8; SD_PATH_MAX],
        dst_path_len: u8,
    },
    FatAppend {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
        data: [u8; SD_WRITE_MAX],
        data_len: u16,
    },
    FatTruncate {
        path: [u8; SD_PATH_MAX],
        path_len: u8,
        size: u32,
    },
    SdWait {
        target: SdWaitTarget,
        timeout_ms: u32,
    },
    DiagGet,
    StateGet,
    StateSet {
        operation: StateSetOperation,
    },
    StateDiag {
        kind: DiagKind,
        targets: DiagTargets,
    },
    #[cfg(feature = "asset-upload-http")]
    NetCfgSet {
        config: NetConfigSet,
    },
    #[cfg(feature = "asset-upload-http")]
    NetCfgGet,
    #[cfg(feature = "asset-upload-http")]
    NetStart,
    #[cfg(feature = "asset-upload-http")]
    NetStop,
    #[cfg(feature = "asset-upload-http")]
    NetStatus,
    #[cfg(feature = "asset-upload-http")]
    NetRecover,
    #[cfg(feature = "asset-upload-http")]
    NetListenerSet {
        enabled: bool,
    },
}

#[derive(Clone, Copy)]
pub(in crate::firmware::serial) enum SchedulerOperation {
    Status,
    Automatic,
    Profile(SchedulerProfile),
}

#[derive(Clone, Copy)]
pub(in crate::firmware::serial) enum StateSetOperation {
    Upload(bool),
}

#[derive(Clone, Copy)]
pub(in crate::firmware::serial) enum TelemetryDomain {
    Wifi,
    Reassoc,
    Net,
    Http,
    Sd,
}

#[derive(Clone, Copy)]
pub(in crate::firmware::serial) enum TelemetrySetOperation {
    Domain {
        domain: TelemetryDomain,
        enabled: bool,
    },
    All {
        enabled: bool,
    },
    Default,
}

impl StateSetOperation {
    pub(in crate::firmware::serial) fn as_state_command(self) -> AppStateCommand {
        match self {
            Self::Upload(enabled) => AppStateCommand::SetUpload(enabled),
        }
    }
}

#[derive(Clone, Copy)]
pub(in crate::firmware::serial) enum SdWaitTarget {
    Next,
    Last,
    Id(u32),
}
