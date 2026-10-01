extern crate std;
use super::*;
use core::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};
use embedded_hal::i2c::ErrorKind;
use std::cell::Cell;

struct Bus<'a> {
    calls: &'a Cell<usize>,
    stalled: &'a Cell<bool>,
}
impl OwnerBus for Bus<'_> {
    type Error = ErrorKind;
    async fn transaction(
        &mut self,
        rate: BusRate,
        address: u8,
        operations: &mut [Operation<'_>],
    ) -> Result<(), Self::Error> {
        self.calls.set(self.calls.get() + 1);
        // A write effect precedes the asynchronous completion. Dropping the
        // caller cannot undo it or allow the next transaction to overtake it.
        core::future::poll_fn(|_| {
            if self.stalled.get() {
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        })
        .await;
        if address == 0x7f {
            return Err(ErrorKind::NoAcknowledge(
                embedded_hal::i2c::NoAcknowledgeSource::Address,
            ));
        }
        assert_eq!(address, 0x51);
        assert_eq!(rate, BusRate::Panel);
        for operation in operations {
            match operation {
                Operation::Write(data) => assert_eq!(*data, [7, 8]),
                Operation::Read(data) => data.fill(9),
            }
        }
        Ok(())
    }
}

const RECORD_CAP: usize = 8;

struct Recording {
    len: usize,
    diags: [Option<TimeoutDiagnostic>; RECORD_CAP],
}

static RECORDING: std::sync::Mutex<Recording> = std::sync::Mutex::new(Recording {
    len: 0,
    diags: [None; RECORD_CAP],
});

fn record_sink(diag: TimeoutDiagnostic) {
    let mut recording = RECORDING.lock().unwrap();
    let index = recording.len;
    assert!(index < RECORD_CAP);
    recording.diags[index] = Some(diag);
    recording.len = index + 1;
}

fn clear_recording() {
    let mut recording = RECORDING.lock().unwrap();
    recording.len = 0;
    recording.diags = [None; RECORD_CAP];
}

fn recorded_len() -> usize {
    RECORDING.lock().unwrap().len
}

fn recorded_get(index: usize) -> TimeoutDiagnostic {
    RECORDING.lock().unwrap().diags[index].expect("missing diagnostic")
}

#[test]
fn configuration_and_multi_operation_transactions_are_forwarded_together() {
    let _clock = crate::TEST_CLOCK.lock().unwrap();
    let proxy = BusProxy::new();
    let calls = Cell::new(0);
    let stalled = Cell::new(false);
    let mut bus = Bus {
        calls: &calls,
        stalled: &stalled,
    };
    let mut owner = pin!(proxy.serve(&mut bus));
    let mut device = proxy.device(BusRate::Panel);
    let mut a = [0; 11];
    let mut b = [0; 2];
    let mut operations = [
        Operation::Write(&[7, 8]),
        Operation::Read(&mut a),
        Operation::Write(&[7, 8]),
        Operation::Read(&mut b),
    ];
    let mut cx = Context::from_waker(Waker::noop());
    {
        let mut call = pin!(device.transaction(0x51, &mut operations));
        assert!(call.as_mut().poll(&mut cx).is_pending());
        assert!(owner.as_mut().poll(&mut cx).is_pending());
        assert!(matches!(call.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    }
    assert_eq!(calls.get(), 1);
    assert_eq!(a, [9; 11]);
    assert_eq!(b, [9; 2]);
}

#[test]
fn queued_cancellation_never_starts_and_stale_reply_cannot_complete_new_call() {
    let _clock = crate::TEST_CLOCK.lock().unwrap();
    clear_recording();
    let proxy = BusProxy::with_timeout_sink(Some(record_sink));
    let calls = Cell::new(0);
    let stalled = Cell::new(false);
    let mut bus = Bus {
        calls: &calls,
        stalled: &stalled,
    };
    let mut owner = pin!(proxy.serve(&mut bus));
    let mut device = proxy.device(BusRate::Panel);
    let mut cx = Context::from_waker(Waker::noop());
    {
        let mut call = pin!(device.write(0x51, &[7, 8]));
        assert!(call.as_mut().poll(&mut cx).is_pending());
    }
    assert!(owner.as_mut().poll(&mut cx).is_pending());
    assert_eq!(calls.get(), 0);
    proxy.replies.signal(Reply {
        id: 1,
        data: [42; CAPACITY],
        result: Ok(()),
    });
    let mut output = [0; 11];
    {
        let mut call = pin!(device.read(0x51, &mut output));
        assert!(call.as_mut().poll(&mut cx).is_pending());
        assert!(owner.as_mut().poll(&mut cx).is_pending());
        assert!(matches!(call.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    }
    assert_eq!(output, [9; 11]);
    assert_eq!(recorded_len(), 0);
}

#[test]
fn concurrent_caller_waits_for_late_write_completion_after_cancellation() {
    let _clock = crate::TEST_CLOCK.lock().unwrap();
    let proxy = BusProxy::new();
    let calls = Cell::new(0);
    let stalled = Cell::new(true);
    let mut bus = Bus {
        calls: &calls,
        stalled: &stalled,
    };
    let mut owner = pin!(proxy.serve(&mut bus));
    let mut first = proxy.device(BusRate::Panel);
    let mut second = proxy.device(BusRate::Panel);
    let mut cx = Context::from_waker(Waker::noop());
    let mut next = pin!(second.write(0x51, &[7, 8]));
    {
        let mut call = pin!(first.write(0x51, &[7, 8]));
        assert!(call.as_mut().poll(&mut cx).is_pending());
        assert!(owner.as_mut().poll(&mut cx).is_pending());
        assert_eq!(calls.get(), 1);
        assert!(next.as_mut().poll(&mut cx).is_pending());
    }
    assert!(next.as_mut().poll(&mut cx).is_pending());
    assert!(owner.as_mut().poll(&mut cx).is_pending());
    assert_eq!(calls.get(), 1);
    stalled.set(false);
    assert!(owner.as_mut().poll(&mut cx).is_pending());
    assert_eq!(calls.get(), 2);
    assert!(matches!(next.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
}

#[test]
fn expired_queue_and_inflight_timeout_are_distinct_and_owner_recovers() {
    let _clock = crate::TEST_CLOCK.lock().unwrap();
    let proxy = BusProxy::new();
    let calls = Cell::new(0);
    let stalled = Cell::new(false);
    let mut bus = Bus {
        calls: &calls,
        stalled: &stalled,
    };
    let mut owner = pin!(proxy.serve(&mut bus));
    let mut device = proxy.device(BusRate::Panel);
    let mut cx = Context::from_waker(Waker::noop());
    {
        let mut call = pin!(device.write(0x51, &[7, 8]));
        assert!(call.as_mut().poll(&mut cx).is_pending());
        embassy_time::MockDriver::get().advance(Duration::from_millis(41));
        assert!(owner.as_mut().poll(&mut cx).is_pending());
        assert_eq!(calls.get(), 0);
        assert!(matches!(
            call.as_mut().poll(&mut cx),
            Poll::Ready(Err(ProxyError::QueueTimeout))
        ));
    }
    stalled.set(true);
    {
        let mut call = pin!(device.write(0x51, &[7, 8]));
        assert!(call.as_mut().poll(&mut cx).is_pending());
        assert!(owner.as_mut().poll(&mut cx).is_pending());
        embassy_time::MockDriver::get().advance(Duration::from_millis(41));
        assert!(owner.as_mut().poll(&mut cx).is_pending());
        assert!(matches!(
            call.as_mut().poll(&mut cx),
            Poll::Ready(Err(ProxyError::TransferTimeout))
        ));
    }
    stalled.set(false);
    let mut call = pin!(device.write(0x51, &[7, 8]));
    assert!(call.as_mut().poll(&mut cx).is_pending());
    assert!(owner.as_mut().poll(&mut cx).is_pending());
    assert!(matches!(call.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
}

#[test]
fn invalid_capacity_and_identity_exhaustion_do_not_queue_work() {
    let _clock = crate::TEST_CLOCK.lock().unwrap();
    let proxy = BusProxy::<ErrorKind>::new();
    let mut device = proxy.device(BusRate::Standard);
    let mut cx = Context::from_waker(Waker::noop());
    {
        let mut call = pin!(device.write(0x51, &[0; CAPACITY + 1]));
        assert!(matches!(
            call.as_mut().poll(&mut cx),
            Poll::Ready(Err(ProxyError::Capacity))
        ));
    }
    proxy.sequence.store(CLAIMED - 1, Ordering::Relaxed);
    let mut call = pin!(device.write(0x51, &[7, 8]));
    assert!(matches!(
        call.as_mut().poll(&mut cx),
        Poll::Ready(Err(ProxyError::IdentityExhausted))
    ));
    assert!(proxy.requests.try_receive().is_err());
}

#[test]
fn waiting_caller_timeout_does_not_cancel_admitted_caller_and_bus_errors_survive() {
    let _clock = crate::TEST_CLOCK.lock().unwrap();
    let proxy = BusProxy::new();
    let calls = Cell::new(0);
    let stalled = Cell::new(false);
    let mut bus = Bus {
        calls: &calls,
        stalled: &stalled,
    };
    let mut owner = pin!(proxy.serve(&mut bus));
    let mut first = proxy.device(BusRate::Panel);
    let mut second = proxy.device(BusRate::Panel);
    let mut cx = Context::from_waker(Waker::noop());
    let mut call = pin!(first.write(0x7f, &[7, 8]));
    assert!(call.as_mut().poll(&mut cx).is_pending());
    assert!(owner.as_mut().poll(&mut cx).is_pending());
    {
        let mut waiting = pin!(second.write(0x51, &[7, 8]));
        assert!(waiting.as_mut().poll(&mut cx).is_pending());
        embassy_time::MockDriver::get().advance(Duration::from_millis(41));
        assert!(matches!(
            waiting.as_mut().poll(&mut cx),
            Poll::Ready(Err(ProxyError::QueueTimeout))
        ));
    }
    assert_eq!(calls.get(), 1);
    assert!(matches!(
        call.as_mut().poll(&mut cx),
        Poll::Ready(Err(ProxyError::Bus(ErrorKind::NoAcknowledge(_))))
    ));
}

#[test]
fn client_lock_timeout_reports_no_id_and_sees_executing_owner() {
    let _clock = crate::TEST_CLOCK.lock().unwrap();
    clear_recording();
    let proxy = BusProxy::<ErrorKind>::with_timeout_sink(Some(record_sink));
    let calls = Cell::new(0);
    let stalled = Cell::new(true);
    let mut bus = Bus {
        calls: &calls,
        stalled: &stalled,
    };
    let mut owner = pin!(proxy.serve(&mut bus));
    let mut first = proxy.device(BusRate::Panel);
    let mut second = proxy.device(BusRate::Panel);
    let mut cx = Context::from_waker(Waker::noop());
    let mut first_call = pin!(first.write(0x51, &[7, 8]));
    assert!(first_call.as_mut().poll(&mut cx).is_pending());
    assert!(owner.as_mut().poll(&mut cx).is_pending());
    assert_eq!(calls.get(), 1);
    let mut second_call = pin!(second.write(0x51, &[7, 8]));
    assert!(second_call.as_mut().poll(&mut cx).is_pending());
    embassy_time::MockDriver::get().advance(Duration::from_millis(41));
    assert!(matches!(
        second_call.as_mut().poll(&mut cx),
        Poll::Ready(Err(ProxyError::QueueTimeout))
    ));
    assert_eq!(recorded_len(), 1);
    let diag = recorded_get(0);
    assert_eq!(diag.phase, TimeoutPhase::ClientLock);
    assert_eq!(diag.id, 0);
    assert_eq!(diag.address, 0x51);
    assert!(!diag.claimed);
    assert!(diag.elapsed_us >= 40_000);
    assert!(diag.elapsed_us < 80_000);
    assert_eq!(diag.owner_phase, OwnerPhase::Executing);
    assert_eq!(diag.owner_id, 1);
    assert_eq!(diag.owner_address, 0x51);
    assert!(diag.owner_age_us >= 40_000);
    assert_eq!(diag.received, 1);
    assert_eq!(diag.discarded, 0);
    assert_eq!(calls.get(), 1);
    assert!(first_call.as_mut().poll(&mut cx).is_pending());
    stalled.set(false);
    assert!(owner.as_mut().poll(&mut cx).is_pending());
    assert!(matches!(
        first_call.as_mut().poll(&mut cx),
        Poll::Ready(Ok(()))
    ));
    assert_eq!(recorded_len(), 1);
}

#[test]
fn publish_timeout_when_channel_held_by_cancelled_request() {
    let _clock = crate::TEST_CLOCK.lock().unwrap();
    clear_recording();
    let proxy = BusProxy::<ErrorKind>::with_timeout_sink(Some(record_sink));
    let calls = Cell::new(0);
    let stalled = Cell::new(false);
    let mut bus = Bus {
        calls: &calls,
        stalled: &stalled,
    };
    let _owner = pin!(proxy.serve(&mut bus));
    let mut device = proxy.device(BusRate::Panel);
    let mut cx = Context::from_waker(Waker::noop());
    {
        let mut first = pin!(device.write(0x51, &[7, 8]));
        assert!(first.as_mut().poll(&mut cx).is_pending());
    }
    assert_eq!(calls.get(), 0);
    let mut second = pin!(device.write(0x51, &[7, 8]));
    assert!(second.as_mut().poll(&mut cx).is_pending());
    embassy_time::MockDriver::get().advance(Duration::from_millis(41));
    assert!(matches!(
        second.as_mut().poll(&mut cx),
        Poll::Ready(Err(ProxyError::QueueTimeout))
    ));
    assert_eq!(recorded_len(), 1);
    let diag = recorded_get(0);
    assert_eq!(diag.phase, TimeoutPhase::Publish);
    assert_eq!(diag.id, 2);
    assert_eq!(diag.address, 0x51);
    assert!(!diag.claimed);
    assert!(diag.elapsed_us >= 40_000);
    assert!(diag.elapsed_us < 80_000);
    assert_eq!(diag.owner_phase, OwnerPhase::NotStarted);
    assert_eq!(diag.owner_id, 0);
    assert_eq!(diag.owner_address, 0xff);
    assert_eq!(diag.owner_age_us, 0);
    assert_eq!(diag.received, 0);
    assert_eq!(diag.discarded, 0);
    assert_eq!(calls.get(), 0);
}

#[test]
fn owner_claim_timeout_before_first_poll_then_owner_discards_and_recovers() {
    let _clock = crate::TEST_CLOCK.lock().unwrap();
    clear_recording();
    let proxy = BusProxy::<ErrorKind>::with_timeout_sink(Some(record_sink));
    let calls = Cell::new(0);
    let stalled = Cell::new(false);
    let mut bus = Bus {
        calls: &calls,
        stalled: &stalled,
    };
    let mut owner = pin!(proxy.serve(&mut bus));
    let mut device = proxy.device(BusRate::Panel);
    let mut cx = Context::from_waker(Waker::noop());
    {
        let mut call = pin!(device.write(0x51, &[7, 8]));
        assert!(call.as_mut().poll(&mut cx).is_pending());
        embassy_time::MockDriver::get().advance(Duration::from_millis(41));
        assert!(matches!(
            call.as_mut().poll(&mut cx),
            Poll::Ready(Err(ProxyError::QueueTimeout))
        ));
    }
    assert_eq!(recorded_len(), 1);
    let diag = recorded_get(0);
    assert_eq!(diag.phase, TimeoutPhase::OwnerClaim);
    assert_eq!(diag.id, 1);
    assert_eq!(diag.address, 0x51);
    assert!(!diag.claimed);
    assert!(diag.elapsed_us >= 40_000);
    assert!(diag.elapsed_us < 80_000);
    assert_eq!(diag.owner_phase, OwnerPhase::NotStarted);
    assert_eq!(diag.owner_id, 0);
    assert_eq!(diag.owner_address, 0xff);
    assert_eq!(diag.owner_age_us, 0);
    assert_eq!(diag.received, 0);
    assert_eq!(diag.discarded, 0);
    assert_eq!(calls.get(), 0);
    assert!(owner.as_mut().poll(&mut cx).is_pending());
    assert_eq!(calls.get(), 0);
    {
        let mut next = pin!(device.write(0x51, &[7, 8]));
        assert!(next.as_mut().poll(&mut cx).is_pending());
        assert!(owner.as_mut().poll(&mut cx).is_pending());
        assert!(matches!(next.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    }
    assert_eq!(recorded_len(), 1);
    assert_eq!(calls.get(), 1);
}

#[test]
fn expired_owner_discard_is_counted_in_next_diagnostic() {
    let _clock = crate::TEST_CLOCK.lock().unwrap();
    clear_recording();
    let proxy = BusProxy::<ErrorKind>::with_timeout_sink(Some(record_sink));
    let calls = Cell::new(0);
    let stalled = Cell::new(false);
    let mut bus = Bus {
        calls: &calls,
        stalled: &stalled,
    };
    let mut owner = pin!(proxy.serve(&mut bus));
    let mut device = proxy.device(BusRate::Panel);
    let mut cx = Context::from_waker(Waker::noop());
    {
        let mut first = pin!(device.write(0x51, &[7, 8]));
        assert!(first.as_mut().poll(&mut cx).is_pending());
        embassy_time::MockDriver::get().advance(Duration::from_millis(41));
        assert!(matches!(
            first.as_mut().poll(&mut cx),
            Poll::Ready(Err(ProxyError::QueueTimeout))
        ));
    }
    assert_eq!(recorded_len(), 1);
    assert!(owner.as_mut().poll(&mut cx).is_pending());
    assert_eq!(calls.get(), 0);
    {
        let mut second = pin!(device.write(0x51, &[7, 8]));
        assert!(second.as_mut().poll(&mut cx).is_pending());
        embassy_time::MockDriver::get().advance(Duration::from_millis(41));
        assert!(matches!(
            second.as_mut().poll(&mut cx),
            Poll::Ready(Err(ProxyError::QueueTimeout))
        ));
    }
    assert_eq!(recorded_len(), 2);
    let diag = recorded_get(1);
    assert_eq!(diag.phase, TimeoutPhase::OwnerClaim);
    assert_eq!(diag.id, 2);
    assert_eq!(diag.address, 0x51);
    assert!(!diag.claimed);
    assert!(diag.elapsed_us >= 40_000);
    assert_eq!(diag.owner_phase, OwnerPhase::Waiting);
    assert!(diag.owner_age_us >= 40_000);
    assert_eq!(diag.received, 1);
    assert_eq!(diag.discarded, 1);
    assert_eq!(calls.get(), 0);
    assert!(owner.as_mut().poll(&mut cx).is_pending());
    {
        let mut next = pin!(device.write(0x51, &[7, 8]));
        assert!(next.as_mut().poll(&mut cx).is_pending());
        assert!(owner.as_mut().poll(&mut cx).is_pending());
        assert!(matches!(next.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    }
    assert_eq!(recorded_len(), 2);
}

#[test]
fn transfer_reply_timeout_while_owner_still_executing() {
    let _clock = crate::TEST_CLOCK.lock().unwrap();
    clear_recording();
    let proxy = BusProxy::<ErrorKind>::with_timeout_sink(Some(record_sink));
    let calls = Cell::new(0);
    let stalled = Cell::new(true);
    let mut bus = Bus {
        calls: &calls,
        stalled: &stalled,
    };
    let mut owner = pin!(proxy.serve(&mut bus));
    let mut device = proxy.device(BusRate::Panel);
    let mut cx = Context::from_waker(Waker::noop());
    {
        let mut call = pin!(device.write(0x51, &[7, 8]));
        assert!(call.as_mut().poll(&mut cx).is_pending());
        assert!(owner.as_mut().poll(&mut cx).is_pending());
        assert_eq!(calls.get(), 1);
        embassy_time::MockDriver::get().advance(Duration::from_millis(41));
        assert!(call.as_mut().poll(&mut cx).is_pending());
        assert_eq!(recorded_len(), 0);
        // Keep the owner unpolled across both advances so the snapshot still sees it executing.
        embassy_time::MockDriver::get().advance(Duration::from_millis(41));
        assert!(matches!(
            call.as_mut().poll(&mut cx),
            Poll::Ready(Err(ProxyError::TransferTimeout))
        ));
    }
    assert_eq!(recorded_len(), 1);
    let diag = recorded_get(0);
    assert_eq!(diag.phase, TimeoutPhase::TransferReply);
    assert_eq!(diag.id, 1);
    assert_eq!(diag.address, 0x51);
    assert!(diag.claimed);
    assert!(diag.elapsed_us >= 80_000);
    assert_eq!(diag.owner_phase, OwnerPhase::Executing);
    assert_eq!(diag.owner_id, 1);
    assert_eq!(diag.owner_address, 0x51);
    assert!(diag.owner_age_us >= 80_000);
    assert_eq!(diag.received, 1);
    assert_eq!(diag.discarded, 0);
    assert_eq!(calls.get(), 1);
    assert!(owner.as_mut().poll(&mut cx).is_pending());
    assert_eq!(recorded_len(), 1);
    stalled.set(false);
    {
        let mut next = pin!(device.write(0x51, &[7, 8]));
        assert!(next.as_mut().poll(&mut cx).is_pending());
        assert!(owner.as_mut().poll(&mut cx).is_pending());
        assert!(matches!(next.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    }
    assert_eq!(recorded_len(), 1);
}

#[test]
fn timeout_sink_stays_silent_on_success_capacity_and_bus_errors() {
    let _clock = crate::TEST_CLOCK.lock().unwrap();
    clear_recording();
    let proxy = BusProxy::<ErrorKind>::with_timeout_sink(Some(record_sink));
    let calls = Cell::new(0);
    let stalled = Cell::new(false);
    let mut bus = Bus {
        calls: &calls,
        stalled: &stalled,
    };
    let mut owner = pin!(proxy.serve(&mut bus));
    let mut device = proxy.device(BusRate::Panel);
    let mut cx = Context::from_waker(Waker::noop());
    {
        let mut call = pin!(device.write(0x51, &[7, 8]));
        assert!(call.as_mut().poll(&mut cx).is_pending());
        assert!(owner.as_mut().poll(&mut cx).is_pending());
        assert!(matches!(call.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    }
    assert_eq!(recorded_len(), 0);
    {
        let mut call = pin!(device.write(0x51, &[0; CAPACITY + 1]));
        assert!(matches!(
            call.as_mut().poll(&mut cx),
            Poll::Ready(Err(ProxyError::Capacity))
        ));
    }
    assert_eq!(recorded_len(), 0);
    assert!(proxy.requests.try_receive().is_err());
    {
        let mut call = pin!(device.write(0x7f, &[7, 8]));
        assert!(call.as_mut().poll(&mut cx).is_pending());
        assert!(owner.as_mut().poll(&mut cx).is_pending());
        assert!(matches!(
            call.as_mut().poll(&mut cx),
            Poll::Ready(Err(ProxyError::Bus(_)))
        ));
    }
    assert_eq!(recorded_len(), 0);
    proxy.sequence.store(CLAIMED - 1, Ordering::Relaxed);
    {
        let mut call = pin!(device.write(0x51, &[7, 8]));
        assert!(matches!(
            call.as_mut().poll(&mut cx),
            Poll::Ready(Err(ProxyError::IdentityExhausted))
        ));
    }
    assert_eq!(recorded_len(), 0);
    assert!(proxy.requests.try_receive().is_err());
}

#[test]
fn owner_timeout_reply_remains_claimed_after_owner_returns_to_waiting() {
    let _clock = crate::TEST_CLOCK.lock().unwrap();
    clear_recording();
    let proxy = BusProxy::<ErrorKind>::with_timeout_sink(Some(record_sink));
    let calls = Cell::new(0);
    let stalled = Cell::new(true);
    let mut bus = Bus {
        calls: &calls,
        stalled: &stalled,
    };
    let mut owner = pin!(proxy.serve(&mut bus));
    let mut device = proxy.device(BusRate::Panel);
    let mut cx = Context::from_waker(Waker::noop());
    let mut call = pin!(device.write(0x51, &[7, 8]));
    assert!(call.as_mut().poll(&mut cx).is_pending());
    assert!(owner.as_mut().poll(&mut cx).is_pending());
    embassy_time::MockDriver::get().advance(Duration::from_millis(41));
    assert!(owner.as_mut().poll(&mut cx).is_pending());
    assert!(matches!(
        call.as_mut().poll(&mut cx),
        Poll::Ready(Err(ProxyError::TransferTimeout))
    ));
    assert_eq!(recorded_len(), 1);
    let diag = recorded_get(0);
    assert_eq!(diag.phase, TimeoutPhase::TransferReply);
    assert!(diag.claimed);
    assert_eq!(diag.owner_phase, OwnerPhase::Waiting);
    assert_eq!((diag.id, diag.owner_id, diag.address), (1, 1, 0x51));
    assert_eq!((diag.received, diag.discarded, calls.get()), (1, 0, 1));
}
