//! Panic breadcrumb in RTC slow memory.
//!
//! The soak hang dies silently with no watchdog to recover it, so the one
//! question a post-mortem boot can answer is: did the previous boot panic
//! (even with a wedged print path) or stall without faulting
//! (deadlock/starvation, which leaves no trace)? This module is the writer
//! side: the esp-backtrace `custom-pre-backtrace` hook stores a magic word
//! plus an FNV-1a of the panic location, using only volatile stores -- no
//! UART, locks, or allocation -- so it works in any panic context. Once that
//! minimal marker is durable, it attempts a bounded backtrace into adjacent
//! RTC words; a damaged stack can prevent the trace without losing the hash.
//! The
//! `.rtc_slow.persistent` section is NOLOAD (never zeroed at boot) and the
//! RTC domain survives the EN-pulse resets the soak harness performs; only
//! a true power loss clears it.
//!
//! Shared by every binary in this package through `#[path]` (each binary
//! links esp-backtrace separately, so each needs its own copy of the
//! hook symbol). The read side is `take_crumb`, called by the app binary
//! at boot; probe/updater binaries link the writer only.

/// Magic word ("PANI" in little-endian). A random power-on pattern matches
/// it 1 in 4G and reports one false hit, then is cleared like any read.
pub(crate) const MAGIC: u32 = 0x5041_4E49;
/// Offset slot base, in words (1 KiB into the 8 KiB RTC slow region). The
/// IDF 2nd-stage bootloader is suspected of clobbering word 0 on every
/// boot (a real panic followed by hit=0 was observed), so the offset slot
/// is authoritative and the base slot is kept as diagnostic evidence
/// for/against that hypothesis.
pub(crate) const OFFSET_WORDS: usize = 256;
const FRAME_WORDS: usize = 10;

#[link_section = ".rtc_slow.persistent"]
static mut CRUMB: [u32; OFFSET_WORDS + 4 + FRAME_WORDS] = [0; OFFSET_WORDS + 4 + FRAME_WORDS];

fn crumb_ptr() -> *mut u32 {
    core::ptr::addr_of_mut!(CRUMB) as *mut u32
}

fn crumb_offset_ptr(base: *mut u32) -> *mut u32 {
    // SAFETY: module-owned static, offset inside its bounds.
    unsafe { base.add(OFFSET_WORDS) }
}

/// FNV-1a over file bytes + line number: core-only, safe in any context.
fn location_hash(info: &core::panic::PanicInfo) -> u32 {
    let mut hash: u32 = 0x811C_9DC5;
    let mut mix = |byte: u8| {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(0x0100_0193);
    };
    if let Some(loc) = info.location() {
        for byte in loc.file().as_bytes() {
            mix(*byte);
        }
        for byte in loc.line().to_le_bytes() {
            mix(byte);
        }
    }
    hash
}

fn write_slot(slot: *mut u32, hash: u32) {
    // SAFETY: module-owned NOLOAD words; see `write_crumb`.
    unsafe {
        slot.write_volatile(MAGIC);
        slot.add(1).write_volatile(hash);
        slot.add(2).write_volatile(0);
        slot.add(3).write_volatile(0);
    }
}

pub(crate) fn write_crumb(hash: u32) {
    let base = crumb_ptr();
    // SAFETY: single writer -- the hook runs once per panic on the faulting
    // core -- and these NOLOAD words are owned by this module.
    // First write wins: a secondary panic (e.g. a lock-wedge detector
    // firing after the original fault) must not overwrite the first
    // location hash.
    unsafe {
        if base.read_volatile() != MAGIC {
            write_slot(base, hash);
        }
        let offset = crumb_offset_ptr(base);
        if offset.read_volatile() != MAGIC {
            write_slot(offset, hash);
        }
    }
}

fn write_frames(frames: &[esp_backtrace::BacktraceFrame]) {
    let offset = crumb_offset_ptr(crumb_ptr());
    for (index, frame) in frames.iter().take(FRAME_WORDS).enumerate() {
        // SAFETY: indices 4..14 are inside CRUMB. The hash and magic were
        // already committed above; a secondary panic may replace frame words,
        // so the hash remains the authoritative first-fault marker.
        unsafe {
            offset
                .add(4 + index)
                .write_volatile(frame.program_counter() as u32);
        }
    }
}

/// Reads + clears both breadcrumb slots. The returned tuple is
/// `(offset_hit, base_word0_set, frames)`: the offset slot is authoritative;
/// a set base word with a clear offset word evidences bootloader
/// clobbering of word 0 rather than a genuine miss. Only the app binary
/// calls this; the rest link the writer for the hook symbol, hence the
/// allowance.
#[allow(dead_code)]
pub(crate) fn take_crumb() -> (Option<u32>, bool, [u32; FRAME_WORDS]) {
    let base = crumb_ptr();
    // SAFETY: early boot on one core; no panic handler can be running yet.
    unsafe {
        let offset = crumb_offset_ptr(base);
        let offset_words = [offset.read_volatile(), offset.add(1).read_volatile()];
        let mut frames = [0; FRAME_WORDS];
        for (index, frame) in frames.iter_mut().enumerate() {
            *frame = offset.add(4 + index).read_volatile();
            offset.add(4 + index).write_volatile(0);
        }
        let base_set = base.read_volatile() == MAGIC;
        offset.write_volatile(0);
        base.write_volatile(0);
        (
            (offset_words[0] == MAGIC).then_some(offset_words[1]),
            base_set,
            frames,
        )
    }
}

/// esp-backtrace `custom-pre-backtrace` hook: runs before the panic banner
/// prints and before the final spin.
#[no_mangle]
pub extern "Rust" fn custom_pre_backtrace(info: &core::panic::PanicInfo) {
    write_crumb(location_hash(info));
    write_frames(esp_backtrace::Backtrace::capture().frames());
}
