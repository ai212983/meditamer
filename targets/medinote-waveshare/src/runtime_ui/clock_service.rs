//! Home wall-clock refresh scheduling and boot-sync session ownership.

use ::wall_clock::policy;
use embassy_time::{Duration, Instant};
use medinote::config::SAMPLE_INTERVAL_S;
use medinote::presentation::format_time_hm;
use render::lvgl_adapter::UiAccessToken;
use shell::types::SurfaceRole;

use super::surfaces::{MedinoteScreen, RuntimeCoordinator};
use crate::{wall_clock, SharedI2c};

pub(super) struct ClockService {
    clock: rtc::driver::Pcf85063a<SharedI2c>,
    boot_sync_pending: bool,
    session: Option<wall_clock::ClockSession>,
    next_home_refresh: Instant,
}

impl ClockService {
    pub(super) fn new(rtc_i2c: SharedI2c) -> Self {
        Self {
            clock: rtc::driver::Pcf85063a::new(rtc_i2c),
            boot_sync_pending: true,
            session: None,
            next_home_refresh: Instant::now(),
        }
    }

    pub(super) fn jtag_owned(&self) -> bool {
        self.session.is_some()
    }

    pub(super) fn schedule_at(&mut self, instant: Instant) {
        self.next_home_refresh = instant;
    }

    /// Opens and advances this runtime's one boot session on UI ticks. After
    /// it closes, fresh RTC reads occur only while Home is active and due.
    pub(super) async fn poll(
        &mut self,
        ui_token: &UiAccessToken,
        coordinator: &RuntimeCoordinator,
    ) -> bool {
        if let Some(session) = self.session.take() {
            match session.poll(&mut self.clock).await {
                Ok(snapshot) => {
                    if apply_snapshot(ui_token, coordinator, snapshot) {
                        self.schedule_next(snapshot.is_some_and(|snapshot| snapshot.valid));
                        return true;
                    }
                }
                Err(session) => self.session = Some(session),
            }
            return false;
        }

        if core::mem::take(&mut self.boot_sync_pending) {
            let (session, snapshot) = wall_clock::start(&mut self.clock).await;
            self.session = Some(session);
            let changed = apply_snapshot(ui_token, coordinator, snapshot);
            self.schedule_next(snapshot.is_some_and(|snapshot| snapshot.valid));
            return changed;
        }

        if coordinator.shell().active().role != SurfaceRole::Ambient
            || Instant::now() < self.next_home_refresh
        {
            return false;
        }

        let snapshot = match self.clock.read_snapshot().await {
            Ok(snapshot) => Some(snapshot),
            Err(error) => {
                console::println!("RTC_READ error={}", error.label());
                None
            }
        };
        let changed = apply_snapshot(ui_token, coordinator, snapshot);
        self.schedule_next(snapshot.is_some_and(|snapshot| snapshot.valid));
        changed
    }

    fn schedule_next(&mut self, available: bool) {
        let interval = if available {
            Duration::from_secs(SAMPLE_INTERVAL_S)
        } else {
            Duration::from_millis(policy::UNAVAILABLE_RETRY_INTERVAL_MS)
        };
        self.next_home_refresh = Instant::now() + interval;
    }
}

fn apply_snapshot(
    ui_token: &UiAccessToken,
    coordinator: &RuntimeCoordinator,
    snapshot: Option<rtc::driver::WallClockSnapshot>,
) -> bool {
    let Some(MedinoteScreen::Home(home)) = coordinator.active_screen() else {
        return false;
    };
    if let Some(snapshot) = snapshot.filter(|snapshot| snapshot.valid) {
        let mut clock_text = [0u8; 8];
        let len = format_time_hm(&mut clock_text, snapshot.local_epoch_seconds);
        if let Ok(text) = core::ffi::CStr::from_bytes_with_nul(&clock_text[..len]) {
            home.set_clock(ui_token, text);
        }
    } else {
        home.set_clock(ui_token, c"--:--");
    }
    true
}
