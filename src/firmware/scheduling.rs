use core::{
    ptr,
    sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, Ordering},
};

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
            Self::Upload
        } else {
            Self::Interactive
        }
    }

    const fn priority(self, class: TaskClass) -> u8 {
        match (self, class) {
            (Self::Interactive | Self::Upload, TaskClass::Serial) => 3,
            (Self::Interactive | Self::Upload, TaskClass::TouchAcquisition) => 3,
            (Self::Interactive | Self::Upload, TaskClass::TouchPipeline) => 2,
            (Self::Interactive, TaskClass::Sd) => 1,
            // Display is the sole APP_EVENTS consumer. Keep it level with the
            // supervised network epoch so an always-ready runner cannot starve
            // state-command acknowledgements after HTTP accept is armed.
            (
                Self::Upload,
                TaskClass::Network
                | TaskClass::Http
                | TaskClass::Sd
                | TaskClass::Wifi
                | TaskClass::Display,
            ) => 1,
            (Self::Diagnostics, TaskClass::Serial) => 3,
            (Self::Diagnostics, TaskClass::Diagnostics | TaskClass::FirmwareHealth) => 2,
            (
                Self::Diagnostics,
                TaskClass::TouchAcquisition
                | TaskClass::TouchPipeline
                | TaskClass::Wifi
                | TaskClass::Sd
                | TaskClass::ImuAcquisition,
            ) => 1,
            (
                Self::Diagnostics,
                TaskClass::Network | TaskClass::Http | TaskClass::Display | TaskClass::ImuPipeline,
            ) => 1,
            _ => 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub(crate) enum TaskClass {
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
}

impl TaskClass {
    const COUNT: usize = 13;
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
static TASK_METADATA: [AtomicPtr<Metadata>; TaskClass::COUNT] =
    [const { AtomicPtr::new(ptr::null_mut()) }; TaskClass::COUNT];

pub(crate) fn configure<S>(class: TaskClass, token: &SpawnToken<S>) {
    let metadata = token.metadata();
    TASK_METADATA[class as usize].store(
        metadata as *const Metadata as *mut Metadata,
        Ordering::Release,
    );
    metadata.set_priority(selected_profile().priority(class));
}

pub(crate) fn spawn<S>(spawner: Spawner, class: TaskClass, token: SpawnToken<S>) {
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
    for (index, slot) in TASK_METADATA.iter().enumerate() {
        let metadata = slot.load(Ordering::Acquire);
        // Task metadata is allocated in Embassy's static task pool. Each pointer
        // is published by that task from a `&'static Metadata` reference.
        if let Some(metadata) = unsafe { metadata.as_ref() } {
            metadata.set_priority(profile.priority(TaskClass::from_index(index)));
        }
    }
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
        assert!(profile.priority(TaskClass::Serial) > profile.priority(TaskClass::Display));
        assert_eq!(profile.priority(TaskClass::Serial), 3);
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

impl TaskClass {
    const fn from_index(index: usize) -> Self {
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
            _ => Self::FirmwareHealth,
        }
    }
}
