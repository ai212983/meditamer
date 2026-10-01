#[path = "../../../../products/meditamer/src/firmware/ui/asset_bundles.rs"]
mod asset_bundles;

use phase_arena::{ArenaId, BundleAdmissionError, PhaseArena};
use residency_policy::BundleError;

#[test]
fn clock_payload_is_exact_and_releases_for_reentry() {
    let bundle = &asset_bundles::CLOCK_FIXED;
    let required = bundle.layout(4).unwrap().bytes;
    assert_eq!(bundle.parts.len(), 12);
    assert_eq!(
        bundle.parts[..9].iter().map(|p| p.bytes).sum::<usize>(),
        clock_assets::PAYLOAD_LEN
    );
    assert_eq!(
        bundle.parts.iter().map(|p| p.bytes).sum::<usize>(),
        2_949_804
    );
    assert!(required >= 2_949_804); // alignment padding is structural, not payload

    let mut arena = PhaseArena::new(ArenaId(1), required, 4).unwrap();
    for _ in 0..3 {
        let staged = arena.reserve_bundle::<12>(bundle).unwrap();
        assert_eq!(staged.spans()[0].len, clock_assets::MAP_LENGTHS[0]);
        let token = staged.commit();
        assert_eq!(arena.requested(), required);
        arena.release(token).unwrap();
        assert_eq!(arena.requested(), 0);
    }
    assert_eq!(arena.high_water(), required);
}

#[test]
fn abandoned_bundle_rolls_back_before_next_activation() {
    let bundle = &asset_bundles::CLOCK_FIXED;
    let required = bundle.layout(4).unwrap().bytes;
    let mut arena = PhaseArena::new(ArenaId(4), required, 4).unwrap();
    let first_spans = {
        let staged = arena.reserve_bundle::<12>(bundle).unwrap();
        *staged.spans()
    };
    assert_eq!(arena.requested(), 0);
    let staged = arena.reserve_bundle::<12>(bundle).unwrap();
    assert_eq!(*staged.spans(), first_spans);
    let token = staged.commit();
    arena.release(token).unwrap();
}

#[test]
fn whole_bundle_rejects_one_byte_short_without_partial_state() {
    let bundle = &asset_bundles::CLOCK_FIXED;
    let required = bundle.layout(4).unwrap().bytes;
    let mut arena = PhaseArena::new(ArenaId(1), required - 1, 4).unwrap();
    assert!(matches!(
        arena.reserve_bundle::<12>(bundle),
        Err(BundleAdmissionError::InsufficientCapacity { required: need, available })
            if need == required && available == required - 1
    ));
    assert_eq!(arena.requested(), 0);
    assert_eq!(arena.high_water(), 0);
}

#[test]
fn ambient_core_and_optional_overlay_are_distinct() {
    let core = asset_bundles::AMBIENT_CORE.layout(4).unwrap().bytes;
    let with_mountain = asset_bundles::AMBIENT_WITH_MOUNTAIN
        .layout(4)
        .unwrap()
        .bytes;
    assert_eq!(core, 45_000);
    assert_eq!(with_mountain, 85_350);
    let mut arena = PhaseArena::new(ArenaId(2), with_mountain, 4).unwrap();
    let token = arena
        .reserve_bundle::<1>(&asset_bundles::AMBIENT_CORE)
        .unwrap()
        .commit();
    assert!(matches!(
        arena.reserve_bundle::<2>(&asset_bundles::AMBIENT_WITH_MOUNTAIN),
        Err(BundleAdmissionError::Arena(phase_arena::ArenaError::Busy))
    ));
    arena.release(token).unwrap();
    let token = arena
        .reserve_bundle::<2>(&asset_bundles::AMBIENT_WITH_MOUNTAIN)
        .unwrap()
        .commit();
    arena.release(token).unwrap();
}

#[test]
fn invalid_declaration_does_not_touch_arena() {
    use residency_policy::{Bundle, BundleId, BundlePart, ResourceId};
    let parts = [BundlePart {
        resource: ResourceId(1),
        bytes: 1,
        align: 8,
    }];
    let bad = Bundle {
        id: BundleId(4),
        version: 1,
        parts: &parts,
    };
    let mut arena = PhaseArena::new(ArenaId(3), 100, 4).unwrap();
    assert!(matches!(
        arena.reserve_bundle::<1>(&bad),
        Err(BundleAdmissionError::InvalidDeclaration(
            BundleError::InvalidAlignment
        ))
    ));
    assert_eq!(arena.requested(), 0);
}
