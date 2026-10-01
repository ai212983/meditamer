//! Request intake for the SD task when the upload path is not built in.

mod dispatch;
mod handlers;
mod nonwifi;

pub(super) use dispatch::{receive_core_request, CoreIntake};
