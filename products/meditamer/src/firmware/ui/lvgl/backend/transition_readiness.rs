//! Presentation conditions for an input-admission transition.
//!
//! Pure policy: `core`-only, so the host harness pulls this file unmodified
//! via `#[path]`. Items use `pub` (not `pub(crate)`) for exactly that
//! reason; inside firmware the parent `mod` is private, so firmware
//! visibility is unchanged.

#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub enum InputTransitionReadiness {
    Ready,
    AwaitAmbientContent,
    AwaitClean,
}

impl InputTransitionReadiness {
    pub const fn after_navigation(changed: bool, destination_is_ambient: bool) -> Self {
        if changed && destination_is_ambient {
            Self::AwaitAmbientContent
        } else {
            Self::Ready
        }
    }

    pub const fn after_ambient_content(self, active_is_ambient: bool) -> Self {
        if matches!(self, Self::AwaitAmbientContent) && active_is_ambient {
            Self::AwaitClean
        } else {
            self
        }
    }

    /// Requires a successful Clean presentation for a transition whose final
    /// content is already ready. An ambient destination still waiting for its
    /// first content frame keeps that stronger prerequisite.
    pub const fn require_clean(self) -> Self {
        if matches!(self, Self::Ready) {
            Self::AwaitClean
        } else {
            self
        }
    }

    pub const fn allows_presentation(self, clean: bool) -> bool {
        matches!(self, Self::Ready) || (matches!(self, Self::AwaitClean) && clean)
    }
}

/// Whether a render pass must force the transition presentation boundary:
/// a transition is pending but its boundary frame has not rendered yet.
/// Once rendered, later passes -- including content-less noop refreshes --
/// must not force another full-canvas invalidation; explicit widget
/// invalidations and new frames still render naturally through the normal
/// refresh path.
pub const fn needs_transition_boundary(pending_input_transition: bool, rendered: bool) -> bool {
    pending_input_transition && !rendered
}

/// Next `input_transition_rendered` flag after one refresh pass. A pass
/// that refreshed marks the boundary rendered; a pass that refreshed
/// nothing keeps the previous flag, so an unrelated noop render never
/// clears an already-rendered boundary back to unrendered (which would
/// re-arm the forced full-canvas invalidation on the following cycle).
pub const fn next_transition_rendered(
    pending_input_transition: bool,
    already_rendered: bool,
    refreshed: bool,
) -> bool {
    if !pending_input_transition {
        return already_rendered;
    }
    if refreshed {
        return true;
    }
    already_rendered
}
