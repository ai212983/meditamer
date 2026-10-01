use super::*;

#[test]
fn reverse_surface_crossing_is_accounted_and_respects_budget() {
    let mut blocked = PaperCellular::empty_centered(9, 9, TOPPLE_SCALE, 1).unwrap();
    blocked.repose_run_cells = 2;
    blocked.generation = 2;
    blocked.place_sand_local(-2, 1).unwrap();
    blocked.place_sand_local(-3, 1).unwrap();
    blocked.place_sand_local(-4, 1).unwrap();
    let before = blocked;

    let result = blocked.relax_surface(-1, Some(0));
    assert_eq!(result, PaperStep::default());
    assert_eq!(blocked.sand, before.sand);

    let mut allowed = before;
    let result = allowed.relax_surface(-1, Some(1));
    assert_eq!(result.lower_to_upper, 1);
    assert!(!allowed.contains_sand_local(-2, 1));
    assert!(allowed.contains_sand_local(0, 0));
}

#[test]
fn fixed_seed_is_deterministic() {
    let mut first = PaperCellular::paper_reference(500, DEFAULT_TOPPLE_PER_MILLE, 42).unwrap();
    let mut second = first;
    for _ in 0..200 {
        assert_eq!(first.step_down(), second.step_down());
    }
    assert_eq!(first, second);
}

#[test]
fn surface_relaxation_is_deterministic() {
    let mut first = PaperCellular::medinote_unpaced(1_000, DEFAULT_TOPPLE_PER_MILLE, 42).unwrap();
    let mut second = first;
    for _ in 0..300 {
        assert_eq!(first.step_down(), second.step_down());
    }
    assert_eq!(first, second);
}

#[test]
fn probability_sweep_keeps_the_same_mass() {
    for topple in [250, 500, 750, 1_000] {
        let mut model = PaperCellular::paper_reference(500, topple, 9).unwrap();
        for _ in 0..200 {
            model.step_down();
        }
        assert_eq!(model.sand_count(), 500);
    }
}

#[test]
fn one_particle_falls_in_each_rotated_gravity_direction() {
    let cases = [
        (GravityDirection::Down, (-2, -2), (-2, -1)),
        (GravityDirection::Right, (-2, -2), (-1, -2)),
        (GravityDirection::Up, (-2, -1), (-2, -2)),
        (GravityDirection::Left, (-1, -2), (-2, -2)),
    ];
    for (direction, source, destination) in cases {
        let mut model = PaperCellular::empty_centered(5, 5, TOPPLE_SCALE, 1).unwrap();
        model.place_sand_local(source.0, source.1).unwrap();
        let step = model.step(direction, None);
        assert!(
            step.changed,
            "{direction:?} should move its unsupported grain"
        );
        assert!(!model.contains_sand_local(source.0, source.1));
        assert!(model.contains_sand_local(destination.0, destination.1));
    }
}

#[test]
fn throat_budget_rejects_a_partial_two_particle_transition() {
    let mut blocked = PaperCellular::empty_centered(5, 5, TOPPLE_SCALE, 1).unwrap();
    blocked.place_sand_local(0, 0).unwrap();
    blocked.place_sand_local(1, 0).unwrap();
    let before = blocked;
    let result = blocked.step(GravityDirection::Down, Some(1));
    assert_eq!(result.upper_to_lower, 0);
    assert_eq!(blocked.sand, before.sand);

    let mut allowed = before;
    let result = allowed.step(GravityDirection::Down, Some(2));
    assert_eq!(result.upper_to_lower, 2);
    assert!(allowed.contains_sand_local(0, 1));
    assert!(allowed.contains_sand_local(1, 1));
}
