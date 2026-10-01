//! Bounded, fixed-capacity `trouble_host::PacketPool` implementation.
//!
//! A peripheral role supplies its own pool sized to its MTU and packet
//! capacity. [`impl_bounded_packet_pool`] declares the required static state
//! and connects it to concrete Trouble pool and packet types.
//!
//! `critical_section::Mutex` needs a registered critical-section
//! implementation, which only exists on-target (`esp-hal` provides one).
//! This module therefore builds only for `target_os = "none"`.

use core::cell::{Cell, UnsafeCell};

/// Allocation state for `CAPACITY` fixed `MTU`-byte packet slots.
///
/// Each successful [`Self::allocate`] returns the only lease allowed to
/// access that slot. The lease returns the slot automatically when dropped.
pub struct BoundedPacketPoolState<const MTU: usize, const CAPACITY: usize> {
    slots: [UnsafeCell<[u8; MTU]>; CAPACITY],
    claimed: critical_section::Mutex<Cell<[bool; CAPACITY]>>,
    exhausted: critical_section::Mutex<Cell<u32>>,
}

// SAFETY: `claimed` transitions are serialized by a critical section. A slot
// is accessed only through its unique, non-cloneable lease while claimed, and
// pool bookkeeping never creates references to the slot bytes.
unsafe impl<const MTU: usize, const CAPACITY: usize> Sync
    for BoundedPacketPoolState<MTU, CAPACITY>
{
}

impl<const MTU: usize, const CAPACITY: usize> BoundedPacketPoolState<MTU, CAPACITY> {
    pub const fn new() -> Self {
        Self {
            slots: [const { UnsafeCell::new([0; MTU]) }; CAPACITY],
            claimed: critical_section::Mutex::new(Cell::new([false; CAPACITY])),
            exhausted: critical_section::Mutex::new(Cell::new(0)),
        }
    }

    /// Claims and zeroes one free slot.
    pub fn allocate(&self) -> Option<BoundedPacketLease<'_, MTU, CAPACITY>> {
        let index = critical_section::with(|cs| {
            let claimed = self.claimed.borrow(cs);
            let mut snapshot = claimed.get();
            let index = snapshot.iter().position(|slot| !slot)?;
            snapshot[index] = true;
            claimed.set(snapshot);
            Some(index)
        });

        let Some(index) = index else {
            critical_section::with(|cs| {
                let count = self.exhausted.borrow(cs);
                count.set(count.get().saturating_add(1));
            });
            return None;
        };

        let mut lease = BoundedPacketLease { pool: self, index };
        lease.as_mut_slice().fill(0);
        Some(lease)
    }

    fn release(&self, index: usize) {
        critical_section::with(|cs| {
            let claimed = self.claimed.borrow(cs);
            let mut snapshot = claimed.get();
            debug_assert!(snapshot[index]);
            snapshot[index] = false;
            claimed.set(snapshot);
        });
    }

    /// Total allocation attempts that found every slot occupied.
    pub fn exhausted_count(&self) -> u32 {
        critical_section::with(|cs| self.exhausted.borrow(cs).get())
    }

    /// Slots currently free.
    pub fn free_count(&self) -> usize {
        critical_section::with(|cs| {
            self.claimed
                .borrow(cs)
                .get()
                .iter()
                .filter(|claimed| !**claimed)
                .count()
        })
    }
}

impl<const MTU: usize, const CAPACITY: usize> Default for BoundedPacketPoolState<MTU, CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}

/// Exclusive access to one claimed packet slot.
///
/// The slot identity and release operation remain private; dropping the lease
/// returns the slot to its pool exactly once.
pub struct BoundedPacketLease<'pool, const MTU: usize, const CAPACITY: usize> {
    pool: &'pool BoundedPacketPoolState<MTU, CAPACITY>,
    index: usize,
}

impl<const MTU: usize, const CAPACITY: usize> BoundedPacketLease<'_, MTU, CAPACITY> {
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: allocation marks this slot claimed before constructing the
        // lease, and no second lease can name it until this lease is dropped.
        unsafe { &*self.pool.slots[self.index].get() }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: the lease is unique for this slot, and `&mut self` prevents
        // overlapping mutable slices through the lease's safe API.
        unsafe { &mut *self.pool.slots[self.index].get() }
    }
}

impl<const MTU: usize, const CAPACITY: usize> AsRef<[u8]>
    for BoundedPacketLease<'_, MTU, CAPACITY>
{
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl<const MTU: usize, const CAPACITY: usize> AsMut<[u8]>
    for BoundedPacketLease<'_, MTU, CAPACITY>
{
    fn as_mut(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

impl<const MTU: usize, const CAPACITY: usize> Drop for BoundedPacketLease<'_, MTU, CAPACITY> {
    fn drop(&mut self) {
        self.pool.release(self.index);
    }
}

/// Declares a concrete packet pool and matching Trouble packet type.
///
/// ```ignore
/// ble::impl_bounded_packet_pool!(DiagnosticPool, DiagnosticPacket, mtu = 64, capacity = 4);
/// ```
#[macro_export]
macro_rules! impl_bounded_packet_pool {
    ($pool_ty:ident, $packet_ty:ident, mtu = $mtu:expr, capacity = $capacity:expr) => {
        static __BOUNDED_PACKET_POOL_STATE: $crate::packet_pool::BoundedPacketPoolState<
            { $mtu },
            { $capacity },
        > = $crate::packet_pool::BoundedPacketPoolState::new();

        pub struct $pool_ty;

        impl $pool_ty {
            pub fn exhausted_count() -> u32 {
                __BOUNDED_PACKET_POOL_STATE.exhausted_count()
            }

            pub fn free_count() -> usize {
                __BOUNDED_PACKET_POOL_STATE.free_count()
            }
        }

        impl trouble_host::prelude::PacketPool for $pool_ty {
            type Packet = $packet_ty;
            const MTU: usize = $mtu;

            fn allocate() -> Option<Self::Packet> {
                __BOUNDED_PACKET_POOL_STATE
                    .allocate()
                    .map(|lease| $packet_ty { lease })
            }

            fn capacity() -> usize {
                $capacity
            }
        }

        pub struct $packet_ty {
            lease: $crate::packet_pool::BoundedPacketLease<'static, { $mtu }, { $capacity }>,
        }

        impl trouble_host::prelude::Packet for $packet_ty {}

        impl AsRef<[u8]> for $packet_ty {
            fn as_ref(&self) -> &[u8] {
                self.lease.as_ref()
            }
        }

        impl AsMut<[u8]> for $packet_ty {
            fn as_mut(&mut self) -> &mut [u8] {
                self.lease.as_mut()
            }
        }
    };
}
