use super::*;

const ARENA: ArenaId = ArenaId(1);
const OTHER: ArenaId = ArenaId(2);

#[test]
fn invalid_config() {
    assert_eq!(
        PhaseArena::new(ArenaId(0), 16, 8).unwrap_err(),
        ArenaError::InvalidConfig
    );
    assert_eq!(
        PhaseArena::new(ARENA, 0, 8).unwrap_err(),
        ArenaError::InvalidConfig
    );
    assert_eq!(
        PhaseArena::new(ARENA, 16, 0).unwrap_err(),
        ArenaError::InvalidConfig
    );
    assert_eq!(
        PhaseArena::new(ARENA, 16, 3).unwrap_err(),
        ArenaError::InvalidConfig
    );
    assert!(PhaseArena::new(ARENA, 16, 8).is_ok());
}

#[test]
fn invalid_alignment() {
    let mut arena = PhaseArena::new(ARENA, 16, 8).unwrap();
    let mut reservation = arena.reserve(16).unwrap();
    assert_eq!(
        reservation.alloc(1, 0).unwrap_err(),
        ArenaError::InvalidAlignment
    );
    assert_eq!(
        reservation.alloc(1, 3).unwrap_err(),
        ArenaError::InvalidAlignment
    );
    assert_eq!(
        reservation.alloc(1, 16).unwrap_err(),
        ArenaError::InvalidAlignment
    );
}

#[test]
fn zero_sizes_rejected() {
    let mut arena = PhaseArena::new(ARENA, 16, 8).unwrap();
    assert_eq!(arena.reserve(0).unwrap_err(), ArenaError::InvalidSize);
    assert_eq!(arena.requested(), 0);
    let mut reservation = arena.reserve(16).unwrap();
    assert_eq!(
        reservation.alloc(0, 1).unwrap_err(),
        ArenaError::InvalidSize
    );
    assert_eq!(
        reservation.alloc(0, 8).unwrap_err(),
        ArenaError::InvalidSize
    );
}

#[test]
fn exact_fit() {
    let mut arena = PhaseArena::new(ARENA, 16, 8).unwrap();
    let token = {
        let mut reservation = arena.reserve(16).unwrap();
        let span = reservation.alloc(16, 1).unwrap();
        assert_eq!(span, Span { offset: 0, len: 16 });
        reservation.commit()
    };
    assert_eq!(arena.requested(), 16);
    arena.release(token).unwrap();
    assert_eq!(arena.requested(), 0);
    assert!(arena.reserve(16).is_ok());
}

#[test]
fn alignment_padding() {
    let mut arena = PhaseArena::new(ARENA, 16, 8).unwrap();
    let mut reservation = arena.reserve(16).unwrap();
    assert_eq!(reservation.alloc(1, 1).unwrap(), Span { offset: 0, len: 1 });
    assert_eq!(reservation.alloc(1, 8).unwrap(), Span { offset: 8, len: 1 });
    assert_eq!(reservation.requested(), 16);
    assert_eq!(reservation.alignment_waste(), 7);
}

#[test]
fn arithmetic_overflow() {
    let mut arena = PhaseArena::new(ARENA, 16, 8).unwrap();
    let mut reservation = arena.reserve(16).unwrap();
    reservation.alloc(4, 1).unwrap();
    assert_eq!(
        reservation.alloc(usize::MAX, 1).unwrap_err(),
        ArenaError::Overflow
    );
}

#[test]
fn reservation_over_capacity() {
    let mut arena = PhaseArena::new(ARENA, 16, 8).unwrap();
    assert_eq!(arena.reserve(17).unwrap_err(), ArenaError::OutOfCapacity);
    assert_eq!(arena.requested(), 0);
}

#[test]
fn second_reserve_excluded_while_committed() {
    let mut arena = PhaseArena::new(ARENA, 16, 8).unwrap();
    let reservation = arena.reserve(8).unwrap();
    let token = reservation.commit();
    assert_eq!(arena.reserve(8).unwrap_err(), ArenaError::Busy);
    arena.release(token).unwrap();
    assert!(arena.reserve(8).is_ok());
}

#[test]
fn drop_rollback_restores_zero_and_identical_offsets() {
    let mut arena = PhaseArena::new(ARENA, 16, 8).unwrap();
    let first = {
        let mut reservation = arena.reserve(10).unwrap();
        let a = reservation.alloc(3, 1).unwrap();
        let b = reservation.alloc(2, 4).unwrap();
        (a, b)
    };
    assert_eq!(arena.requested(), 0);
    let second = {
        let mut reservation = arena.reserve(10).unwrap();
        let a = reservation.alloc(3, 1).unwrap();
        let b = reservation.alloc(2, 4).unwrap();
        (a, b)
    };
    assert_eq!(first, second);
    assert_eq!(first.0, Span { offset: 0, len: 3 });
    assert_eq!(first.1, Span { offset: 4, len: 2 });
}

#[test]
fn commit_then_consuming_release_restores_capacity() {
    let mut arena = PhaseArena::new(ARENA, 8, 8).unwrap();
    let reservation = arena.reserve(8).unwrap();
    let token = reservation.commit();
    assert_eq!(arena.requested(), 8);
    arena.release(token).unwrap();
    assert_eq!(arena.requested(), 0);
    let reservation = arena.reserve(8).unwrap();
    let token = reservation.commit();
    arena.release(token).unwrap();
    assert_eq!(arena.requested(), 0);
}

#[test]
fn wrong_and_stale_tokens_cannot_release() {
    let mut arena = PhaseArena::new(ARENA, 8, 8).unwrap();
    let reservation = arena.reserve(8).unwrap();
    let token = reservation.commit();
    assert_eq!(
        arena
            .release(BackingToken::for_test(ARENA, 8, 999))
            .unwrap_err()
            .error(),
        ArenaError::WrongToken
    );
    assert_eq!(
        arena
            .release(BackingToken::for_test(ARENA, 4, token.generation))
            .unwrap_err()
            .error(),
        ArenaError::WrongToken
    );
    assert_eq!(arena.requested(), 8);
    let stale_generation = token.generation;
    arena.release(token).unwrap();
    let reservation = arena.reserve(8).unwrap();
    let token = reservation.commit();
    assert_eq!(
        arena
            .release(BackingToken::for_test(ARENA, 8, stale_generation))
            .unwrap_err()
            .error(),
        ArenaError::WrongToken
    );
    arena.release(token).unwrap();
    assert_eq!(arena.requested(), 0);
}

#[test]
fn cross_arena_token_rejected_then_released_on_origin() {
    let mut origin = PhaseArena::new(ARENA, 8, 8).unwrap();
    let mut other = PhaseArena::new(OTHER, 8, 8).unwrap();
    let token = origin.reserve(8).unwrap().commit();
    let failure = other.reserve(8).unwrap().commit();
    let failure = origin.release(failure).unwrap_err();
    assert_eq!(failure.error(), ArenaError::WrongToken);
    assert_eq!(origin.requested(), 8);
    other.release(failure.into_token()).unwrap();
    assert_eq!(other.requested(), 0);
    origin.release(token).unwrap();
    assert_eq!(origin.requested(), 0);
}

#[test]
fn failed_release_preserves_token_and_state() {
    let mut arena = PhaseArena::new(ARENA, 8, 8).unwrap();
    let token = arena.reserve(8).unwrap().commit();
    let generation = token.generation;
    let failure = arena
        .release(BackingToken::for_test(ARENA, 8, generation.wrapping_add(1)))
        .unwrap_err();
    assert_eq!(failure.error(), ArenaError::WrongToken);
    assert_eq!(arena.requested(), 8);
    arena.release(failure.into_token()).unwrap_err();
    assert_eq!(arena.requested(), 8);
    arena.release(token).unwrap();
    assert_eq!(arena.requested(), 0);
}

#[test]
fn generation_max_release_succeeds_then_reserve_exhausted() {
    let mut arena = PhaseArena::new(ARENA, 8, 8).unwrap();
    arena.set_generation_for_test(u32::MAX);
    let token = arena.reserve(8).unwrap().commit();
    assert_eq!(token.generation, u32::MAX);
    arena.release(token).unwrap();
    assert_eq!(arena.requested(), 0);
    assert_eq!(
        arena.reserve(8).unwrap_err(),
        ArenaError::GenerationExhausted
    );
    assert_eq!(
        arena.reserve(1).unwrap_err(),
        ArenaError::GenerationExhausted
    );
}

#[test]
fn span_over_reservation_leaves_cursor_and_waste_unchanged() {
    let mut arena = PhaseArena::new(ARENA, 16, 8).unwrap();
    let mut reservation = arena.reserve(8).unwrap();
    reservation.alloc(1, 1).unwrap();
    let waste_before = reservation.alignment_waste();
    assert_eq!(
        reservation.alloc(8, 8).unwrap_err(),
        ArenaError::OutOfCapacity
    );
    assert_eq!(reservation.alignment_waste(), waste_before);
    assert_eq!(reservation.alloc(7, 1).unwrap(), Span { offset: 1, len: 7 });
    assert_eq!(reservation.alignment_waste(), waste_before);
}

#[test]
fn partial_use_commit_keeps_full_reservation_accounting() {
    let mut arena = PhaseArena::new(ARENA, 16, 8).unwrap();
    let token = {
        let mut reservation = arena.reserve(16).unwrap();
        reservation.alloc(4, 1).unwrap();
        reservation.commit()
    };
    assert_eq!(arena.requested(), 16);
    arena.release(token).unwrap();
    assert_eq!(arena.requested(), 0);
}

#[test]
fn high_water_monotonic_and_waste_exact() {
    let mut arena = PhaseArena::new(ARENA, 16, 8).unwrap();
    {
        let mut reservation = arena.reserve(4).unwrap();
        reservation.alloc(4, 1).unwrap();
    }
    assert_eq!(arena.high_water(), 4);
    assert_eq!(arena.alignment_waste(), 0);
    {
        let mut reservation = arena.reserve(10).unwrap();
        reservation.alloc(1, 1).unwrap();
        reservation.alloc(1, 8).unwrap();
    }
    assert_eq!(arena.high_water(), 10);
    assert_eq!(arena.alignment_waste(), 7);
    assert_eq!(arena.requested(), 0);
    {
        let _reservation = arena.reserve(4).unwrap();
    }
    assert_eq!(arena.high_water(), 10);
    assert_eq!(arena.alignment_waste(), 7);
}
