//! Bounded diagnostic output, independently scheduled from command reception.
use super::{command_dispatch, io::write_tap_trace_sample, status::format_status};
#[cfg(not(feature = "wifi-debug-slim-app"))]
use crate::firmware::touch::{
    config::{TOUCH_TRACE_ENABLED, TOUCH_TRACE_SAMPLES},
    debug_log::write_touch_trace_sample,
};
use crate::firmware::{
    config::{SD_SERIAL_LINES, SERIAL_STATUS_EVENTS, TAP_TRACE_ENABLED, TAP_TRACE_SAMPLES},
    touch::{
        config::{TOUCH_EVENT_TRACE_ENABLED, TOUCH_EVENT_TRACE_SAMPLES},
        debug_log::{uart_write_all, write_touch_event_trace_sample},
    },
    types::SerialWriter,
};
use core::{
    future::poll_fn,
    sync::atomic::{AtomicBool, Ordering},
    task::Poll,
};
use embassy_futures::{
    select::{select, Either},
    yield_now,
};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};

#[derive(Clone, Copy)]
pub(super) enum MetricsKind {
    General,
    Network,
}
static METRICS: Signal<CriticalSectionRawMutex, MetricsKind> = Signal::new();
static METRICS_BUSY: AtomicBool = AtomicBool::new(false);
const TAP_TRACE_DRAIN_BUDGET: usize = 1;

/// One admitted metrics operation, including its entire output lifetime.
/// A second command gets BUSY instead of overwriting or growing a work queue.
pub(super) fn request_metrics(kind: MetricsKind) -> bool {
    if METRICS_BUSY
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return false;
    }
    METRICS.signal(kind);
    true
}

pub(super) async fn run() {
    let mut uart = SerialWriter;
    loop {
        drain_samples(&mut uart).await;
        // Stack-overflow hunt: track the serial/tap loop's low-water against
        // the main-task guard (this loop runs on the main executor).
        crate::firmware::observability::record_stack_headroom();
        match select(METRICS.wait(), poll_fn(poll_work)).await {
            Either::First(kind) => {
                metrics(&mut uart, kind).await;
                METRICS_BUSY.store(false, Ordering::Release);
            }
            Either::Second(()) => {}
        }
        yield_now().await;
    }
}

async fn metrics(uart: &mut SerialWriter, kind: MetricsKind) {
    let allocation = crate::firmware::psram::InternalValue::try_new_bounded::<1_200, _>(|| async {
        match kind {
            MetricsKind::General => command_dispatch::metrics_command(uart).await,
            MetricsKind::Network => command_dispatch::metrics_net_command(uart).await,
        }
    });
    match allocation {
        Ok(mut command) => command.pin_mut().await,
        Err(_) => {
            let line: &[u8] = match kind {
                MetricsKind::General => b"METRICS ERR reason=internal_dispatch_alloc\r\n",
                MetricsKind::Network => b"METRICSNET ERR reason=internal_dispatch_alloc\r\n",
            };
            console::write_response(line).await;
        }
    }
}

fn poll_work(cx: &mut core::task::Context<'_>) -> Poll<()> {
    let mut ready = SERIAL_STATUS_EVENTS.poll_ready_to_receive(cx).is_ready();
    ready |= SD_SERIAL_LINES.poll_ready_to_receive(cx).is_ready();
    if TAP_TRACE_ENABLED {
        ready |= TAP_TRACE_SAMPLES.poll_ready_to_receive(cx).is_ready();
    }
    if TOUCH_EVENT_TRACE_ENABLED {
        ready |= TOUCH_EVENT_TRACE_SAMPLES
            .poll_ready_to_receive(cx)
            .is_ready();
    }
    #[cfg(not(feature = "wifi-debug-slim-app"))]
    if TOUCH_TRACE_ENABLED {
        ready |= TOUCH_TRACE_SAMPLES.poll_ready_to_receive(cx).is_ready();
    }
    if ready {
        Poll::Ready(())
    } else {
        Poll::Pending
    }
}

async fn drain_samples(uart: &mut SerialWriter) {
    let quiet = crate::firmware::update::transport_quiet();
    if let Ok(event) = SERIAL_STATUS_EVENTS.try_receive() {
        if !quiet {
            let line = format_status(event);
            let _ = uart_write_all(uart, line.as_bytes()).await;
        }
    }

    if TOUCH_EVENT_TRACE_ENABLED {
        if let Ok(event) = TOUCH_EVENT_TRACE_SAMPLES.try_receive() {
            if !quiet {
                write_touch_event_trace_sample(uart, event).await;
            }
        }
    }

    #[cfg(not(feature = "wifi-debug-slim-app"))]
    if TOUCH_TRACE_ENABLED {
        if let Ok(sample) = TOUCH_TRACE_SAMPLES.try_receive() {
            if !quiet {
                write_touch_trace_sample(uart, sample).await;
            }
        }
    }

    if TAP_TRACE_ENABLED {
        for _ in 0..TAP_TRACE_DRAIN_BUDGET {
            let Ok(sample) = TAP_TRACE_SAMPLES.try_receive() else {
                break;
            };
            if !quiet {
                write_tap_trace_sample(uart, sample).await;
            }
        }
    }

    if let Ok(line) = SD_SERIAL_LINES.try_receive() {
        if !quiet {
            let _ = uart_write_all(uart, line.as_bytes()).await;
            yield_now().await;
        }
    }
}
