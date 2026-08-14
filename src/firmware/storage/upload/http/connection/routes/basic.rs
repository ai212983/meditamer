use embassy_net::tcp::TcpSocket;
use esp_println::println;

use super::params::{drain_body, parse_path_or_400, sd_upload_or_http_error, write_response};
use super::RequestContext;
use crate::firmware::observability;
use crate::firmware::types::SdUploadCommand;

pub(super) async fn handle_health(
    socket: &mut TcpSocket<'_>,
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    // Contract: /health stays lightweight and independent from SD upload paths.
    // It must remain safe to probe under pressure and must always bump telemetry
    // so host workflows can correlate reachability checks with runtime behavior.
    drain_body(socket, request).await?;
    observability::record_upload_http_health_request();
    if observability::log_filter_enabled(observability::LOG_DOMAIN_HTTP) {
        println!("upload_http: health ok");
    }
    write_response(socket, b"200 OK", b"ok").await;
    Ok(())
}

pub(super) async fn handle_mkdir(
    socket: &mut TcpSocket<'_>,
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    drain_body(socket, request).await?;
    let (path, path_len) = parse_path_or_400(socket, request.target, "/mkdir").await?;
    let path_str = core::str::from_utf8(&path[..path_len as usize]).unwrap_or("<invalid>");
    if observability::log_filter_enabled(observability::LOG_DOMAIN_SD) {
        println!("upload_http: mkdir begin path={}", path_str);
    }
    sd_upload_or_http_error(socket, SdUploadCommand::Mkdir { path, path_len }).await?;
    if observability::log_filter_enabled(observability::LOG_DOMAIN_SD) {
        println!("upload_http: mkdir done path={}", path_str);
    }
    write_response(socket, b"200 OK", b"mkdir ok").await;
    Ok(())
}

pub(super) async fn handle_stat(
    socket: &mut TcpSocket<'_>,
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    drain_body(socket, request).await?;
    let (path, path_len) = parse_path_or_400(socket, request.target, "/stat").await?;
    sd_upload_or_http_error(socket, SdUploadCommand::Stat { path, path_len }).await?;
    write_response(socket, b"200 OK", b"stat ok").await;
    Ok(())
}

pub(super) async fn handle_delete(
    socket: &mut TcpSocket<'_>,
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    drain_body(socket, request).await?;
    let (path, path_len) = parse_path_or_400(socket, request.target, "/rm").await?;
    sd_upload_or_http_error(socket, SdUploadCommand::Remove { path, path_len }).await?;
    write_response(socket, b"200 OK", b"delete ok").await;
    Ok(())
}
