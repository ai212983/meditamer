//! Regressions use the production desired-state protocol and participant loop.
//! Fake deadlines make delayed actors and dropped callers deterministic.

#[path = "../../../products/meditamer/src/firmware/bounded_control.rs"]
mod bounded_control;

use bounded_control::{
    all_permit_shared_bus_access, Control, ControlAck, ControlCommand, SuspendAck, WaitOutcome,
};
use core::{
    future::{pending, ready, Future},
    pin::Pin,
    task::{Context, Poll, Waker},
};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;

type Client = Control<NoopRawMutex>;

fn poll<F: Future>(future: Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}

fn immediately<F: Future>(future: F) -> F::Output {
    let mut future = core::pin::pin!(future);
    match poll(future.as_mut()) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("expected immediate completion"),
    }
}

#[test]
fn late_same_state_ack_cannot_authorize_a_new_suspend() {
    let client = Client::new();
    let a = client.request_suspend().unwrap();
    assert_eq!(
        immediately(client.wait_suspended(a, ready(()))),
        SuspendAck::TimedOut
    );
    // Actor A finishes after its caller gave up. Its retained ack must not
    // authorize B, even if resume A has not yet been handled by the actor.
    assert!(client.acknowledge(a, ControlAck::Quiesced));
    let resume = client.request_resume(false);
    let b = client.request_suspend().unwrap();
    assert_ne!(a.id(), b.id());
    assert_eq!(client.current_id(), b.id());
    assert!(!client.acknowledge(a, ControlAck::Quiesced));
    assert!(!client.acknowledge(resume, ControlAck::Running));
    assert_eq!(
        immediately(client.wait_suspended(b, ready(()))),
        SuspendAck::TimedOut
    );
    assert_eq!(
        immediately(client.wait_suspended(a, ready(()))),
        SuspendAck::Superseded
    );
    assert_eq!(client.try_receive(), Some(b));
    assert!(client.acknowledge(b, ControlAck::Quiesced));
    assert_eq!(
        immediately(client.wait_suspended(b, ready(()))),
        SuspendAck::Quiesced
    );
}

#[test]
fn recovery_survives_timeout_and_replaces_delayed_suspend_intent() {
    let client = Client::new();
    let a = client.request_suspend().unwrap();
    let mut actor = core::pin::pin!(client.hold_suspended(a, true));
    assert!(poll(actor.as_mut()).is_pending());
    assert_eq!(
        immediately(client.wait_suspended(a, ready(()))),
        SuspendAck::Quiesced
    );
    // Exact old lost-resume schedule: actor delayed, R1 then S2 then R2.
    // All caller waits expire. Recovery must need no subsequent panel request.
    assert!(!immediately(client.resume(false, ready(()))));
    assert_eq!(immediately(client.suspend(ready(()))), SuspendAck::TimedOut);
    let recovery = client.request_resume(false);
    assert_eq!(
        immediately(client.wait(recovery, ready(()))),
        WaitOutcome::TimedOut
    );
    assert!(poll(actor.as_mut()).is_ready());
    assert_eq!(
        immediately(client.wait(recovery, ready(()))),
        WaitOutcome::Acknowledged(ControlAck::Running)
    );
    assert!(client.try_receive().is_none());
}

#[test]
fn dropping_a_waiter_does_not_cancel_the_desired_request() {
    let client = Client::new();
    let request = client.request_suspend().unwrap();
    {
        let mut waiter = core::pin::pin!(client.wait_suspended(request, pending()));
        assert!(poll(waiter.as_mut()).is_pending());
    }
    assert_eq!(immediately(client.receive()), request);
    let mut actor = core::pin::pin!(client.hold_suspended(request, true));
    assert!(poll(actor.as_mut()).is_pending());
    assert_eq!(
        immediately(client.wait_suspended(request, ready(()))),
        SuspendAck::Quiesced
    );
    let resume = client.request_resume(false);
    assert!(poll(actor.as_mut()).is_ready());
    assert_eq!(
        immediately(client.wait(resume, ready(()))),
        WaitOutcome::Acknowledged(ControlAck::Running)
    );
}

#[test]
fn acknowledgement_must_match_both_request_and_terminal_state() {
    let client = Client::new();
    let suspended = client.request_suspend().unwrap();
    assert!(!client.acknowledge(suspended, ControlAck::Running));
    let mut actor = core::pin::pin!(client.hold_suspended(suspended, false));
    assert!(poll(actor.as_mut()).is_pending());
    assert_eq!(
        immediately(client.wait_suspended(suspended, ready(()))),
        SuspendAck::CleanupFailed
    );
    let running = client.request_resume(false);
    assert!(!client.acknowledge(running, ControlAck::Quiesced));
    assert!(!client.acknowledge(running, ControlAck::CleanupFailed));
    assert!(poll(actor.as_mut()).is_ready());
    assert_eq!(
        immediately(client.wait(running, ready(()))),
        WaitOutcome::Acknowledged(ControlAck::Running)
    );
}

#[test]
fn reset_obligation_survives_coalescing_and_stale_completion() {
    let client = Client::new();
    let first = client.request_resume(true);
    client.request_suspend().unwrap();
    let latest = client.request_resume(false);
    assert_eq!(
        latest.command,
        ControlCommand::Resume {
            reset_pipeline: true
        }
    );
    assert!(!client.acknowledge(first, ControlAck::Running));
    let latest = client.request_resume(false);
    assert_eq!(
        latest.command,
        ControlCommand::Resume {
            reset_pipeline: true
        }
    );
    assert_eq!(client.try_receive(), Some(latest));
    assert!(client.acknowledge(latest, ControlAck::Running));
    assert_eq!(
        client.request_resume(false).command,
        ControlCommand::Resume {
            reset_pipeline: false
        }
    );
}

#[test]
fn partial_suspension_recovers_healthy_and_eventually_unblocked_clients() {
    let clients: [Client; 5] = core::array::from_fn(|_| Client::new());
    let requests = clients
        .each_ref()
        .map(|client| client.request_suspend().unwrap());
    let mut actors: [_; 5] =
        core::array::from_fn(|i| Box::pin(clients[i].hold_suspended(requests[i], true)));
    for actor in actors.iter_mut().take(4) {
        assert!(poll(actor.as_mut()).is_pending());
    }
    let acks =
        core::array::from_fn(|i| immediately(clients[i].wait_suspended(requests[i], ready(()))));
    assert_eq!(acks[4], SuspendAck::TimedOut);
    assert!(!all_permit_shared_bus_access(acks));
    let recovery = clients
        .each_ref()
        .map(|client| client.request_resume(false));
    for (i, actor) in actors.iter_mut().enumerate() {
        // Even the previously blocked actor sees latest Running on return.
        assert!(poll(actor.as_mut()).is_ready());
        assert_eq!(
            immediately(clients[i].wait(recovery[i], ready(()))),
            WaitOutcome::Acknowledged(ControlAck::Running)
        );
    }
}

#[test]
fn every_non_quiescent_outcome_blocks_shared_bus_access() {
    assert!(all_permit_shared_bus_access([SuspendAck::Quiesced; 5]));
    for failure in [
        SuspendAck::CleanupFailed,
        SuspendAck::TimedOut,
        SuspendAck::Superseded,
        SuspendAck::Exhausted,
    ] {
        for index in 0..5 {
            let mut acks = [SuspendAck::Quiesced; 5];
            acks[index] = failure;
            assert!(!all_permit_shared_bus_access(acks));
        }
    }
}

#[test]
fn diagnostic_snapshot_never_pairs_new_intent_with_old_acknowledgement() {
    let control = Client::new();
    let suspend = control.request_suspend().unwrap();
    assert_eq!(control.snapshot(), (suspend, None));
    assert!(control.acknowledge(suspend, ControlAck::Quiesced));
    assert_eq!(control.snapshot(), (suspend, Some(ControlAck::Quiesced)));
    let resume = control.request_resume(false);
    assert_eq!(control.snapshot(), (resume, None));
    assert!(!control.acknowledge(suspend, ControlAck::Quiesced));
    assert!(control.acknowledge(resume, ControlAck::Running));
    assert_eq!(control.snapshot(), (resume, Some(ControlAck::Running)));
}

#[test]
fn full_output_queue_cannot_block_suspend_or_resume() {
    use embassy_sync::channel::Channel;
    let control = Client::new();
    let queue = Channel::<NoopRawMutex, u8, 1>::new();
    queue.try_send(1).unwrap();
    let actor = async {
        let request = control.interruptible(queue.send(2)).await.unwrap_err();
        control.hold_suspended(request, true).await;
        // The interrupted sample is dropped; it must not appear after resume.
    };
    let mut actor = core::pin::pin!(actor);
    assert!(poll(actor.as_mut()).is_pending());
    let suspend = control.request_suspend().unwrap();
    assert!(poll(actor.as_mut()).is_pending());
    assert_eq!(
        immediately(control.wait_suspended(suspend, ready(()))),
        SuspendAck::Quiesced
    );
    let resume = control.request_resume(true);
    assert!(poll(actor.as_mut()).is_ready());
    assert_eq!(
        immediately(control.wait(resume, ready(()))),
        WaitOutcome::Acknowledged(ControlAck::Running)
    );
    assert_eq!(queue.try_receive(), Ok(1));
    assert!(queue.try_receive().is_err());
}
