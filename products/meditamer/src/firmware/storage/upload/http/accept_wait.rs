use core::{future::Future, pin::pin};
use embassy_futures::select::{select, Either};

pub(super) async fn wait_accept<F, G, T, Tick>(
    accept: F,
    mut gate_open: G,
    mut tick: T,
) -> Option<F::Output>
where
    F: Future,
    G: FnMut() -> bool,
    T: FnMut() -> Tick,
    Tick: Future<Output = ()>,
{
    // Timer polls must retain the actual accept future: restarting accept
    // calls listen again, which rejects an in-progress TCP handshake.
    let mut accept = pin!(accept);
    loop {
        if !gate_open() {
            return None;
        }
        match select(accept.as_mut(), tick()).await {
            Either::First(result) => return Some(result),
            Either::Second(()) => {}
        }
    }
}
