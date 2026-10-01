use super::*;
use residency_policy::{coordinate, Generation, RequestId, ResourceId};

const ID: PoolId = match PoolId::new(7) {
    Ok(id) => id,
    Err(_) => panic!("bad test pool id"),
};
const OTHER_ID: PoolId = match PoolId::new(9) {
    Ok(id) => id,
    Err(_) => panic!("bad test pool id"),
};

fn handle() -> ResourceHandle {
    ResourceHandle::new(ResourceId(3), RequestId(11), Generation(7))
}

fn take<const CAP: usize, const WORDS: usize, const MAX: usize>(
    pool: &Pool<CAP, WORDS, MAX>,
    blocks: usize,
) -> Grant<MAX> {
    let mut reservation = pool.reserve(blocks).expect("reserve");
    reservation.prepare(handle()).expect("prepare").commit()
}

struct FailStaged;

impl Staged for FailStaged {
    type Output = u8;

    fn commit(self) -> u8 {
        0
    }

    fn rollback(self) {}
}

struct FailRight;

impl Participant for FailRight {
    type Staged = FailStaged;
    type Error = PoolError;

    fn prepare(&mut self, _handle: ResourceHandle) -> Result<Provisional<FailStaged>, PoolError> {
        Err(PoolError::Exhausted)
    }
}

#[test]
fn invalid_configurations() {
    assert_eq!(PoolId::new(0).unwrap_err(), PoolError::InvalidConfig);
    assert!(PoolId::new(1).is_ok());
    assert_eq!(
        Pool::<0, 1, 1>::new(PoolId::new(1).unwrap()).unwrap_err(),
        PoolError::InvalidConfig
    );
    assert_eq!(
        Pool::<8, 0, 1>::new(PoolId::new(1).unwrap()).unwrap_err(),
        PoolError::InvalidConfig
    );
    assert_eq!(
        Pool::<33, 1, 1>::new(PoolId::new(1).unwrap()).unwrap_err(),
        PoolError::InvalidConfig
    );
    assert_eq!(
        Pool::<8, 1, 0>::new(PoolId::new(1).unwrap()).unwrap_err(),
        PoolError::InvalidConfig
    );
    assert_eq!(
        Pool::<8, 1, 9>::new(PoolId::new(1).unwrap()).unwrap_err(),
        PoolError::InvalidConfig
    );
    assert_eq!(
        Pool::<65536, 2048, 1>::new(PoolId::new(1).unwrap()).unwrap_err(),
        PoolError::InvalidConfig
    );
    assert!(Pool::<8, 1, 8>::new(ID).is_ok());
}

#[test]
fn no_phantom_slots_past_cap() {
    let pool = Pool::<33, 2, 8>::new(ID).unwrap();
    let mut stored: [Option<BlockHandle>; 33] = core::array::from_fn(|_| None);
    let mut total = 0;
    while total < 33 {
        let want = core::cmp::min(8, 33 - total);
        for handle in take(&pool, want).iter() {
            stored[total] = Some(handle);
            total += 1;
        }
    }
    assert_eq!(total, 33);
    assert_eq!(pool.current(), 33);
    assert_eq!(pool.free(), 0);
    assert_eq!(pool.word_for_test(0), u32::MAX);
    assert_eq!(pool.word_for_test(1), 1);
    let mut reservation = pool.reserve(1).unwrap();
    assert_eq!(
        reservation.prepare(handle()).err(),
        Some(PoolError::Exhausted)
    );
    for slot in stored.iter().filter_map(|slot| *slot) {
        pool.release(slot).unwrap();
    }
    assert_eq!(pool.current(), 0);
}

#[test]
fn zero_and_over_limit_requests_rejected() {
    let pool = Pool::<8, 1, 4>::new(ID).unwrap();
    assert_eq!(pool.reserve(0).unwrap_err(), PoolError::InvalidRequest);
    assert_eq!(pool.reserve(5).unwrap_err(), PoolError::InvalidRequest);
    assert_eq!(pool.current(), 0);
    assert_eq!(pool.high_water(), 0);
}

#[test]
fn exhausted_prepare_leaves_pool_unchanged_then_exact_fit() {
    let pool = Pool::<8, 1, 8>::new(ID).unwrap();
    let grant = take(&pool, 5);
    assert_eq!(grant.len(), 5);
    let mut reservation = pool.reserve(4).unwrap();
    assert_eq!(
        reservation.prepare(handle()).err(),
        Some(PoolError::Exhausted)
    );
    assert_eq!(pool.current(), 5);
    assert_eq!(pool.free(), 3);
    let rest = take(&pool, 3);
    assert_eq!(rest.len(), 3);
    assert_eq!(pool.current(), 8);
    assert_eq!(pool.free(), 0);
    let mut reservation = pool.reserve(1).unwrap();
    assert_eq!(
        reservation.prepare(handle()).err(),
        Some(PoolError::Exhausted)
    );
}

#[test]
fn dropped_provisional_rolls_back_all_staged_blocks() {
    let pool = Pool::<8, 1, 8>::new(ID).unwrap();
    let first = {
        let mut reservation = pool.reserve(3).unwrap();
        let provisional = reservation.prepare(handle()).unwrap();
        drop(provisional);
        assert_eq!(pool.current(), 0);
        take(&pool, 3)
    };
    let before: [usize; 3] = [
        first.get(0).unwrap().index(),
        first.get(1).unwrap().index(),
        first.get(2).unwrap().index(),
    ];
    assert_eq!(before, [0, 1, 2]);
    for handle in first.iter() {
        pool.release(handle).unwrap();
    }
    assert_eq!(pool.current(), 0);
}

#[test]
fn commit_names_handles_and_keeps_bits() {
    let pool = Pool::<8, 1, 8>::new(ID).unwrap();
    let grant = take(&pool, 3);
    assert_eq!(grant.len(), 3);
    assert!(!grant.is_empty());
    let mut seen = [false; 8];
    for handle in grant.iter() {
        assert_eq!(handle.pool_id(), ID);
        assert_eq!(handle.epoch(), 0);
        assert!(!seen[handle.index()]);
        seen[handle.index()] = true;
    }
    assert!(seen[0] && seen[1] && seen[2]);
    assert_eq!(pool.current(), 3);
    assert_eq!(pool.free(), 5);
    assert_eq!(grant.get(3), None);
    for handle in grant.iter() {
        pool.release(handle).unwrap();
    }
    assert_eq!(pool.current(), 0);
}

#[test]
fn arbitrary_release_order() {
    let pool = Pool::<8, 1, 8>::new(ID).unwrap();
    let grant = take(&pool, 4);
    for at in [2, 0, 3, 1] {
        pool.release(grant.get(at).unwrap()).unwrap();
    }
    assert_eq!(pool.current(), 0);
    assert_eq!(pool.free(), 8);
}

#[test]
fn cross_pool_release_rejected_without_mutation() {
    let left = Pool::<4, 1, 4>::new(ID).unwrap();
    let right = Pool::<4, 1, 4>::new(OTHER_ID).unwrap();
    let grant = take(&left, 2);
    let foreign = grant.get(0).unwrap();
    assert_eq!(right.release(foreign).unwrap_err(), PoolError::WrongHandle);
    assert_eq!(right.current(), 0);
    assert_eq!(left.current(), 2);
    left.release(foreign).unwrap();
    assert_eq!(left.current(), 1);
}

#[test]
fn stale_and_double_release_rejected_without_mutation() {
    let pool = Pool::<4, 1, 4>::new(ID).unwrap();
    let grant = take(&pool, 2);
    let first = grant.get(0).unwrap();
    let second = grant.get(1).unwrap();
    pool.release(first).unwrap();
    assert_eq!(pool.release(first).unwrap_err(), PoolError::WrongHandle);
    assert_eq!(pool.current(), 1);
    let fresh = take(&pool, 1);
    assert_eq!(fresh.get(0).unwrap().index(), 0);
    assert_eq!(fresh.get(0).unwrap().epoch(), 1);
    assert_eq!(pool.release(first).unwrap_err(), PoolError::WrongHandle);
    assert_eq!(pool.current(), 2);
    assert_eq!(pool.word_for_test(0), 0b11);
    pool.release(second).unwrap();
    pool.release(fresh.get(0).unwrap()).unwrap();
    assert_eq!(pool.current(), 0);
}

#[test]
fn max_epoch_release_retires_slot_permanently() {
    let pool = Pool::<4, 1, 4>::new(ID).unwrap();
    let grant = take(&pool, 1);
    assert_eq!(grant.get(0).unwrap().index(), 0);
    pool.set_epoch_for_test(0, u32::MAX);
    let retiring = BlockHandle::for_test(ID, 0, u32::MAX);
    pool.release(retiring).unwrap();
    assert_eq!(pool.current(), 0);
    assert_eq!(pool.retired_slots(), 1);
    assert_eq!(pool.free(), 3);
    let stale = grant.get(0).unwrap();
    assert_eq!(pool.release(stale).unwrap_err(), PoolError::WrongHandle);
    assert_eq!(pool.release(retiring).unwrap_err(), PoolError::WrongHandle);
    let refill = take(&pool, 3);
    let mut issued = [false; 4];
    for handle in refill.iter() {
        issued[handle.index()] = true;
    }
    assert!(!issued[0]);
    assert!(issued[1] && issued[2] && issued[3]);
    assert_eq!(pool.free(), 0);
    let mut reservation = pool.reserve(4).unwrap();
    assert_eq!(
        reservation.prepare(handle()).err(),
        Some(PoolError::Exhausted)
    );
    assert_eq!(pool.high_water(), 3);
}

#[test]
fn high_water_is_monotonic() {
    let pool = Pool::<8, 1, 8>::new(ID).unwrap();
    let grant = take(&pool, 3);
    assert_eq!(pool.high_water(), 3);
    for handle in grant.iter() {
        pool.release(handle).unwrap();
    }
    assert_eq!(pool.current(), 0);
    assert_eq!(pool.high_water(), 3);
    let small = take(&pool, 1);
    assert_eq!(pool.current(), 1);
    assert_eq!(pool.high_water(), 3);
    pool.release(small.get(0).unwrap()).unwrap();
}

#[test]
fn coordinate_right_failure_rolls_back_pool_left() {
    let pool = Pool::<4, 1, 4>::new(ID).unwrap();
    let mut left = pool.reserve(2).unwrap();
    let mut right = FailRight;
    assert_eq!(
        coordinate(&mut left, &mut right, handle()).unwrap_err(),
        residency_policy::PairError::Right(PoolError::Exhausted)
    );
    assert_eq!(pool.current(), 0);
    assert_eq!(pool.word_for_test(0), 0);
    let grant = take(&pool, 2);
    assert_eq!(grant.len(), 2);
}
