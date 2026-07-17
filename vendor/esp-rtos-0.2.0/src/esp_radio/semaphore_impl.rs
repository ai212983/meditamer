use allocator_api2::boxed::Box;
use esp_radio_rtos_driver::{
    register_semaphore_implementation,
    semaphore::{SemaphoreImplementation, SemaphoreKind, SemaphorePtr},
};

use crate::semaphore::Semaphore;

impl Semaphore {
    unsafe fn from_ptr<'a>(ptr: SemaphorePtr) -> &'a Self {
        unsafe { ptr.cast::<Self>().as_ref() }
    }
}

impl SemaphoreImplementation for Semaphore {
    fn create(kind: SemaphoreKind) -> SemaphorePtr {
        let sem = Box::new(match kind {
            SemaphoreKind::Counting { max, initial } => Semaphore::new_counting(initial, max),
            SemaphoreKind::Mutex => Semaphore::new_mutex(false),
            SemaphoreKind::RecursiveMutex => Semaphore::new_mutex(true),
        });
        core::ptr::NonNull::from(Box::leak(sem)).cast()
    }

    unsafe fn delete(semaphore: SemaphorePtr) {
        let sem = unsafe { Box::from_raw(semaphore.cast::<Semaphore>().as_ptr()) };
        core::mem::drop(sem);
    }

    unsafe fn take(semaphore: SemaphorePtr, timeout_us: Option<u32>) -> bool {
        let semaphore = unsafe { Semaphore::from_ptr(semaphore) };
        semaphore.take(timeout_us)
    }

    unsafe fn give(semaphore: SemaphorePtr) -> bool {
        let semaphore = unsafe { Semaphore::from_ptr(semaphore) };
        semaphore.give()
    }

    unsafe fn current_count(semaphore: SemaphorePtr) -> u32 {
        let semaphore = unsafe { Semaphore::from_ptr(semaphore) };
        semaphore.current_count()
    }

    unsafe fn try_take(semaphore: SemaphorePtr) -> bool {
        let semaphore = unsafe { Semaphore::from_ptr(semaphore) };
        semaphore.try_take()
    }

    unsafe fn try_give_from_isr(semaphore: SemaphorePtr, hptw: Option<&mut bool>) -> bool {
        let semaphore = unsafe { Semaphore::from_ptr(semaphore) };
        let ok = semaphore.try_give_from_isr();
        if ok {
            if let Some(flag) = hptw {
                *flag = true;
            }
        }
        ok
    }

    unsafe fn try_take_from_isr(semaphore: SemaphorePtr, hptw: Option<&mut bool>) -> bool {
        let semaphore = unsafe { Semaphore::from_ptr(semaphore) };
        let ok = semaphore.try_take_from_isr();
        if ok {
            if let Some(flag) = hptw {
                *flag = true;
            }
        }
        ok
    }
}

register_semaphore_implementation!(Semaphore);
