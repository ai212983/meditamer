use embassy_net::tcp::TcpSocket;

use super::params::{drain_body, parse_path_or_400, sd_upload_or_http_error};
use super::RequestContext;

pub(super) async fn handle_health<H: crate::Host>(
    socket: &mut TcpSocket<'_>,
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    // Contract: /health stays lightweight and independent from SD upload paths.
    // It must remain safe to probe under pressure and must always bump telemetry
    // so host workflows can correlate reachability checks with runtime behavior.
    drain_body(socket, request).await?;
    H::record_upload_http_health_request();
    if H::log_filter_enabled(crate::LogDomain::Http) {
        H::log(format_args!("upload_http: health ok"));
    }
    super::write_success_response(socket, super::RequestRouteKind::Health).await;
    Ok(())
}

pub(super) async fn handle_mkdir<H: crate::Host>(
    socket: &mut TcpSocket<'_>,
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    drain_body(socket, request).await?;
    let (path, path_len) = parse_path_or_400(socket, request.target, "/mkdir").await?;
    let path_str = core::str::from_utf8(&path[..path_len as usize]).unwrap_or("<invalid>");
    if H::log_filter_enabled(crate::LogDomain::Sd) {
        H::log(format_args!("upload_http: mkdir begin path={}", path_str));
    }
    sd_upload_or_http_error::<H>(socket, crate::Command::Mkdir { path, path_len }).await?;
    if H::log_filter_enabled(crate::LogDomain::Sd) {
        H::log(format_args!("upload_http: mkdir done path={}", path_str));
    }
    super::write_success_response(socket, super::RequestRouteKind::Mkdir).await;
    Ok(())
}

pub(super) async fn handle_stat<H: crate::Host>(
    socket: &mut TcpSocket<'_>,
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    drain_body(socket, request).await?;
    let (path, path_len) = parse_path_or_400(socket, request.target, "/stat").await?;
    sd_upload_or_http_error::<H>(socket, crate::Command::Stat { path, path_len }).await?;
    super::write_success_response(socket, super::RequestRouteKind::Stat).await;
    Ok(())
}

pub(super) async fn handle_delete<H: crate::Host>(
    socket: &mut TcpSocket<'_>,
    request: &RequestContext<'_>,
) -> Result<(), &'static str> {
    drain_body(socket, request).await?;
    let (path, path_len) = parse_path_or_400(socket, request.target, "/rm").await?;
    sd_upload_or_http_error::<H>(socket, crate::Command::Remove { path, path_len }).await?;
    super::write_success_response(socket, super::RequestRouteKind::Remove).await;
    Ok(())
}
