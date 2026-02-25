use embassy_time::Instant;

#[derive(Clone, Copy)]
pub(crate) struct TimeSyncCommand {
    pub(crate) unix_epoch_utc_seconds: u64,
    pub(crate) tz_offset_minutes: i32,
}

#[derive(Clone, Copy)]
pub(crate) struct TimeSyncState {
    pub(crate) unix_epoch_utc_seconds: u64,
    pub(crate) tz_offset_minutes: i32,
    pub(crate) sync_instant: Instant,
}
