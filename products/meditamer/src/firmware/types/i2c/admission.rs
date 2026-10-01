//! FIFO admission for the five CPU1 physical-bus clients: touch, IMU,
//! environment, battery and the CPU0 request owner. Bootstrap precedes sensors.
use core::{
    cell::RefCell,
    future::poll_fn,
    task::{Poll, Waker},
};
use critical_section::Mutex;

const CLIENTS: usize = 5;
pub(super) struct Admission {
    state: Mutex<RefCell<State>>,
}
struct State {
    held: bool,
    held_address: u8,
    order: heapless::Vec<u8, CLIENTS>,
    wakers: [Option<Waker>; CLIENTS],
    woke: [Option<u32>; CLIENTS],
    addresses: [u8; CLIENTS],
    queued_at: [u32; CLIENTS],
    observed_head: [Option<u8>; CLIENTS],
    observations: [QueueTrace; CLIENTS],
}
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Full;

#[derive(Clone, Copy, Default)]
pub(super) struct QueueTrace {
    pub held_address: u8,
    pub head_address: u8,
    pub head_age_us: u32,
    pub head_turn_us: u32,
    pub head_turned: bool,
    pub head_cancelled: bool,
}

impl Admission {
    pub const fn new() -> Self {
        Self {
            state: Mutex::new(RefCell::new(State {
                held: false,
                held_address: 0xff,
                order: heapless::Vec::new(),
                wakers: [const { None }; CLIENTS],
                woke: [const { None }; CLIENTS],
                addresses: [0xff; CLIENTS],
                queued_at: [0; CLIENTS],
                observed_head: [None; CLIENTS],
                observations: [QueueTrace {
                    held_address: 0xff,
                    head_address: 0xff,
                    head_age_us: 0,
                    head_turn_us: 0,
                    head_turned: false,
                    head_cancelled: false,
                }; CLIENTS],
            })),
        }
    }
    fn with<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        critical_section::with(|cs| f(&mut self.state.borrow(cs).borrow_mut()))
    }

    pub async fn acquire(&self, address: u8) -> Result<Permit<'_>, Full> {
        let mut registration = Registration {
            admission: self,
            slot: None,
        };
        poll_fn(|cx| {
            self.with(|s| {
                let now = now_us();
                if !s.held && s.order.first().copied() == registration.slot {
                    let (woke, observation) = match registration.slot.take() {
                        Some(slot) => {
                            s.finish_head_turn(slot, now, false);
                            s.order.remove(0);
                            s.wakers[slot as usize] = None;
                            s.observed_head[slot as usize] = None;
                            (s.woke[slot as usize].take(), s.observations[slot as usize])
                        }
                        None => (None, s.observe(now)),
                    };
                    s.held = true;
                    s.held_address = address;
                    return Poll::Ready(Ok(Permit(self, woke, observation)));
                }
                let slot = match registration.slot {
                    Some(slot) => slot,
                    None => {
                        let Some(slot) = s.wakers.iter().position(Option::is_none) else {
                            return Poll::Ready(Err(Full));
                        };
                        let observation = s.observe(now);
                        let head = s.order.first().copied();
                        // A free waker slot implies a free queue position.
                        s.order.push(slot as u8).unwrap();
                        registration.slot = Some(slot as u8);
                        s.woke[slot] = None;
                        s.addresses[slot] = address;
                        s.queued_at[slot] = now;
                        s.observed_head[slot] = head;
                        s.observations[slot] = observation;
                        slot as u8
                    }
                };
                let waker = &mut s.wakers[slot as usize];
                if !waker.as_ref().is_some_and(|w| w.will_wake(cx.waker())) {
                    *waker = Some(cx.waker().clone());
                }
                Poll::Pending
            })
        })
        .await
    }
}
#[cfg(test)]
static TEST_NOW: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
#[cfg(test)]
fn now_us() -> u32 {
    TEST_NOW.load(core::sync::atomic::Ordering::Relaxed)
}
#[cfg(not(test))]
fn now_us() -> u32 {
    embassy_time::Instant::now().as_micros() as u32
}
impl State {
    fn observe(&self, now: u32) -> QueueTrace {
        let head = self.order.first().copied();
        QueueTrace {
            held_address: self.held_address,
            head_address: head.map_or(0xff, |slot| self.addresses[slot as usize]),
            head_age_us: head.map_or(0, |slot| now.wrapping_sub(self.queued_at[slot as usize])),
            ..QueueTrace::default()
        }
    }
    fn finish_head_turn(&mut self, head: u8, now: u32, cancelled: bool) {
        for slot in self.order.iter().copied() {
            let index = slot as usize;
            if self.observed_head[index] == Some(head) {
                self.observations[index].head_turn_us = now.wrapping_sub(self.queued_at[index]);
                self.observations[index].head_turned = true;
                self.observations[index].head_cancelled = cancelled;
                self.observed_head[index] = None;
            }
        }
    }
    fn wake_next(&mut self, now: u32) {
        if !self.held {
            if let Some(slot) = self.order.first() {
                let index = *slot as usize;
                if let Some(waker) = &self.wakers[index] {
                    if self.woke[index].is_none() {
                        self.woke[index] = Some(now);
                    }
                    waker.wake_by_ref();
                }
            }
        }
    }
}
struct Registration<'a> {
    admission: &'a Admission,
    slot: Option<u8>,
}
impl Drop for Registration<'_> {
    fn drop(&mut self) {
        if let Some(slot) = self.slot {
            self.admission.with(|s| {
                let now = now_us();
                s.finish_head_turn(slot, now, true);
                if let Some(index) = s.order.iter().position(|v| *v == slot) {
                    s.order.remove(index);
                }
                s.wakers[slot as usize] = None;
                s.woke[slot as usize] = None;
                s.observed_head[slot as usize] = None;
                s.wake_next(now);
            });
        }
    }
}
pub(super) struct Permit<'a>(&'a Admission, Option<u32>, QueueTrace);
impl Permit<'_> {
    pub(super) fn woke_at(&self) -> Option<u32> {
        self.1
    }
    pub(super) fn queue_trace(&self) -> QueueTrace {
        self.2
    }
}
impl Drop for Permit<'_> {
    fn drop(&mut self) {
        self.0.with(|s| {
            s.held = false;
            s.held_address = 0xff;
            let now = now_us();
            s.wake_next(now);
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use core::task::{RawWaker, RawWakerVTable, Waker};
    fn dummy_waker() -> Waker {
        fn clone(_: *const ()) -> RawWaker {
            RawWaker::new(core::ptr::null(), &VTABLE)
        }
        fn noop(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
        unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VTABLE)) }
    }
    fn queued(slot: u8) -> State {
        let mut s = State {
            held: true,
            held_address: 0x6b,
            order: heapless::Vec::new(),
            wakers: [const { None }; CLIENTS],
            woke: [const { None }; CLIENTS],
            addresses: [0xff; CLIENTS],
            queued_at: [0; CLIENTS],
            observed_head: [None; CLIENTS],
            observations: [QueueTrace::default(); CLIENTS],
        };
        s.order.push(slot).unwrap();
        s.wakers[slot as usize] = Some(dummy_waker());
        s
    }
    #[test]
    fn first_wake_is_kept() {
        let mut s = queued(1);
        s.held = false;
        s.wake_next(1000);
        assert_eq!(s.woke[1], Some(1000));
        s.wake_next(2000);
        assert_eq!(s.woke[1], Some(1000));
    }
    #[test]
    fn held_blocks_wake() {
        let mut s = queued(0);
        s.wake_next(1000);
        assert_eq!(s.woke[0], None);
    }
    #[test]
    fn permit_reports_wake() {
        let a = Admission::new();
        let p = Permit(
            &a,
            Some(7),
            QueueTrace {
                head_address: 0x15,
                head_age_us: 123,
                ..QueueTrace::default()
            },
        );
        assert_eq!(p.woke_at(), Some(7));
        assert_eq!(p.queue_trace().head_address, 0x15);
        assert_eq!(p.queue_trace().head_age_us, 123);
        core::mem::forget(p);
        let q = Permit(&a, None, QueueTrace::default());
        assert_eq!(q.woke_at(), None);
        core::mem::forget(q);
    }
    #[test]
    fn removal_clears_wake_for_reuse() {
        let mut s = queued(0);
        s.held = false;
        s.wake_next(500);
        assert_eq!(s.woke[0], Some(500));
        s.woke[0] = None;
        s.wakers[0] = None;
        s.order.clear();
        s.woke[0] = None;
        assert_eq!(s.woke[0], None);
        assert!(s.wakers[0].is_none());
    }
    #[test]
    fn test_clock_feeds_wake() {
        TEST_NOW.store(777, core::sync::atomic::Ordering::Relaxed);
        let mut s = queued(2);
        s.held = false;
        let now = now_us();
        s.wake_next(now);
        assert_eq!(s.woke[2], Some(777));
    }
    #[test]
    fn free_bus_head_turn_is_attributed_to_later_touch_waiter() {
        let mut s = queued(0);
        s.held = false;
        s.held_address = 0xff;
        s.addresses[0] = 0x6b;
        s.queued_at[0] = u32::MAX - 49;
        let observed = s.observe(50);
        assert_eq!(observed.held_address, 0xff);
        assert_eq!(observed.head_address, 0x6b);
        assert_eq!(observed.head_age_us, 100);
        s.order.push(1).unwrap();
        s.queued_at[1] = 50;
        s.observed_head[1] = Some(0);
        s.observations[1] = observed;
        s.finish_head_turn(0, 1450, false);
        let touch = s.observations[1];
        assert!(touch.head_turned);
        assert!(!touch.head_cancelled);
        assert_eq!(touch.head_turn_us, 1400);
    }
    #[test]
    fn cancelled_head_is_retained_when_slot_is_reused() {
        let mut s = queued(0);
        s.addresses[0] = 0x48;
        s.order.push(1).unwrap();
        s.queued_at[1] = 100;
        s.observed_head[1] = Some(0);
        s.observations[1] = s.observe(100);
        s.finish_head_turn(0, 200, true);
        s.order.remove(0);
        s.addresses[0] = 0x6b;
        s.order.push(0).unwrap();
        s.finish_head_turn(0, 300, false);
        let touch = s.observations[1];
        assert_eq!(touch.head_address, 0x48);
        assert!(touch.head_turned);
        assert!(touch.head_cancelled);
        assert_eq!(touch.head_turn_us, 100);
    }
}
