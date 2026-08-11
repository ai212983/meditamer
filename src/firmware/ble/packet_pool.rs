use core::cell::{Cell, RefCell};

use ::embassy_sync as embassy_sync_08;
use embassy_sync_08::blocking_mutex::{
    raw::CriticalSectionRawMutex as CriticalSectionRawMutex08, Mutex,
};
use trouble_host::prelude::{Packet, PacketPool};

use super::{PACKET_COUNT, PACKET_MTU};

struct PacketSlot {
    bytes: [u8; PACKET_MTU],
    free: bool,
}

impl PacketSlot {
    const fn new() -> Self {
        Self {
            bytes: [0; PACKET_MTU],
            free: true,
        }
    }
}

static PACKET_SLOTS: Mutex<CriticalSectionRawMutex08, RefCell<[PacketSlot; PACKET_COUNT]>> =
    Mutex::new(RefCell::new([const { PacketSlot::new() }; PACKET_COUNT]));
static POOL_EXHAUSTED: critical_section::Mutex<Cell<u32>> =
    critical_section::Mutex::new(Cell::new(0));

pub(super) struct Phase1PacketPool;

impl PacketPool for Phase1PacketPool {
    type Packet = Phase1Packet;
    const MTU: usize = PACKET_MTU;

    fn allocate() -> Option<Self::Packet> {
        let packet = PACKET_SLOTS.lock(|slots| {
            let mut slots = slots.borrow_mut();
            slots.iter_mut().enumerate().find_map(|(index, slot)| {
                if slot.free {
                    slot.free = false;
                    slot.bytes.fill(0);
                    Some(Phase1Packet {
                        index,
                        bytes: slot.bytes.as_mut_ptr(),
                    })
                } else {
                    None
                }
            })
        });
        if packet.is_none() {
            critical_section::with(|cs| {
                let count = POOL_EXHAUSTED.borrow(cs);
                count.set(count.get().saturating_add(1));
            });
        }
        packet
    }

    fn capacity() -> usize {
        PACKET_COUNT
    }
}

pub(super) struct Phase1Packet {
    index: usize,
    bytes: *mut u8,
}

impl Packet for Phase1Packet {}

impl AsRef<[u8]> for Phase1Packet {
    fn as_ref(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.bytes, PACKET_MTU) }
    }
}

impl AsMut<[u8]> for Phase1Packet {
    fn as_mut(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.bytes, PACKET_MTU) }
    }
}

impl Drop for Phase1Packet {
    fn drop(&mut self) {
        PACKET_SLOTS.lock(|slots| {
            slots.borrow_mut()[self.index].free = true;
        });
    }
}

pub(super) fn pool_exhausted_count() -> u32 {
    critical_section::with(|cs| POOL_EXHAUSTED.borrow(cs).get())
}

pub(super) fn free_packet_count() -> usize {
    PACKET_SLOTS.lock(|slots| slots.borrow().iter().filter(|slot| slot.free).count())
}
