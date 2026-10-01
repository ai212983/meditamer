use embassy_executor::{Executor, Metadata, Spawner};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use std::{
    future::poll_fn,
    sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Mutex, OnceLock,
    },
    task::{Poll, Waker},
};

static SORTED_META: OnceLock<&'static Metadata> = OnceLock::new();
static SORTED_WAKER: OnceLock<Waker> = OnceLock::new();
static SORTED_RELEASED: AtomicBool = AtomicBool::new(false);
static SORTED_ORDER: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
static SORTED_DONE: AtomicU8 = AtomicU8::new(0);

#[embassy_executor::task]
async fn sorted_imu() {
    assert!(SORTED_META.set(Metadata::for_current_task().await).is_ok());
    poll_fn(|cx| {
        if SORTED_RELEASED.load(Ordering::SeqCst) {
            Poll::Ready(())
        } else {
            assert!(SORTED_WAKER.set(cx.waker().clone()).is_ok());
            Poll::Pending
        }
    })
    .await;
    SORTED_ORDER.lock().unwrap().push("imu");
    SORTED_DONE.fetch_add(1, Ordering::SeqCst);
}

#[embassy_executor::task]
async fn sorted_competitor() {
    SORTED_ORDER.lock().unwrap().push("competitor");
    SORTED_DONE.fetch_add(1, Ordering::SeqCst);
}

#[embassy_executor::task]
async fn sorted_controller() {
    SORTED_META.get().unwrap().set_priority(3);
    SORTED_WAKER.get().unwrap().wake_by_ref();
    SORTED_ORDER.lock().unwrap().push("controller");
}

#[embassy_executor::task]
async fn sorted_setup(spawner: Spawner) {
    SORTED_META.get().unwrap().set_priority(1);
    let competitor = sorted_competitor().unwrap();
    competitor.metadata().set_priority(2);
    spawner.spawn(competitor);
    let controller = sorted_controller().unwrap();
    controller.metadata().set_priority(4);
    spawner.spawn(controller);
    SORTED_RELEASED.store(true, Ordering::SeqCst);
    SORTED_WAKER.get().unwrap().wake_by_ref();
}

#[test]
fn priority_change_and_extra_wake_do_not_resort_ready_tasks() {
    let executor = Box::leak(Box::new(Executor::new()));
    executor.run_until(
        |spawner| {
            let imu = sorted_imu().unwrap();
            imu.metadata().set_priority(3);
            spawner.spawn(imu);
            let setup = sorted_setup(spawner).unwrap();
            setup.metadata().set_priority(2);
            spawner.spawn(setup);
        },
        || SORTED_DONE.load(Ordering::SeqCst) == 2,
    );
    assert_eq!(
        *SORTED_ORDER.lock().unwrap(),
        ["controller", "competitor", "imu"]
    );
}

static STACK_SIGNAL: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static STACK_META: OnceLock<&'static Metadata> = OnceLock::new();
static STACK_ORDER: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
static STACK_DONE: AtomicU8 = AtomicU8::new(0);

#[embassy_executor::task]
async fn stack_imu() {
    assert!(STACK_META.set(Metadata::for_current_task().await).is_ok());
    STACK_SIGNAL.wait().await;
    STACK_ORDER.lock().unwrap().push("imu");
    STACK_DONE.fetch_add(1, Ordering::SeqCst);
}

#[embassy_executor::task]
async fn stack_competitor() {
    STACK_ORDER.lock().unwrap().push("competitor");
    STACK_DONE.fetch_add(1, Ordering::SeqCst);
}

#[embassy_executor::task]
async fn stack_setup(spawner: Spawner) {
    STACK_META.get().unwrap().set_priority(1);
    let competitor = stack_competitor().unwrap();
    competitor.metadata().set_priority(2);
    spawner.spawn(competitor);
    STACK_SIGNAL.signal(());
    STACK_META.get().unwrap().set_priority(3);
}

#[test]
fn priority_change_before_sorting_changes_poll_order() {
    let executor = Box::leak(Box::new(Executor::new()));
    executor.run_until(
        |spawner| {
            let imu = stack_imu().unwrap();
            imu.metadata().set_priority(3);
            spawner.spawn(imu);
            let setup = stack_setup(spawner).unwrap();
            setup.metadata().set_priority(2);
            spawner.spawn(setup);
        },
        || STACK_DONE.load(Ordering::SeqCst) == 2,
    );
    assert_eq!(*STACK_ORDER.lock().unwrap(), ["imu", "competitor"]);
}
