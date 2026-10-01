use embassy_time::Instant;

use super::error::UploadBodyError;
use super::progress::{UploadBodyProgress, UPLOAD_CHUNK_PIPELINE_ENABLED};
use super::stats::elapsed_ms_u32;

pub(super) struct InflightChunk<I> {
    transfer: I,
    len: usize,
    queue_ms: u32,
    copy_ms: u32,
}

pub(super) async fn queue_chunk_for_sd<H: crate::Host>(
    data: &[u8],
    inflight: &mut Option<InflightChunk<H::Inflight>>,
    progress: &mut UploadBodyProgress,
) -> Result<(), UploadBodyError<H::Error>> {
    let ingress_flush_started_at = Instant::now();
    flush_inflight_chunk::<H>(inflight, progress).await?;
    progress.record_ingress_flush_wait(elapsed_ms_u32(ingress_flush_started_at));

    let queue_started_at = Instant::now();
    let transfer = H::chunk_start(data)
        .await
        .map_err(UploadBodyError::Roundtrip)?;
    let queue_ms = elapsed_ms_u32(queue_started_at);
    *inflight = Some(InflightChunk {
        copy_ms: transfer.copy_ms,
        queue_ms,
        len: data.len(),
        transfer: transfer.transfer,
    });
    if !UPLOAD_CHUNK_PIPELINE_ENABLED {
        flush_inflight_chunk::<H>(inflight, progress).await?;
        progress.record_ingress_flush_wait(elapsed_ms_u32(queue_started_at));
    }
    Ok(())
}

pub(super) async fn flush_inflight_chunk<H: crate::Host>(
    inflight: &mut Option<InflightChunk<H::Inflight>>,
    progress: &mut UploadBodyProgress,
) -> Result<(), UploadBodyError<H::Error>> {
    let Some(inflight_chunk) = inflight.take() else {
        return Ok(());
    };
    let InflightChunk {
        transfer,
        len,
        queue_ms,
        copy_ms,
    } = inflight_chunk;
    let chunk_finish = H::chunk_finish(transfer)
        .await
        .map_err(UploadBodyError::Roundtrip)?;
    progress.apply_finished_chunk(len, queue_ms, copy_ms, chunk_finish);
    Ok(())
}

pub(super) fn try_drain_inflight_chunk<H: crate::Host>(
    inflight: &mut Option<InflightChunk<H::Inflight>>,
    progress: &mut UploadBodyProgress,
) -> Result<(), UploadBodyError<H::Error>> {
    let Some(inflight_chunk) = inflight.take() else {
        return Ok(());
    };
    let InflightChunk {
        transfer,
        len,
        queue_ms,
        copy_ms,
    } = inflight_chunk;
    match H::chunk_try_finish(transfer).map_err(UploadBodyError::Roundtrip)? {
        crate::ChunkTryFinish::Pending(transfer) => {
            *inflight = Some(InflightChunk {
                transfer,
                len,
                queue_ms,
                copy_ms,
            });
        }
        crate::ChunkTryFinish::Finished(chunk_finish) => {
            progress.apply_finished_chunk(len, queue_ms, copy_ms, chunk_finish);
        }
    }
    Ok(())
}

pub(super) async fn drain_inflight_on_error<H: crate::Host>(
    inflight: &mut Option<InflightChunk<H::Inflight>>,
) {
    let Some(inflight_chunk) = inflight.take() else {
        return;
    };
    let _ = H::chunk_finish(inflight_chunk.transfer).await;
}

#[cfg(all(test, feature = "asset-upload-http-pipeline"))]
mod tests {
    use super::*;
    use crate::{ChunkFinish, ChunkStarted, ChunkTryFinish, Command, Host};
    use core::future::Future;
    use std::{sync::Mutex, vec::Vec};
    static EVENTS: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    struct Backend;
    impl Host for Backend {
        type Error = ();
        type Inflight = bool;
        fn admission_open() -> bool {
            true
        }
        fn upload_token() -> Option<&'static [u8]> {
            None
        }
        async fn roundtrip(_: Command) -> Result<(), ()> {
            Ok(())
        }
        async fn chunk_start(_: &[u8]) -> Result<ChunkStarted<bool>, ()> {
            EVENTS.lock().unwrap().push("start");
            Ok(ChunkStarted {
                transfer: false,
                copy_ms: 0,
            })
        }
        async fn chunk_finish(_: bool) -> Result<ChunkFinish, ()> {
            EVENTS.lock().unwrap().push("finish");
            Ok(finish())
        }
        fn chunk_try_finish(ready: bool) -> Result<ChunkTryFinish<bool>, ()> {
            EVENTS.lock().unwrap().push("poll");
            Ok(if ready {
                ChunkTryFinish::Finished(finish())
            } else {
                ChunkTryFinish::Pending(true)
            })
        }
        fn error_log(_: ()) -> &'static str {
            "error"
        }
        fn error_status(_: ()) -> &'static [u8] {
            b"500 Internal Server Error"
        }
        fn error_body(_: ()) -> &'static [u8] {
            b"error"
        }
    }
    fn finish() -> ChunkFinish {
        ChunkFinish {
            roundtrip_ms: 1,
            queue_wait_ms: 0,
            handler_ms: 1,
            post_handler_ms: 0,
            publish_to_receive_ms: 0,
        }
    }
    fn ready<F: Future>(future: F) -> F::Output {
        let mut future = core::pin::pin!(future);
        let mut context = core::task::Context::from_waker(core::task::Waker::noop());
        match future.as_mut().poll(&mut context) {
            core::task::Poll::Ready(result) => result,
            core::task::Poll::Pending => panic!("mock unexpectedly blocked"),
        }
    }
    #[test]
    fn pipeline_retains_pending_ownership_and_finishes_before_next_start() {
        EVENTS.lock().unwrap().clear();
        let mut progress = UploadBodyProgress::new();
        let mut inflight = None;
        assert!(ready(queue_chunk_for_sd::<Backend>(
            b"first",
            &mut inflight,
            &mut progress
        ))
        .is_ok());
        assert!(try_drain_inflight_chunk::<Backend>(&mut inflight, &mut progress).is_ok());
        assert!(inflight.is_some());
        assert!(ready(queue_chunk_for_sd::<Backend>(
            b"second",
            &mut inflight,
            &mut progress
        ))
        .is_ok());
        assert_eq!(
            *EVENTS.lock().unwrap(),
            ["start", "poll", "finish", "start"]
        );
        assert!(try_drain_inflight_chunk::<Backend>(&mut inflight, &mut progress).is_ok());
        assert!(try_drain_inflight_chunk::<Backend>(&mut inflight, &mut progress).is_ok());
        assert!(inflight.is_none());
        assert!(ready(queue_chunk_for_sd::<Backend>(
            b"third",
            &mut inflight,
            &mut progress
        ))
        .is_ok());
        ready(drain_inflight_on_error::<Backend>(&mut inflight));
        assert!(inflight.is_none());
        assert_eq!(EVENTS.lock().unwrap().last(), Some(&"finish"));
    }
}
