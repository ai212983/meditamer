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
            (Self::Interactive | Self::Upload, TaskClass::TouchAcquisition) => 3,
            (Self::Interactive | Self::Upload, TaskClass::TouchPipeline) => 2,
            (Self::Interactive, TaskClass::Sd) => 1,
            (
                Self::Upload,
                TaskClass::Network | TaskClass::Http | TaskClass::Sd | TaskClass::Wifi,
            ) => 1,
            (Self::Diagnostics, TaskClass::Serial) => 3,
            (Self::Diagnostics, TaskClass::Diagnostics) => 2,
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
}

impl TaskClass {
    const COUNT: usize = 12;
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
        assert!(profile.priority(TaskClass::Sd) > profile.priority(TaskClass::Serial));
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
            _ => Self::Battery,
        }
    }
}
