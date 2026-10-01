//! Fixed-block host core for the transfer cache.
//!
//! [`Pool`] owns `CAP` block slots tracked by a bitmap. A [`Reservation`]
//! stages a request for up to `MAX_REQUEST` blocks through the
//! [`residency_policy::Participant`] contract: [`Participant::prepare`]
//! scans for free slots without mutation, then marks all or none and yields a
//! policy-owned provisional. Dropping the provisional rolls the staged blocks
//! back; committing yields a [`Grant`] of [`BlockHandle`]s. [`Pool::release`]
//! consumes one handle and frees its slot in any order. A release at
//! [`u32::MAX`] retires its slot permanently instead of wrapping its epoch.
//! Dropping a [`Grant`] releases nothing; only [`Pool::release`] frees slots.

#![no_std]

use core::cell::Cell;
use residency_policy::{Participant, Provisional, ResourceHandle, Staged};

/// Failure modes for pool configuration, staging, and release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoolError {
    /// Zero id, zero capacity, capacity above `u16::MAX`, too few bitmap
    /// words, or a zero/above-capacity maximum request.
    InvalidConfig,
    /// Zero requested blocks or more than `MAX_REQUEST`.
    InvalidRequest,
    /// Fewer free slots than requested.
    Exhausted,
    /// A handle names the wrong pool, an out-of-range or retired slot, a
    /// clear bit (double release), or a stale epoch.
    WrongHandle,
}

/// Copyable pool identity. The `0` value is reserved: [`PoolId::new`]
/// rejects it with [`PoolError::InvalidConfig`]. IDs must be unique among
/// simultaneously live pools; a duplicate ID makes a foreign handle look
/// local, which this core cannot detect beyond its own ID check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolId(u32);

impl PoolId {
    /// Name a pool. Fails on the reserved `0` value.
    pub const fn new(raw: u32) -> Result<Self, PoolError> {
        if raw == 0 {
            return Err(PoolError::InvalidConfig);
        }
        Ok(Self(raw))
    }

    /// Raw identity value. Never `0` for a pool built through [`PoolId::new`].
    pub const fn get(self) -> u32 {
        self.0
    }
}

/// Copyable handle to one committed block: owning pool, slot index, and the
/// slot epoch at commit time. Any mismatch at release rejects the handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockHandle {
    pool: PoolId,
    index: usize,
    epoch: u32,
}

impl BlockHandle {
    /// Pool that minted this handle.
    pub const fn pool_id(self) -> PoolId {
        self.pool
    }
    /// Slot index within the pool, always below `CAP`.
    pub const fn index(self) -> usize {
        self.index
    }
    /// Slot epoch at commit time.
    pub const fn epoch(self) -> u32 {
        self.epoch
    }
}

#[cfg(test)]
impl BlockHandle {
    const fn for_test(pool: PoolId, index: usize, epoch: u32) -> Self {
        Self { pool, index, epoch }
    }
}

/// Fixed-block pool with `CAP` slots, `WORDS` bitmap words, and at most
/// `MAX_REQUEST` blocks per reservation. Interior mutability only: the pool
/// is usable through a shared reference and is intentionally not [`Sync`]
/// (its [`Cell`] state must stay on one thread or behind a caller lock).
#[derive(Debug)]
pub struct Pool<const CAP: usize, const WORDS: usize, const MAX_REQUEST: usize> {
    id: PoolId,
    bits: [Cell<u32>; WORDS],
    epochs: [Cell<u32>; CAP],
    retired: [Cell<bool>; CAP],
    current: Cell<usize>,
    high_water: Cell<usize>,
    retired_count: Cell<usize>,
}

#[cfg(test)]
impl<const CAP: usize, const WORDS: usize, const MAX_REQUEST: usize> Pool<CAP, WORDS, MAX_REQUEST> {
    fn word_for_test(&self, word: usize) -> u32 {
        self.bits[word].get()
    }
    fn set_epoch_for_test(&self, index: usize, epoch: u32) {
        self.epochs[index].set(epoch);
    }
}

impl<const CAP: usize, const WORDS: usize, const MAX_REQUEST: usize> Pool<CAP, WORDS, MAX_REQUEST> {
    /// Create an empty pool. Fails with [`PoolError::InvalidConfig`] on
    /// `CAP == 0`, `CAP` above `u16::MAX`, `WORDS * 32 < CAP` (checked math),
    /// or `MAX_REQUEST == 0 || MAX_REQUEST > CAP`.
    pub fn new(id: PoolId) -> Result<Self, PoolError> {
        if CAP == 0 || CAP > u16::MAX as usize {
            return Err(PoolError::InvalidConfig);
        }
        let covered = WORDS.checked_mul(32).ok_or(PoolError::InvalidConfig)?;
        if covered < CAP {
            return Err(PoolError::InvalidConfig);
        }
        if MAX_REQUEST == 0 || MAX_REQUEST > CAP {
            return Err(PoolError::InvalidConfig);
        }
        Ok(Self {
            id,
            bits: core::array::from_fn(|_| Cell::new(0)),
            epochs: core::array::from_fn(|_| Cell::new(0)),
            retired: core::array::from_fn(|_| Cell::new(false)),
            current: Cell::new(0),
            high_water: Cell::new(0),
            retired_count: Cell::new(0),
        })
    }

    /// Pool identity.
    pub const fn id(&self) -> PoolId {
        self.id
    }
    /// Total slots, including retired ones.
    pub const fn capacity(&self) -> usize {
        CAP
    }
    /// Largest single request this pool accepts.
    pub const fn max_request(&self) -> usize {
        MAX_REQUEST
    }
    /// Slots currently marked, staged or committed.
    pub fn current(&self) -> usize {
        self.current.get()
    }
    /// Slots still allocatable: capacity minus marked minus retired.
    pub fn free(&self) -> usize {
        CAP.saturating_sub(self.current.get())
            .saturating_sub(self.retired_count.get())
    }
    /// Largest marked-slot count seen. Monotonic; never reset.
    pub fn high_water(&self) -> usize {
        self.high_water.get()
    }
    /// Slots retired permanently by maximum-epoch releases. Monotonic.
    pub fn retired_slots(&self) -> usize {
        self.retired_count.get()
    }

    /// Configure one request for `blocks` slots. Checks only the request
    /// shape here (`0` or above `MAX_REQUEST` fails); availability is checked
    /// atomically later by [`Participant::prepare`].
    pub fn reserve(
        &self,
        blocks: usize,
    ) -> Result<Reservation<'_, CAP, WORDS, MAX_REQUEST>, PoolError> {
        if blocks == 0 || blocks > MAX_REQUEST {
            return Err(PoolError::InvalidRequest);
        }
        Ok(Reservation { pool: self, blocks })
    }

    /// Free the slot named by `handle`. Any failure leaves the pool
    /// unchanged and reports [`PoolError::WrongHandle`]. A release whose
    /// epoch is [`u32::MAX`] succeeds but retires the slot permanently; the
    /// epoch never wraps. Slots free in any order.
    pub fn release(&self, handle: BlockHandle) -> Result<(), PoolError> {
        if handle.pool != self.id || handle.index >= CAP {
            return Err(PoolError::WrongHandle);
        }
        if self.retired[handle.index].get() || !self.is_marked(handle.index) {
            return Err(PoolError::WrongHandle);
        }
        if self.epochs[handle.index].get() != handle.epoch {
            return Err(PoolError::WrongHandle);
        }
        self.clear_bit(handle.index);
        self.current.set(self.current.get() - 1);
        if handle.epoch == u32::MAX {
            self.retired[handle.index].set(true);
            self.retired_count.set(self.retired_count.get() + 1);
        } else {
            self.epochs[handle.index].set(handle.epoch + 1);
        }
        Ok(())
    }

    fn is_marked(&self, index: usize) -> bool {
        (self.bits[index / 32].get() >> (index % 32)) & 1 == 1
    }
    fn set_bit(&self, index: usize) {
        let cell = &self.bits[index / 32];
        cell.set(cell.get() | (1 << (index % 32)));
    }
    fn clear_bit(&self, index: usize) {
        let cell = &self.bits[index / 32];
        cell.set(cell.get() & !(1 << (index % 32)));
    }
    fn note_marked(&self, count: usize) {
        let current = self.current.get() + count;
        self.current.set(current);
        if current > self.high_water.get() {
            self.high_water.set(current);
        }
    }
}

/// One configured request for a fixed block count. Allocates nothing until
/// [`Participant::prepare`] stages it against the pool.
#[derive(Debug)]
pub struct Reservation<'a, const CAP: usize, const WORDS: usize, const MAX_REQUEST: usize> {
    pool: &'a Pool<CAP, WORDS, MAX_REQUEST>,
    blocks: usize,
}

impl<const CAP: usize, const WORDS: usize, const MAX_REQUEST: usize>
    Reservation<'_, CAP, WORDS, MAX_REQUEST>
{
    /// Requested block count, visible before staging.
    pub const fn requested(&self) -> usize {
        self.blocks
    }
}

impl<'a, const CAP: usize, const WORDS: usize, const MAX_REQUEST: usize> Participant
    for Reservation<'a, CAP, WORDS, MAX_REQUEST>
{
    type Staged = StagedBlocks<'a, CAP, WORDS, MAX_REQUEST>;
    type Error = PoolError;

    /// Scan for free slots without mutation, then mark all or none. Fails
    /// with [`PoolError::Exhausted`] when fewer than the requested blocks are
    /// free, leaving the pool unchanged. Spare bitmap bits past `CAP` are
    /// never collected.
    fn prepare(&mut self, _handle: ResourceHandle) -> Result<Provisional<Self::Staged>, PoolError> {
        let mut slots: [Option<usize>; MAX_REQUEST] = core::array::from_fn(|_| None);
        let mut found = 0;
        let mut index = 0;
        while index < CAP && found < self.blocks {
            if !self.pool.retired[index].get() && !self.pool.is_marked(index) {
                slots[found] = Some(index);
                found += 1;
            }
            index += 1;
        }
        if found < self.blocks {
            return Err(PoolError::Exhausted);
        }
        let mut at = 0;
        while at < self.blocks {
            if let Some(slot) = slots[at] {
                self.pool.set_bit(slot);
            }
            at += 1;
        }
        self.pool.note_marked(self.blocks);
        Ok(Provisional::new(StagedBlocks {
            pool: self.pool,
            slots,
            len: self.blocks,
        }))
    }
}

/// Mechanism-owned staging for one prepared request. Borrows the pool and
/// owns the staged slot list; [`Staged::rollback`] clears exactly those bits
/// and [`Staged::commit`] names them in a [`Grant`].
#[derive(Debug)]
pub struct StagedBlocks<'a, const CAP: usize, const WORDS: usize, const MAX_REQUEST: usize> {
    pool: &'a Pool<CAP, WORDS, MAX_REQUEST>,
    slots: [Option<usize>; MAX_REQUEST],
    len: usize,
}

impl<const CAP: usize, const WORDS: usize, const MAX_REQUEST: usize> Staged
    for StagedBlocks<'_, CAP, WORDS, MAX_REQUEST>
{
    type Output = Grant<MAX_REQUEST>;

    /// Publish the staged blocks, keeping their bits marked. Infallible:
    /// everything fallible stayed in [`Participant::prepare`].
    fn commit(self) -> Grant<MAX_REQUEST> {
        let mut handles: [Option<BlockHandle>; MAX_REQUEST] = core::array::from_fn(|_| None);
        let mut at = 0;
        while at < self.len {
            if let Some(slot) = self.slots[at] {
                handles[at] = Some(BlockHandle {
                    pool: self.pool.id,
                    index: slot,
                    epoch: self.pool.epochs[slot].get(),
                });
            }
            at += 1;
        }
        Grant {
            handles,
            len: self.len,
        }
    }

    /// Discard the staged blocks, clearing exactly their bits.
    fn rollback(self) {
        let mut at = 0;
        while at < self.len {
            if let Some(slot) = self.slots[at] {
                self.pool.clear_bit(slot);
            }
            at += 1;
        }
        self.pool.current.set(self.pool.current.get() - self.len);
    }
}

/// Committed ownership of `len` blocks. Not `Copy` or `Clone`, and dropping
/// a grant releases nothing: only [`Pool::release`] frees slots, in any
/// order, so grants stay usable as heterogeneous coordinator outputs.
#[derive(Debug, PartialEq, Eq)]
pub struct Grant<const MAX_REQUEST: usize> {
    handles: [Option<BlockHandle>; MAX_REQUEST],
    len: usize,
}

impl<const MAX_REQUEST: usize> Grant<MAX_REQUEST> {
    /// Committed block count.
    pub const fn len(&self) -> usize {
        self.len
    }
    /// True only for an empty grant.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Handle at position `at`, or [`None`] past the end.
    pub fn get(&self, at: usize) -> Option<BlockHandle> {
        if at < self.len {
            self.handles[at]
        } else {
            None
        }
    }
    /// Handles in commit order. Bounded by [`Grant::len`].
    pub fn iter(&self) -> impl Iterator<Item = BlockHandle> + '_ {
        self.handles.iter().filter_map(|slot| *slot)
    }
}

#[cfg(test)]
mod tests;
