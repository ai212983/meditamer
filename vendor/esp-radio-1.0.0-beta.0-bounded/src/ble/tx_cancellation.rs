use core::marker::PhantomData;

pub(crate) trait TxCancellationLatch {
    fn latch();
}

pub(crate) struct TxCancellationGuard<L: TxCancellationLatch> {
    armed: bool,
    latch: PhantomData<L>,
}

impl<L: TxCancellationLatch> TxCancellationGuard<L> {
    pub(crate) fn armed() -> Self {
        Self {
            armed: true,
            latch: PhantomData,
        }
    }

    pub(crate) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl<L: TxCancellationLatch> Drop for TxCancellationGuard<L> {
    fn drop(&mut self) {
        if self.armed {
            L::latch();
        }
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use core::sync::atomic::{AtomicBool, Ordering};
    use self::std::sync::Mutex;

    use super::{TxCancellationGuard, TxCancellationLatch};

    static FAULTED: AtomicBool = AtomicBool::new(false);
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    struct TestLatch;

    impl TxCancellationLatch for TestLatch {
        fn latch() {
            FAULTED.store(true, Ordering::Release);
        }
    }

    fn reset() {
        FAULTED.store(false, Ordering::Release);
    }

    #[test]
    fn cancellation_before_controller_availability_latches_fault() {
        let _test_guard = TEST_LOCK.lock().unwrap();
        reset();
        let guard = TxCancellationGuard::<TestLatch>::armed();
        drop(guard);
        assert!(FAULTED.load(Ordering::Acquire));
    }

    #[test]
    fn cancellation_after_packet_submission_latches_fault() {
        let _test_guard = TEST_LOCK.lock().unwrap();
        reset();
        let guard = TxCancellationGuard::<TestLatch>::armed();
        drop(guard);
        assert!(FAULTED.load(Ordering::Acquire));
    }

    #[test]
    fn completed_send_disarms_without_fault() {
        let _test_guard = TEST_LOCK.lock().unwrap();
        reset();
        let mut guard = TxCancellationGuard::<TestLatch>::armed();
        guard.disarm();
        drop(guard);
        assert!(!FAULTED.load(Ordering::Acquire));
    }
}
