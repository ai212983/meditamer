#![forbid(unsafe_code)]

//! Temperature/humidity footer for Ambient Home.

use core::fmt::Write as _;

use heapless::String;
use render::lvgl_adapter::{Font, StyleState, TextAlign, UiAccessToken, Widget, WidgetKind};

use super::{environment_font, model};
use crate::firmware::config::{
    INKPLATE_HUMIDITY_SCALE_PERMILLE, INKPLATE_TEMPERATURE_OFFSET_CENTIDEGREES,
};
use crate::firmware::environment::EnvironmentSnapshot;

pub(super) struct Footer {
    label: Widget,
    internal_label: Widget,
    display: Option<model::EnvironmentDisplay>,
    internal_display: Option<model::EnvironmentDisplay>,
}

impl Footer {
    pub(super) fn create(screen: &Widget, token: &UiAccessToken) -> Option<Self> {
        let label = screen.child(token, WidgetKind::Label).ok()?;
        let built = label.set_width(token, 600).is_ok()
            && label.set_pos(token, 0, 426).is_ok()
            && label
                .set_text_align(token, TextAlign::Center, StyleState::Default)
                .is_ok()
            && label
                .set_text_font(
                    token,
                    render::lvgl_adapter::Font::custom(environment_font::font()),
                    StyleState::Default,
                )
                .is_ok()
            && label
                .set_text_color(token, render::lvgl_adapter::black(), StyleState::Default)
                .is_ok()
            && label.set_text(token, c"--.- C   --% RH").is_ok()
            && label.set_clickable(token, false).is_ok();
        if !built {
            return None;
        }

        let internal_label = screen.child(token, WidgetKind::Label).ok()?;
        let internal_built = internal_label.set_width(token, 600).is_ok()
            && internal_label.set_pos(token, 0, 513).is_ok()
            && internal_label
                .set_text_align(token, TextAlign::Center, StyleState::Default)
                .is_ok()
            && internal_label
                .set_text_font(token, Font::Size32, StyleState::Default)
                .is_ok()
            && internal_label
                .set_text_color(token, render::lvgl_adapter::black(), StyleState::Default)
                .is_ok()
            && internal_label.set_text(token, c"").is_ok()
            && internal_label.set_clickable(token, false).is_ok()
            && internal_label.set_hidden(token, true).is_ok();
        if !internal_built {
            return None;
        }

        Some(Self {
            label,
            internal_label,
            display: None,
            internal_display: None,
        })
    }

    /// Returns `true` only when either rounded, visible representation changed.
    pub(super) fn apply(&mut self, token: &UiAccessToken, reading: EnvironmentSnapshot) -> bool {
        let onboard = model::environment_display(
            reading.onboard.temperature_centidegrees,
            reading.onboard.humidity_millipercent,
            INKPLATE_TEMPERATURE_OFFSET_CENTIDEGREES,
            INKPLATE_HUMIDITY_SCALE_PERMILLE,
        );
        // Combined ambient source (external preferred, corrected onboard
        // fallback), shared with mountain snow coverage.
        let climate = model::ambient_climate(
            reading.onboard.temperature_centidegrees,
            reading.onboard.humidity_millipercent,
            INKPLATE_TEMPERATURE_OFFSET_CENTIDEGREES,
            INKPLATE_HUMIDITY_SCALE_PERMILLE,
            reading.external.map(|reading| {
                (
                    reading.temperature_centidegrees,
                    reading.humidity_millipercent,
                )
            }),
        );
        let external = reading.external.map(|_| {
            model::environment_display(
                climate.temperature_centidegrees,
                climate.humidity_millipercent,
                0,
                1_000,
            )
        });
        let display = external.unwrap_or(onboard);
        let mut changed = false;
        if self.display != Some(display) {
            if !set_reading(&self.label, token, "", display) {
                return false;
            }
            self.display = Some(display);
            changed = true;
        }

        let internal_display = external.map(|_| onboard);
        if self.internal_display != internal_display {
            let updated = match internal_display {
                Some(display) => {
                    set_reading(&self.internal_label, token, "", display)
                        && self.internal_label.set_hidden(token, false).is_ok()
                }
                None => self.internal_label.set_hidden(token, true).is_ok(),
            };
            if !updated {
                return changed;
            }
            self.internal_display = internal_display;
            changed = true;
        }
        changed
    }
}

fn set_reading(
    label: &Widget,
    token: &UiAccessToken,
    prefix: &str,
    display: model::EnvironmentDisplay,
) -> bool {
    let mut text = String::<48>::new();
    let temperature = i32::from(display.temperature_tenths);
    let magnitude = temperature.unsigned_abs();
    let formatted = if temperature < 0 {
        write!(
            text,
            "{}-{}.{:01} C   {}% RH",
            prefix,
            magnitude / 10,
            magnitude % 10,
            display.humidity_percent,
        )
    } else {
        write!(
            text,
            "{}{}.{:01} C   {}% RH",
            prefix,
            magnitude / 10,
            magnitude % 10,
            display.humidity_percent,
        )
    };
    if formatted.is_err() || text.push('\0').is_err() {
        return false;
    }
    let Ok(text) = core::ffi::CStr::from_bytes_with_nul(text.as_bytes()) else {
        return false;
    };
    label.set_text(token, text).is_ok()
}
