#![allow(dead_code)]

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AllocatorState {
    Disabled,
    NotInitialized,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AllocatorStatus {
    pub(crate) feature_enabled: bool,
    pub(crate) state: AllocatorState,
}

pub(crate) fn init_allocator() -> AllocatorStatus {
    allocator_status()
}

pub(crate) fn allocator_status() -> AllocatorStatus {
    if cfg!(feature = "psram-alloc") {
        AllocatorStatus {
            feature_enabled: true,
            state: AllocatorState::NotInitialized,
        }
    } else {
        AllocatorStatus {
            feature_enabled: false,
            state: AllocatorState::Disabled,
        }
    }
}

pub(crate) fn log_allocator_status() {
    let status = allocator_status();
    esp_println::println!(
        "psram: feature_enabled={} state={:?}",
        status.feature_enabled,
        status.state
    );
}
