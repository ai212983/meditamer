//! Asset upload over HTTP.
//!
//! [`http`] serves the upload protocol; [`sd_bridge`] turns its routes into SD
//! commands. The radio and the network stack belong to [`crate::firmware::net`];
//! this module is one of its consumers.

mod http;
mod sd_bridge;

use embassy_net::Stack;

#[embassy_executor::task]
pub(crate) async fn http_server_task(stack: Stack<'static>) {
    http::run_http_server(stack).await;
}
