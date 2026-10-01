//! First impossible task pointer at the Xtensa context-copy boundary.
//!
//! One record per core lives in RTC slow memory so a timer-group watchdog
//! reset cannot erase the evidence. This module observes the copy only; it
//! does not change task selection or reject a context.

use esp_hal::{ram, system::Cpu};

const MAGIC: u32 = 0x4354_5031; // CTP1
const WORDS: usize = 9;

// The linker maps this section to retained RTC slow memory, outside the
// linked internal-DRAM stack budget. Each core writes only its own slot and
// commits MAGIC last; boot reads before the scheduler starts.
#[unsafe(link_section = ".rtc_slow.persistent")]
static mut FIRST_BAD_COPY: [[u32; WORDS]; 2] = [[0; WORDS]; 2];

#[inline(always)]
pub(crate) fn plausible_thread_pointer(value: u32) -> bool {
    value == 0 || (0x3ff0_0000..0x4000_0000).contains(&value)
}

/// `stage=1`: saved source was already bad before the copy.
/// `stage=2`: source was plausible, but the destination was bad afterward.
#[ram]
pub(crate) unsafe fn record(
    stage: u32,
    current_context: u32,
    next_context: u32,
    saved_tp: u32,
    saved_pc: u32,
    saved_sp: u32,
    frame_before_tp: u32,
    frame_after_tp: u32,
) {
    let core = Cpu::current() as usize;
    let slot = unsafe {
        core::ptr::addr_of_mut!(FIRST_BAD_COPY)
            .cast::<u32>()
            .add(core * WORDS)
    };
    // SAFETY: the calling core owns its slot. A reset during a partial write
    // leaves MAGIC unset, so the next boot ignores the incomplete record.
    unsafe {
        if slot.read_volatile() == MAGIC {
            return;
        }
        slot.add(1).write_volatile(stage);
        slot.add(2).write_volatile(current_context);
        slot.add(3).write_volatile(next_context);
        slot.add(4).write_volatile(saved_tp);
        slot.add(5).write_volatile(saved_pc);
        slot.add(6).write_volatile(saved_sp);
        slot.add(7).write_volatile(frame_before_tp);
        slot.add(8).write_volatile(frame_after_tp);
        slot.write_volatile(MAGIC);
    }
}

/// Return and clear first-bad-copy records before starting the scheduler.
pub fn take() -> [Option<[u32; WORDS - 1]>; 2] {
    let mut records = [None; 2];
    for (core, record) in records.iter_mut().enumerate() {
        let slot = unsafe {
            core::ptr::addr_of_mut!(FIRST_BAD_COPY)
                .cast::<u32>()
                .add(core * WORDS)
        };
        // SAFETY: boot calls this before either core can write a new record.
        unsafe {
            if slot.read_volatile() == MAGIC {
                let mut words = [0; WORDS - 1];
                for (index, word) in words.iter_mut().enumerate() {
                    *word = slot.add(index + 1).read_volatile();
                }
                *record = Some(words);
            }
            slot.write_volatile(0);
        }
    }
    records
}
