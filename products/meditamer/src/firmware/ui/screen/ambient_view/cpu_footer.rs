//! Low-churn diagnostic footer. Sampling is independent of display refreshes,
//! and repainting is throttled well below the sample rate: every repaint
//! re-holds the panel power lease, so an unthrottled footer would keep the
//! PMIC rails cycling every quarter minute on measurement jitter alone.
use core::fmt::Write;
use heapless::String;
use render::lvgl_adapter::{Font, StyleState, TextAlign, UiAccessToken, Widget, WidgetKind};

/// Minimum time between footer repaints. Load snapshots arrive far more
/// often; only the gate (plus an unchanged-text check below) decides paint.
const UPDATE_INTERVAL_MS: u64 = 60_000;
/// Displayed load is rounded down to this step so percent-by-percent jitter
/// cannot change the visible text and force a repaint by itself.
const PERCENT_STEP: u8 = 5;

pub(super) struct Footer {
    label: Widget,
    next_ms: u64,
    text: String<128>,
}

fn quantize(percent: u8) -> u8 {
    percent / PERCENT_STEP * PERCENT_STEP
}

impl Footer {
    pub(super) fn create(screen: &Widget, token: &UiAccessToken) -> Option<Self> {
        let label = screen.child(token, WidgetKind::Label).ok()?;
        label.set_width(token, 600).ok()?;
        label.set_pos(token, 0, 554).ok()?;
        label
            .set_text_align(token, TextAlign::Center, StyleState::Default)
            .ok()?;
        label
            .set_text_font(token, Font::Size24, StyleState::Default)
            .ok()?;
        label
            .set_text_color(token, render::lvgl_adapter::black(), StyleState::Default)
            .ok()?;
        label.set_text(token, c"CPU: measuring...").ok()?;
        label.set_clickable(token, false).ok()?;
        Some(Self {
            label,
            next_ms: 0,
            text: String::new(),
        })
    }

    pub(super) fn update(
        &mut self,
        token: &UiAccessToken,
        now_ms: u64,
        reading: Option<cpu_load::Snapshot>,
    ) {
        // A fresh snapshot must not bypass the cadence gate on its own: load
        // sampling runs far hotter than this footer may repaint.
        if now_ms < self.next_ms {
            return;
        }
        self.next_ms = now_ms + UPDATE_INTERVAL_MS;
        let snapshot = reading.unwrap_or(cpu_load::History::new().snapshot);
        let mut text = String::<128>::new();
        if (now_ms as u32).wrapping_sub(snapshot.at_ms) > 15_000 {
            let _ = write!(text, "CPU: sample unavailable");
        } else {
            match snapshot.cores {
                [Some(first), Some(second)] => {
                    let _ = write!(
                        text,
                        "CPU 0: {}%   1: {}%   Peak: {}/{}%",
                        quantize(first.percent()),
                        quantize(second.percent()),
                        quantize(snapshot.peak_percent[0]),
                        quantize(snapshot.peak_percent[1])
                    );
                }
                _ => {
                    let _ = write!(text, "CPU: measuring...");
                }
            }
        }
        if self.text == text {
            return;
        }
        let mut terminated = text.clone();
        if terminated.push('\0').is_err() {
            return;
        }
        if let Ok(value) = core::ffi::CStr::from_bytes_with_nul(terminated.as_bytes()) {
            if self.label.set_text(token, value).is_ok() {
                self.text = text;
            }
        }
    }
}
