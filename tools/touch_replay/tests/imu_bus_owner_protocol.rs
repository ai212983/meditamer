#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ImuRequest {
    id: u32,
    requested_at_us: u64,
    tap_register: u8,
    axes_register: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ImuReply {
    id: u32,
    queue_us: u64,
    tap_source: u8,
    axes: [u8; 12],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Empty,
    Queued(ImuRequest),
    Claimed {
        request: ImuRequest,
        abandoned: bool,
    },
}

#[derive(Debug, PartialEq, Eq)]
struct OwnerProtocol {
    next_id: u32,
    state: State,
    reply: Option<ImuReply>,
    queue_max_us: u64,
}

const CLAIMED: u32 = 1 << 31;

impl OwnerProtocol {
    fn new() -> Self {
        Self {
            next_id: 0,
            state: State::Empty,
            reply: None,
            queue_max_us: 0,
        }
    }

    fn request(
        &mut self,
        requested_at_us: u64,
        tap_register: u8,
        axes_register: u8,
    ) -> Option<u32> {
        if self.state != State::Empty {
            return None;
        }
        self.next_id = if self.next_id >= CLAIMED - 1 {
            1
        } else {
            self.next_id + 1
        };
        let id = self.next_id;
        self.state = State::Queued(ImuRequest {
            id,
            requested_at_us,
            tap_register,
            axes_register,
        });
        Some(id)
    }

    fn claim(&mut self) -> Option<ImuRequest> {
        let State::Queued(request) = self.state else {
            return None;
        };
        self.state = State::Claimed {
            request,
            abandoned: false,
        };
        Some(request)
    }

    fn cancel(&mut self, id: u32) -> bool {
        match self.state {
            State::Queued(request) if request.id == id => {
                self.state = State::Empty;
                true
            }
            State::Claimed { request, .. } if request.id == id => {
                self.state = State::Claimed {
                    request,
                    abandoned: true,
                };
                false
            }
            _ => false,
        }
    }

    fn complete(
        &mut self,
        id: u32,
        bus_acquired_at_us: u64,
        tap_source: u8,
        axes: [u8; 12],
    ) -> Option<ImuReply> {
        let State::Claimed { request, abandoned } = self.state else {
            return None;
        };
        if request.id != id {
            return None;
        }
        let queue_us = bus_acquired_at_us - request.requested_at_us;
        self.queue_max_us = self.queue_max_us.max(queue_us);
        self.state = State::Empty;
        if abandoned {
            return None;
        }
        let reply = ImuReply {
            id,
            queue_us,
            tap_source,
            axes,
        };
        self.reply = Some(reply);
        Some(reply)
    }

    fn take_reply(&mut self, id: u32) -> Option<ImuReply> {
        if self.reply.is_some_and(|reply| reply.id == id) {
            self.reply.take()
        } else {
            None
        }
    }
}

#[test]
fn caller_timestamp_includes_wait_before_owner_claim_and_bus_admission() {
    let mut owner = OwnerProtocol::new();
    let id = owner.request(100, 0x1c, 0x22).unwrap();
    assert_eq!(owner.claim().unwrap().id, id);
    let reply = owner.complete(id, 4_600, 0x40, [3; 12]).unwrap();
    assert_eq!(reply.queue_us, 4_500);
    assert_eq!(owner.queue_max_us, 4_500);
    assert_eq!((reply.tap_source, reply.axes), (0x40, [3; 12]));
    assert_eq!(owner.take_reply(id), Some(reply));
}

#[test]
fn cancellation_before_claim_prevents_execution() {
    let mut owner = OwnerProtocol::new();
    let id = owner.request(100, 0x1c, 0x22).unwrap();
    assert!(owner.cancel(id));
    assert_eq!(owner.claim(), None);
    assert_eq!(owner.queue_max_us, 0);
    assert!(owner.request(200, 0x1c, 0x22).is_some());
}

#[test]
fn cancellation_after_claim_finishes_without_reply_or_overlap() {
    let mut owner = OwnerProtocol::new();
    let id = owner.request(100, 0x1c, 0x22).unwrap();
    assert_eq!(owner.claim().unwrap().id, id);
    assert!(!owner.cancel(id));
    assert_eq!(owner.request(200, 0x1c, 0x22), None);
    assert_eq!(owner.complete(id, 600, 0, [0; 12]), None);
    assert_eq!(owner.reply, None);
    assert_eq!(owner.queue_max_us, 500);
    assert!(owner.request(700, 0x1c, 0x22).is_some());
}

#[test]
fn stale_completion_cannot_release_a_different_request() {
    let mut owner = OwnerProtocol::new();
    let old = owner.request(100, 0x1c, 0x22).unwrap();
    assert!(owner.cancel(old));
    let current = owner.request(200, 0x1c, 0x22).unwrap();
    assert_eq!(owner.claim().unwrap().id, current);
    assert_eq!(owner.complete(old, 300, 0, [0; 12]), None);
    assert_eq!(
        owner.state,
        State::Claimed {
            request: ImuRequest {
                id: current,
                requested_at_us: 200,
                tap_register: 0x1c,
                axes_register: 0x22,
            },
            abandoned: false,
        }
    );
    assert!(owner.complete(current, 400, 0, [0; 12]).is_some());
    assert_eq!(owner.take_reply(old), None);
    assert_eq!(owner.take_reply(current).unwrap().queue_us, 200);
}

#[test]
fn request_identity_wraps_without_using_claim_bit() {
    let mut owner = OwnerProtocol::new();
    owner.next_id = CLAIMED - 2;
    let last = owner.request(100, 0x1c, 0x22).unwrap();
    assert_eq!(last, CLAIMED - 1);
    assert!(owner.cancel(last));
    let wrapped = owner.request(200, 0x1c, 0x22).unwrap();
    assert_eq!(wrapped, 1);
}

static BUS: admission::Admission = admission::Admission::new();
static REQUESTS: Channel<CriticalSectionRawMutex, u32, 1> = Channel::new();
static TOUCH_READY: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static ORDER: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
static DONE: AtomicBool = AtomicBool::new(false);

#[embassy_executor::task]
async fn bus_worker() {
    let _request = REQUESTS.receive().await;
    let permit = BUS.acquire(0x6b).await.unwrap();
    ORDER.lock().unwrap().push("tap");
    ORDER.lock().unwrap().push("axes");
    drop(permit);
}

#[embassy_executor::task]
async fn touch_waiter() {
    TOUCH_READY.wait().await;
    let permit = BUS.acquire(0x15).await.unwrap();
    ORDER.lock().unwrap().push("touch");
    drop(permit);
    DONE.store(true, Ordering::SeqCst);
}

#[embassy_executor::task]
async fn imu_caller() {
    REQUESTS.send(1).await;
    TOUCH_READY.signal(());
}

#[test]
fn fixed_priority_worker_releases_pair_before_touch_enters_fifo() {
    let executor = Box::leak(Box::new(Executor::new()));
    executor.run_until(
        |spawner: Spawner| {
            let worker = bus_worker().unwrap();
            worker.metadata().set_priority(4);
            spawner.spawn(worker);
            let touch = touch_waiter().unwrap();
            touch.metadata().set_priority(3);
            spawner.spawn(touch);
            let caller = imu_caller().unwrap();
            caller.metadata().set_priority(1);
            spawner.spawn(caller);
        },
        || DONE.load(Ordering::SeqCst),
    );
    assert_eq!(*ORDER.lock().unwrap(), ["tap", "axes", "touch"]);
}
#[path = "../../../products/meditamer/src/firmware/types/i2c/admission.rs"]
mod admission;

use embassy_executor::{Executor, Spawner};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel, signal::Signal,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
