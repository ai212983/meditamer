//! Request intake for the SD task, split by whether the Wi-Fi/upload path is built in.
//!
//! [`dispatch`] picks the source set to await; [`wifi`] and [`nonwifi`] are the
//! two builds of that wait; [`handlers`] is what they both do with a request.

mod dispatch;
mod handlers;
mod nonwifi;
mod wifi;

pub(super) use dispatch::receive_core_request;
