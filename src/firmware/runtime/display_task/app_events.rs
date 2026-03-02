use core::sync::atomic::Ordering;

use embassy_time::{Duration, Instant};

use super::super::super::{
    app_state::{AppStateCommand, BaseMode, OverlayMode},
    config::{APP_STATE_APPLY_ACKS, FULL_REFRESH_EVERY_N_UPDATES},
    render::{
        next_visual_seed, render_active_mode, render_clock_overlay, render_shanshui_update,
        render_suminagashi_update, render_visual_update, sample_battery_percent,
        RenderActiveParams, RenderVisualParams,
    },
    touch::{
        config::{TOUCH_IRQ_BURST_MS, TOUCH_IRQ_LOW, TOUCH_SAMPLE_IDLE_FALLBACK_MS},
        tasks::request_touch_pipeline_reset,
        wizard::{render_touch_wizard_waiting_screen, TouchCalibrationWizard},
    },
    types::{AppEvent, AppStateApplyAck, DisplayContext, TimeSyncState},
};

use super::state::DisplayLoopState;

include!("app_events/status_mapping.rs");
include!("app_events/dispatch.rs");
include!("app_events/render_helpers.rs");
include!("app_events/lifecycle.rs");
include!("app_events/touch_wizard.rs");
include!("app_events/repaint.rs");
include!("app_events/apply_state.rs");
