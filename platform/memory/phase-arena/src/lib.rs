//! Caller-owned single-activation phase arena.
//!
//! [`PhaseArena`] owns a byte capacity and hands out exactly one activation at
//! a time: either a staged [`Reservation`] or one committed allocation named
//! by a [`BackingToken`]. Allocation returns caller-relative [`Span`] offsets;
//! the offset core owns no backing bytes. The sibling [`BundleBacking`] path
//! can acquire real, individually aligned parts from a caller-supplied
//! allocator before publishing the activation.
//! Dropping an uncommitted reservation rolls its staging back,
//! [`Reservation::commit`] publishes it, and [`PhaseArena::release`] consumes
//! the token to restore full capacity. Accounting stays reservation-based:
//! [`PhaseArena::requested`] reports the reserved bytes even when the
//! reservation commits with an unused tail. All arithmetic is checked.

#![no_std]

use residency_policy::{Bundle, BundleError};

mod backing;
mod runtime;

pub use backing::{
    prepare_backed_bundle, BackedBundle, BackingError, BundleBacking, PrepareBackingError,
    PreparedBacking,
};
pub use runtime::{
    AccessError, PartRequest, PartShape, RuntimeBundle, RuntimeBundleError, RuntimeView,
};

/// Failure to stage a complete named bundle. No arena activation survives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BundleAdmissionError {
    InvalidDeclaration(BundleError),
    WrongPartCount,
    InsufficientCapacity { required: usize, available: usize },
    Arena(ArenaError),
}

/// One complete, uncommitted bundle and its ordered spans. Dropping it rolls
/// back the arena reservation; committing publishes exactly one activation.
pub struct BundleReservation<'a, const N: usize> {
    reservation: Reservation<'a>,
    spans: [Span; N],
}

impl<const N: usize> BundleReservation<'_, N> {
    pub const fn spans(&self) -> &[Span; N] {
        &self.spans
    }

    pub fn commit(self) -> BackingToken {
        self.reservation.commit()
    }

    fn commit_in_place(&mut self) -> BackingToken {
        self.reservation.commit_in_place()
    }

    fn release_committed(&mut self, token: BackingToken) -> Result<(), ReleaseFailure> {
        self.reservation.arena.release(token)
    }
}

/// Failure modes for arena configuration, staging, and release.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArenaError {
    /// Zero id, zero capacity, or a zero/non-power-of-two maximum alignment.
    InvalidConfig,
    /// Zero, non-power-of-two, or above-maximum allocation alignment.
    InvalidAlignment,
    /// Zero reservation or allocation size.
    InvalidSize,
    /// A reservation was requested while an activation was already live, or
    /// staging vanished under a live reservation.
    Busy,
    /// A reservation exceeds capacity, or a span exceeds its reservation.
    OutOfCapacity,
    /// A checked offset or counter operation overflowed.
    Overflow,
    /// A token names no live committed allocation (wrong arena, wrong or
    /// stale token).
    WrongToken,
    /// The arena retired after a release at the maximum generation.
    GenerationExhausted,
}

/// Copyable arena identity. The `0` value is reserved: [`PhaseArena::new`]
/// rejects it with [`ArenaError::InvalidConfig`]. IDs must be unique among
/// simultaneously live arenas; duplicate IDs are a caller configuration error
/// not detectable by this local core, which validates only its own token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArenaId(pub u32);

/// Caller-relative byte range inside the arena capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    /// Byte offset from the start of the arena capacity.
    pub offset: usize,
    /// Requested byte length (excludes alignment padding before it).
    pub len: usize,
}

/// Proof of one committed activation. Non-`Copy` with private fields: only
/// [`Reservation::commit`] mints a live token, so a wrong or stale token can
/// only be forged through the `cfg(test)` helper below.
#[derive(Debug, PartialEq, Eq)]
pub struct BackingToken {
    arena: ArenaId,
    reserved: usize,
    generation: u32,
}

impl BackingToken {
    /// Arena that minted this token.
    pub const fn arena_id(&self) -> ArenaId {
        self.arena
    }
}

#[cfg(test)]
impl BackingToken {
    fn for_test(arena: ArenaId, reserved: usize, generation: u32) -> Self {
        Self {
            arena,
            reserved,
            generation,
        }
    }
}

/// Failed [`PhaseArena::release`]. Returns the original token so the caller
/// can retry against the correct arena; a failed release changes nothing.
#[derive(Debug)]
pub struct ReleaseFailure {
    error: ArenaError,
    token: BackingToken,
}

impl ReleaseFailure {
    /// Why the release failed.
    pub const fn error(&self) -> ArenaError {
        self.error
    }
    /// Borrow the preserved token.
    pub const fn token(&self) -> &BackingToken {
        &self.token
    }
    /// Recover the preserved token for a retry.
    pub fn into_token(self) -> BackingToken {
        self.token
    }
}

fn fail(error: ArenaError, token: BackingToken) -> ReleaseFailure {
    ReleaseFailure { error, token }
}

#[derive(Debug)]
struct Staged {
    cursor: usize,
}

#[derive(Debug)]
struct Committed {
    reserved: usize,
    generation: u32,
}

/// Caller-owned arena for exactly one staged or committed activation.
#[derive(Debug)]
pub struct PhaseArena {
    id: ArenaId,
    capacity: usize,
    max_align: usize,
    current: usize,
    high_water: usize,
    waste: usize,
    generation: u32,
    retired: bool,
    staged: Option<Staged>,
    committed: Option<Committed>,
}

#[cfg(test)]
impl PhaseArena {
    fn set_generation_for_test(&mut self, generation: u32) {
        self.generation = generation;
    }
}

impl PhaseArena {
    /// Create an idle arena. Fails on a zero id, zero capacity, or a zero or
    /// non-power-of-two maximum alignment.
    pub const fn new(id: ArenaId, capacity: usize, max_align: usize) -> Result<Self, ArenaError> {
        if id.0 == 0 || capacity == 0 || max_align == 0 || !max_align.is_power_of_two() {
            return Err(ArenaError::InvalidConfig);
        }
        Ok(Self {
            id,
            capacity,
            max_align,
            current: 0,
            high_water: 0,
            waste: 0,
            generation: 0,
            retired: false,
            staged: None,
            committed: None,
        })
    }

    /// Arena identity.
    pub const fn id(&self) -> ArenaId {
        self.id
    }

    /// Total byte capacity.
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Maximum accepted allocation alignment.
    pub const fn max_align(&self) -> usize {
        self.max_align
    }

    /// Stage all ordered parts of a named bundle against this arena's
    /// structural capacity. The caller must provide the actual backing bytes;
    /// this offset-only core does not reserve the shared PSRAM heap.
    pub fn reserve_bundle<const N: usize>(
        &mut self,
        bundle: &Bundle<'_>,
    ) -> Result<BundleReservation<'_, N>, BundleAdmissionError> {
        if bundle.parts.len() != N {
            return Err(BundleAdmissionError::WrongPartCount);
        }
        let layout = bundle
            .layout(self.max_align)
            .map_err(BundleAdmissionError::InvalidDeclaration)?;
        if layout.bytes > self.capacity {
            return Err(BundleAdmissionError::InsufficientCapacity {
                required: layout.bytes,
                available: self.capacity,
            });
        }
        let mut reservation = self
            .reserve(layout.bytes)
            .map_err(BundleAdmissionError::Arena)?;
        let mut spans = [Span { offset: 0, len: 0 }; N];
        for (part, span) in bundle.parts.iter().zip(spans.iter_mut()) {
            *span = reservation
                .alloc(part.bytes, part.align)
                .map_err(BundleAdmissionError::Arena)?;
        }
        Ok(BundleReservation { reservation, spans })
    }

    /// Currently requested bytes: the live reservation when staged or
    /// committed, zero when idle. Reservation-based, not span-based.
    pub const fn requested(&self) -> usize {
        self.current
    }

    /// Largest single reservation seen. Monotonic; never reset.
    pub const fn high_water(&self) -> usize {
        self.high_water
    }

    /// Cumulative alignment padding bytes. Monotonic; never reset, including
    /// padding from reservations later rolled back.
    pub const fn alignment_waste(&self) -> usize {
        self.waste
    }

    /// Stage one reservation of `bytes`. Fails with
    /// [`ArenaError::GenerationExhausted`] once retired, [`ArenaError::Busy`]
    /// while an activation is live, [`ArenaError::InvalidSize`] on zero
    /// bytes, or [`ArenaError::OutOfCapacity`] above capacity.
    pub fn reserve(&mut self, bytes: usize) -> Result<Reservation<'_>, ArenaError> {
        if self.retired {
            return Err(ArenaError::GenerationExhausted);
        }
        if self.staged.is_some() || self.committed.is_some() {
            return Err(ArenaError::Busy);
        }
        if bytes == 0 {
            return Err(ArenaError::InvalidSize);
        }
        if bytes > self.capacity {
            return Err(ArenaError::OutOfCapacity);
        }
        if bytes > self.high_water {
            self.high_water = bytes;
        }
        self.current = bytes;
        self.staged = Some(Staged { cursor: 0 });
        Ok(Reservation {
            arena: self,
            reserved: bytes,
            done: false,
        })
    }

    /// Consume `token` and restore full capacity. Any failure returns a
    /// [`ReleaseFailure`] holding the error and the original token; a failed
    /// release changes nothing. Releasing the correct token at `u32::MAX`
    /// succeeds, clears the activation, and retires the arena so later
    /// [`PhaseArena::reserve`] fails with
    /// [`ArenaError::GenerationExhausted`]; the generation never wraps.
    pub fn release(&mut self, token: BackingToken) -> Result<(), ReleaseFailure> {
        if token.arena != self.id {
            return Err(fail(ArenaError::WrongToken, token));
        }
        let active = match self.committed.as_ref() {
            Some(active) => active,
            None => return Err(fail(ArenaError::WrongToken, token)),
        };
        if active.reserved != token.reserved || active.generation != token.generation {
            return Err(fail(ArenaError::WrongToken, token));
        }
        let generation = active.generation;
        if generation == u32::MAX {
            self.committed = None;
            self.current = 0;
            self.retired = true;
            return Ok(());
        }
        // generation < u32::MAX here, so generation + 1 cannot overflow.
        self.committed = None;
        self.current = 0;
        self.generation = generation + 1;
        Ok(())
    }
}

/// One staged activation. May allocate several spans within its reserved byte
/// limit. Dropping without [`Reservation::commit`] rolls the staging back and
/// restores zero requested bytes.
#[derive(Debug)]
pub struct Reservation<'a> {
    arena: &'a mut PhaseArena,
    reserved: usize,
    done: bool,
}

impl<'a> Reservation<'a> {
    /// Allocate `len` bytes at `align` within the reserved limit. Padding
    /// counts against the reservation and the arena waste counter but is
    /// excluded from the returned [`Span::len`]. Cursor and waste are
    /// unchanged on failure.
    pub fn alloc(&mut self, len: usize, align: usize) -> Result<Span, ArenaError> {
        if len == 0 {
            return Err(ArenaError::InvalidSize);
        }
        if align == 0 || !align.is_power_of_two() || align > self.arena.max_align {
            return Err(ArenaError::InvalidAlignment);
        }
        let cursor = self
            .arena
            .staged
            .as_ref()
            .map(|s| s.cursor)
            .ok_or(ArenaError::Busy)?;
        let misalign = cursor.checked_rem(align).ok_or(ArenaError::Overflow)?;
        let gap = align.checked_sub(misalign).ok_or(ArenaError::Overflow)?;
        let padding = gap.checked_rem(align).ok_or(ArenaError::Overflow)?;
        let start = cursor.checked_add(padding).ok_or(ArenaError::Overflow)?;
        let end = start.checked_add(len).ok_or(ArenaError::Overflow)?;
        if end > self.reserved {
            return Err(ArenaError::OutOfCapacity);
        }
        let waste = self
            .arena
            .waste
            .checked_add(padding)
            .ok_or(ArenaError::Overflow)?;
        self.arena.staged.as_mut().ok_or(ArenaError::Busy)?.cursor = end;
        self.arena.waste = waste;
        Ok(Span { offset: start, len })
    }

    /// Live reserved bytes, visible while staging. Reservation-based.
    pub const fn requested(&self) -> usize {
        self.arena.current
    }

    /// Cumulative arena alignment padding, visible while staging.
    pub const fn alignment_waste(&self) -> usize {
        self.arena.waste
    }

    /// Publish the reservation, yielding its backing token. Infallible: the
    /// unused reserved tail stays committed and accounting remains
    /// reservation-based.
    pub fn commit(mut self) -> BackingToken {
        self.commit_in_place()
    }

    fn commit_in_place(&mut self) -> BackingToken {
        debug_assert!(!self.done);
        self.done = true;
        let generation = self.arena.generation;
        self.arena.staged = None;
        self.arena.committed = Some(Committed {
            reserved: self.reserved,
            generation,
        });
        BackingToken {
            arena: self.arena.id,
            reserved: self.reserved,
            generation,
        }
    }
}

impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if !self.done {
            self.arena.staged = None;
            self.arena.current = 0;
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod backing_tests;

#[cfg(test)]
mod runtime_tests;
