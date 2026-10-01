//! A cancellation-safe wait: only the selected app event is consumed. Other
//! queues retain their payloads for the display's ordered service pass.
use core::{
    future::{poll_fn, Future},
    task::Poll,
};
use embassy_futures::select::{select4, Either4};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, signal::Signal,
};

pub(super) async fn wait<
    A,
    S,
    T,
    U,
    const NA: usize,
    const NS: usize,
    const NT: usize,
    const NU: usize,
>(
    app: &Channel<CriticalSectionRawMutex, A, NA>,
    sd: &Channel<CriticalSectionRawMutex, S, NS>,
    touch: &Channel<CriticalSectionRawMutex, T, NT>,
    multi: &Channel<CriticalSectionRawMutex, U, NU>,
    include_touch: bool,
    wake: &Signal<CriticalSectionRawMutex, ()>,
    timer: impl Future<Output = ()>,
) -> Option<A> {
    let queued = poll_fn(|cx| {
        let sd_ready = sd.poll_ready_to_receive(cx).is_ready();
        let touch_ready = include_touch && touch.poll_ready_to_receive(cx).is_ready();
        let multi_ready = include_touch && multi.poll_ready_to_receive(cx).is_ready();
        if sd_ready || touch_ready || multi_ready {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    });
    match select4(app.receive(), queued, wake.wait(), timer).await {
        Either4::First(event) => Some(event),
        _ => None,
    }
}
