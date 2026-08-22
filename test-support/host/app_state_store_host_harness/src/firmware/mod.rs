pub mod app_state;
pub mod config;
pub mod flash;

pub(crate) mod scheduling {
    pub(crate) fn apply_snapshot<T>(_snapshot: T) {}
}
