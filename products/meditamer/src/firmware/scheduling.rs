use core::{
    cell::Cell,
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};

use critical_section::Mutex;
use embassy_executor::{Metadata, SpawnToken, Spawner};

use crate::firmware::app_state::{AppStateSnapshot, Phase};

const PROFILE_AUTO: u8 = u8::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum SchedulerProfile {
    Interactive,
    Upload,
    Diagnostics,
}

impl SchedulerProfile {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::Upload => "upload",
            Self::Diagnostics => "diagnostics",
        }
    }

    const fn from_raw(raw: u8) -> Self {
        match raw {
            1 => Self::Upload,
            2 => Self::Diagnostics,
            _ => Self::Interactive,
        }
    }

    const fn for_snapshot(snapshot: AppStateSnapshot) -> Self {
        if matches!(snapshot.phase, Phase::DiagnosticsExclusive) {
            Self::Diagnostics
        } else if snapshot.services.upload_enabled {
            // Keep fairness while the service is enabled: even an idle connected
            // network runner can remain ready. This is not a UI restriction.
            Self::Upload
        } else {
            Self::Interactive
        }
    }

    const fn priority(self, class: TaskClass) -> u8 {
        match (self, class) {
            (_, TaskClass::FlashQuiesce | TaskClass::ImuBusOwner) => 4,
            #[cfg(feature = "firmware-trace")]
            (_, TaskClass::TraceDrain) => 1,
            (Self::Interactive | Self::Upload, TaskClass::Serial) => 3,
            (
                Self::Interactive | Self::Upload,
                TaskClass::TouchAcquisition | TaskClass::I2cOwner,
            ) => 3,
            (Self::Interactive | Self::Upload, TaskClass::TouchPipeline) => 2,
            (Self::Interactive, TaskClass::Sd) => 1,
            // Keep the UI and observation providers level with the network
            // epoch: its continuously ready runner otherwise starves admitted
            // sensor requests and control acknowledgements during an upload.
            // IMU acquisition also needs its pipeline to drain blocking sends
            // before it can acknowledge panel bus control while sampling stays live.
            (
                Self::Upload,
                TaskClass::Network
                | TaskClass::Http
                | TaskClass::Sd
                | TaskClass::Wifi
                | TaskClass::Display
                | TaskClass::Battery
                | TaskClass::EnvironmentAcquisition
                | TaskClass::CpuObservation
                | TaskClass::ImuAcquisition
                | TaskClass::ImuPipeline
                | TaskClass::Console,
            ) => 1,
            (Self::Diagnostics, TaskClass::Serial) => 3,
            (Self::Diagnostics, TaskClass::TouchAcquisition | TaskClass::I2cOwner) => 3,
            (Self::Diagnostics, TaskClass::TouchPipeline) => 2,
            (Self::Diagnostics, TaskClass::Diagnostics | TaskClass::FirmwareHealth) => 2,
            (Self::Diagnostics, TaskClass::Wifi | TaskClass::Sd | TaskClass::ImuAcquisition) => 1,
            (
                Self::Diagnostics,
                TaskClass::Network
                | TaskClass::Http
                | TaskClass::Display
                | TaskClass::ImuPipeline
                | TaskClass::Console,
            ) => 1,
            _ => 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum TaskClass {
    TouchAcquisition,
    TouchPipeline,
    ImuAcquisition,
    ImuPipeline,
    Display,
    Diagnostics,
    Sd,
    Serial,
    Wifi,
    Network,
    Http,
    Battery,
    // Distinct from `Diagnostics` so `firmware_health_task` and
    // `self_test::diagnostics_task` no longer collide on one metadata slot
    // (F-003). Same intended priority as `Diagnostics` in every profile —
    // see `SchedulerProfile::priority`.
    FirmwareHealth,
    // Appended so existing task-class indices stay stable. Like Battery, it
    // shares the upload I/O priority to make progress while the network
    // runner remains continuously ready.
    EnvironmentAcquisition,
    CpuObservation,
    I2cOwner,
    Console,
    #[cfg(feature = "firmware-trace")]
    TraceDrain,
    #[cfg(feature = "firmware-trace")]
    TraceProbe,
    FlashQuiesce,
    ImuBusOwner,
}

impl TaskClass {
    const COUNT: usize = 19
        + if cfg!(feature = "firmware-trace") {
            2
        } else {
            0
        };
}

impl TaskClass {
    fn from_index(index: usize) -> Self {
        match index {
            0 => Self::TouchAcquisition,
            1 => Self::TouchPipeline,
            2 => Self::ImuAcquisition,
            3 => Self::ImuPipeline,
            4 => Self::Display,
            5 => Self::Diagnostics,
            6 => Self::Sd,
            7 => Self::Serial,
            8 => Self::Wifi,
            9 => Self::Network,
            10 => Self::Http,
            11 => Self::Battery,
            12 => Self::FirmwareHealth,
            13 => Self::EnvironmentAcquisition,
            14 => Self::CpuObservation,
            15 => Self::I2cOwner,
            16 => Self::Console,
            #[cfg(feature = "firmware-trace")]
            17 => Self::TraceDrain,
            #[cfg(feature = "firmware-trace")]
            18 => Self::TraceProbe,
            #[cfg(feature = "firmware-trace")]
            19 => Self::FlashQuiesce,
            #[cfg(not(feature = "firmware-trace"))]
            17 => Self::FlashQuiesce,
            #[cfg(feature = "firmware-trace")]
            20 => Self::ImuBusOwner,
            #[cfg(not(feature = "firmware-trace"))]
            18 => Self::ImuBusOwner,
            _ => unreachable!("invalid task class index"),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SchedulerStatus {
    pub(crate) automatic: SchedulerProfile,
    pub(crate) selected: SchedulerProfile,
    pub(crate) override_profile: Option<SchedulerProfile>,
}

static AUTOMATIC_PROFILE: AtomicU8 = AtomicU8::new(SchedulerProfile::Interactive as u8);
static OVERRIDE_PROFILE: AtomicU8 = AtomicU8::new(PROFILE_AUTO);
static RUNTIME_READY: AtomicBool = AtomicBool::new(false);
static TASK_METADATA: Mutex<Cell<[Option<&'static Metadata>; TaskClass::COUNT]>> =
    Mutex::new(Cell::new([None; TaskClass::COUNT]));

pub(crate) fn configure<S>(class: TaskClass, token: &SpawnToken<S>) {
    // Embassy's public accessor ties the borrow to `token`, but the metadata
    // belongs to the static task pool (the raw implementation returns a
    // `&'static Metadata`). Keep this lifetime bridge at the API boundary;
    // publication and every later read remain typed and critical-section
    // synchronized.
    let metadata: &'static Metadata = unsafe { core::mem::transmute(token.metadata()) };
    critical_section::with(|cs| {
        let metadata_slots = TASK_METADATA.borrow(cs);
        let mut slots = metadata_slots.get();
        slots[class as usize] = Some(metadata);
        metadata_slots.set(slots);
        metadata.set_priority(selected_profile().priority(class));
    });
}

pub fn spawn<S>(spawner: Spawner, class: TaskClass, token: SpawnToken<S>) {
    configure(class, &token);
    spawner.spawn(token);
}

pub fn spawn_interrupt<S: Send>(
    spawner: embassy_executor::SendSpawner,
    class: TaskClass,
    token: SpawnToken<S>,
) {
    configure(class, &token);
    spawner.spawn(token);
}

pub(crate) fn apply_snapshot(snapshot: AppStateSnapshot) {
    let next = SchedulerProfile::for_snapshot(snapshot) as u8;
    let previous = AUTOMATIC_PROFILE.swap(next, Ordering::Relaxed);
    if previous != next && OVERRIDE_PROFILE.load(Ordering::Relaxed) == PROFILE_AUTO {
        apply_selected_profile();
    }
}

pub(crate) fn set_override(profile: Option<SchedulerProfile>) {
    OVERRIDE_PROFILE.store(
        profile.map_or(PROFILE_AUTO, |value| value as u8),
        Ordering::Relaxed,
    );
    apply_selected_profile();
}

pub(crate) fn status() -> SchedulerStatus {
    let automatic = SchedulerProfile::from_raw(AUTOMATIC_PROFILE.load(Ordering::Relaxed));
    let override_raw = OVERRIDE_PROFILE.load(Ordering::Relaxed);
    let override_profile =
        (override_raw != PROFILE_AUTO).then(|| SchedulerProfile::from_raw(override_raw));
    SchedulerStatus {
        automatic,
        selected: override_profile.unwrap_or(automatic),
        override_profile,
    }
}

pub(crate) fn mark_runtime_ready() {
    RUNTIME_READY.store(true, Ordering::Release);
}

pub(crate) fn runtime_ready() -> bool {
    RUNTIME_READY.load(Ordering::Acquire)
}

fn selected_profile() -> SchedulerProfile {
    status().selected
}

fn apply_selected_profile() {
    let profile = selected_profile();
    critical_section::with(|cs| {
        for (index, metadata) in TASK_METADATA.borrow(cs).get().into_iter().enumerate() {
            if let Some(metadata) = metadata {
                metadata.set_priority(profile.priority(TaskClass::from_index(index)));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{SchedulerProfile, TaskClass};
    use crate::firmware::app_state::{AppStateSnapshot, Phase};

    #[test]
    fn automatic_profile_follows_behavior_with_explicit_precedence() {
        let mut snapshot = AppStateSnapshot::default();
        assert_eq!(
            SchedulerProfile::for_snapshot(snapshot),
            SchedulerProfile::Interactive
        );

        snapshot.services.upload_enabled = true;
        assert_eq!(
            SchedulerProfile::for_snapshot(snapshot),
            SchedulerProfile::Upload
        );

        snapshot.phase = Phase::DiagnosticsExclusive;
        assert_eq!(
            SchedulerProfile::for_snapshot(snapshot),
            SchedulerProfile::Diagnostics
        );
    }

    #[test]
    fn upload_profile_keeps_touch_preemptive_and_balances_io_tasks() {
        let profile = SchedulerProfile::Upload;
        assert!(
            profile.priority(TaskClass::TouchAcquisition)
                > profile.priority(TaskClass::TouchPipeline)
        );
        assert!(profile.priority(TaskClass::TouchPipeline) > profile.priority(TaskClass::Sd));
        assert_eq!(
            profile.priority(TaskClass::Network),
            profile.priority(TaskClass::Http)
        );
        assert_eq!(
            profile.priority(TaskClass::Http),
            profile.priority(TaskClass::Sd)
        );
        assert_eq!(
            profile.priority(TaskClass::Wifi),
            profile.priority(TaskClass::Sd)
        );
        assert_eq!(
            profile.priority(TaskClass::Display),
            profile.priority(TaskClass::Network)
        );
        for provider in [
            TaskClass::Battery,
            TaskClass::EnvironmentAcquisition,
            TaskClass::CpuObservation,
            TaskClass::ImuPipeline,
        ] {
            assert_eq!(
                profile.priority(provider),
                profile.priority(TaskClass::Network)
            );
            assert!(profile.priority(provider) < profile.priority(TaskClass::TouchPipeline));
        }
        assert!(profile.priority(TaskClass::Serial) > profile.priority(TaskClass::Display));
        assert_eq!(profile.priority(TaskClass::Serial), 3);
    }

    #[test]
    fn diagnostics_keeps_touch_acquisition_ahead_of_observation_work() {
        let profile = SchedulerProfile::Diagnostics;
        assert_eq!(profile.priority(TaskClass::TouchAcquisition), 3);
        assert_eq!(profile.priority(TaskClass::I2cOwner), 3);
        assert_eq!(profile.priority(TaskClass::TouchPipeline), 2);
        assert!(
            profile.priority(TaskClass::TouchAcquisition)
                > profile.priority(TaskClass::Diagnostics)
        );
        assert!(
            profile.priority(TaskClass::TouchPipeline)
                > profile.priority(TaskClass::ImuAcquisition)
        );
    }

    #[test]
    fn firmware_health_has_a_distinct_class_index_from_diagnostics() {
        // F-003: `self_test::diagnostics_task` and `firmware_health_task` must no longer
        // collide on one `TASK_METADATA` slot.
        assert_ne!(
            TaskClass::Diagnostics as usize,
            TaskClass::FirmwareHealth as usize
        );
        assert!((TaskClass::FirmwareHealth as usize) < TaskClass::COUNT);
    }

    #[test]
    fn writer_can_release_a_record_while_network_stays_ready_in_every_profile() {
        for profile in [
            SchedulerProfile::Interactive,
            SchedulerProfile::Upload,
            SchedulerProfile::Diagnostics,
        ] {
            // RX may await the writer's current record, so continuously ready
            // network service must not prevent that record from finishing.
            assert!(profile.priority(TaskClass::Console) >= profile.priority(TaskClass::Network));
            assert!(profile.priority(TaskClass::Console) < profile.priority(TaskClass::Serial));
        }
    }

    #[test]
    fn firmware_health_matches_diagnostics_priority_in_every_profile() {
        // The correction changes registration identity only — intended priority must stay
        // identical to `Diagnostics` in every scheduler profile.
        for profile in [
            SchedulerProfile::Interactive,
            SchedulerProfile::Upload,
            SchedulerProfile::Diagnostics,
        ] {
            assert_eq!(
                profile.priority(TaskClass::Diagnostics),
                profile.priority(TaskClass::FirmwareHealth),
                "profile {profile:?} disagrees on Diagnostics vs FirmwareHealth priority"
            );
        }
    }

    #[test]
    fn flash_quiesce_can_preempt_every_profile() {
        for profile in [
            SchedulerProfile::Interactive,
            SchedulerProfile::Upload,
            SchedulerProfile::Diagnostics,
        ] {
            assert!(
                profile.priority(TaskClass::FlashQuiesce) > profile.priority(TaskClass::Serial),
                "profile {profile:?} must let CPU1 acknowledge a flash write"
            );
        }
    }

    #[test]
    fn task_class_from_index_round_trips_every_slot() {
        // Every `TASK_METADATA` slot index must map back to a distinct `TaskClass`, including
        // the new `FirmwareHealth` slot added at the end by F-003.
        let mut seen = [false; TaskClass::COUNT];
        for index in 0..TaskClass::COUNT {
            let class = TaskClass::from_index(index);
            let class_index = class as usize;
            assert_eq!(
                class_index, index,
                "from_index({index}) round-tripped to {class_index}"
            );
            assert!(
                !seen[class_index],
                "index {index} produced a class already seen"
            );
            seen[class_index] = true;
        }
        assert!(
            seen.iter().all(|&s| s),
            "not every TASK_METADATA slot has a distinct class"
        );
    }
}
