//! Medinote's navigable screens, one owned root each, built through the
//! checked LVGL adapter -- matching Meditamer's `firmware::ui::screen` split.

pub mod counter;
pub mod home;
mod home_caption_font;
mod home_heading_font;
pub mod hourglass;
pub mod launcher;
#[cfg(all(feature = "lvgl", feature = "network-controls"))]
pub mod network;
pub mod power;
