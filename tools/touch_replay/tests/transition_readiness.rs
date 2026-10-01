#[path = "../../../products/meditamer/src/firmware/ui/lvgl/backend/transition_readiness.rs"]
#[allow(dead_code)] // The host target imports the full policy module for selected tests.
mod transition_readiness;

use transition_readiness::InputTransitionReadiness;

#[test]
fn ambient_transition_waits_for_content_then_clean_presentation() {
    let readiness = InputTransitionReadiness::after_navigation(true, true);
    assert!(!readiness.allows_presentation(true));

    let readiness = readiness.after_ambient_content(true);
    assert!(!readiness.allows_presentation(false));
    assert!(readiness.allows_presentation(true));
}

#[test]
fn fallback_content_and_recovery_keep_the_same_clean_requirement() {
    let readiness =
        InputTransitionReadiness::after_navigation(true, true).after_ambient_content(true);
    assert!(!readiness.allows_presentation(false));
    assert!(readiness.allows_presentation(true));
}

#[test]
fn unchanged_or_nonambient_navigation_needs_only_normal_presentation() {
    assert!(InputTransitionReadiness::after_navigation(false, true).allows_presentation(false));
    assert!(InputTransitionReadiness::after_navigation(true, false).allows_presentation(false));
}

#[test]
fn content_from_a_departed_ambient_surface_does_not_advance_the_transition() {
    let readiness =
        InputTransitionReadiness::after_navigation(true, true).after_ambient_content(false);
    assert!(!readiness.allows_presentation(true));
}
