//! Ownership of a power screen whose LVGL deletion must be retried.

use medinote::ui::screen::power::PowerScreen;
use render::lvgl_adapter::UiAccessToken;

pub(super) struct PowerScreenCleanup {
    blocked: Option<PowerScreen>,
}

impl PowerScreenCleanup {
    pub(super) const fn new() -> Self {
        Self { blocked: None }
    }

    pub(super) fn retry(&mut self, ui_token: &UiAccessToken) {
        super::power::retry_blocked_cleanup(ui_token, &mut self.blocked);
    }

    /// Rejects a new sleep attempt while this owner still retains a live root.
    pub(super) fn admit_sleep(
        &self,
        requested: bool,
        fixture: Option<&crate::sleep_fixture::PendingSleep>,
    ) -> bool {
        if !requested || self.blocked.is_none() {
            return true;
        }
        console::println!("SLEEP_ABORTED reason=power_screen_cleanup_blocked");
        if let Some(fixture) = fixture {
            (*fixture).end("Rejected");
        }
        false
    }

    pub(super) fn retain(&mut self, screen: Option<PowerScreen>) {
        self.blocked = screen;
    }
}
