//! Timestamped physical-button recognition for Medinote's Waveshare target.
//!
//! GPIO sampling stays in `boards/waveshare-rlcd42`; this module turns those
//! raw active-low samples into debounced physical events. Each button owns one
//! recognizer, so an unfinished click on `KEY` can never affect `BOOT`.
//!
//! The recognizer lives here so its tests run in this crate's existing host
//! suite. This module has no hardware dependency of its own: raw samples and
//! timestamps arrive as plain values, while the GPIO sampling that feeds it
//! remains in `targets/medinote-waveshare`.

use heapless::Vec;
use shell::types::{NavIntent, SurfaceInstanceToken, SurfaceRef, SurfaceRole};
use statig::{blocking::IntoStateMachineExt as _, prelude::*};

/// The two physical buttons on the Waveshare S3 board.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ButtonId {
    Key,
    Boot,
}

impl ButtonId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Key => "key",
            Self::Boot => "boot",
        }
    }
}

/// A physical interaction recognized from one button's raw GPIO samples.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ButtonEventKind {
    Pressed,
    Released,
    Click,
    DoubleClick,
    LongPress,
    Cancelled,
}

impl ButtonEventKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pressed => "pressed",
            Self::Released => "released",
            Self::Click => "click",
            Self::DoubleClick => "double_click",
            Self::LongPress => "long_press",
            Self::Cancelled => "cancelled",
        }
    }
}

/// A timestamped physical button event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ButtonEvent {
    pub button: ButtonId,
    pub kind: ButtonEventKind,
    pub at_ms: u64,
    pub held_ms: u32,
}

/// Two events are sufficient for every one-sample transition:
///
/// * a delayed click followed by a new press;
/// * a long press followed by its confirmed release; or
/// * a second release followed by its double-click classification.
pub const MAX_EVENTS_PER_SAMPLE: usize = 2;
pub type ButtonEvents = Vec<ButtonEvent, MAX_EVENTS_PER_SAMPLE>;

/// Time thresholds for one [`ButtonRecognizer`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ButtonTiming {
    pub debounce_ms: u64,
    pub long_press_ms: u64,
    pub double_click_ms: u64,
}

impl Default for ButtonTiming {
    fn default() -> Self {
        Self {
            debounce_ms: 30,
            long_press_ms: 700,
            double_click_ms: 350,
        }
    }
}

#[derive(Clone, Copy)]
enum InputEvent {
    Sample { raw_pressed: bool, now_ms: u64 },
    Cancel { now_ms: u64 },
}

#[derive(Clone, Copy)]
enum DebouncedEdge {
    Pressed(u64),
    Released(u64),
}

#[derive(Clone, Copy)]
struct Candidate {
    pressed: bool,
    since_ms: u64,
}

#[derive(Default)]
struct DispatchContext {
    events: ButtonEvents,
}

struct ButtonHsm {
    button: ButtonId,
    timing: ButtonTiming,
    stable_pressed: bool,
    candidate: Option<Candidate>,
}

impl ButtonHsm {
    fn edge(&mut self, raw_pressed: bool, now_ms: u64) -> Option<DebouncedEdge> {
        match self.candidate {
            Some(candidate) if candidate.pressed == raw_pressed => {
                if self.stable_pressed != raw_pressed
                    && now_ms.saturating_sub(candidate.since_ms) >= self.timing.debounce_ms
                {
                    self.stable_pressed = raw_pressed;
                    return Some(if raw_pressed {
                        DebouncedEdge::Pressed(now_ms)
                    } else {
                        DebouncedEdge::Released(now_ms)
                    });
                }
            }
            _ => {
                self.candidate = Some(Candidate {
                    pressed: raw_pressed,
                    since_ms: now_ms,
                });
            }
        }
        None
    }

    fn reset_debounce(&mut self) {
        self.stable_pressed = false;
        self.candidate = None;
    }

    fn emit(&self, context: &mut DispatchContext, kind: ButtonEventKind, at_ms: u64, held_ms: u64) {
        context
            .events
            .push(ButtonEvent {
                button: self.button,
                kind,
                at_ms,
                held_ms: held_ms.min(u64::from(u32::MAX)) as u32,
            })
            .expect("button state transition never emits more than two events");
    }

    fn cancel(&mut self, context: &mut DispatchContext, now_ms: u64, held_ms: u64) {
        self.emit(context, ButtonEventKind::Cancelled, now_ms, held_ms);
        self.reset_debounce();
    }
}

#[state_machine(initial = "State::idle()")]
impl ButtonHsm {
    #[state]
    fn idle(&mut self, context: &mut DispatchContext, event: &InputEvent) -> Outcome<State> {
        match *event {
            InputEvent::Sample {
                raw_pressed,
                now_ms,
            } => {
                if let Some(DebouncedEdge::Pressed(at_ms)) = self.edge(raw_pressed, now_ms) {
                    self.emit(context, ButtonEventKind::Pressed, at_ms, 0);
                    Transition(State::pressed(at_ms))
                } else {
                    Handled
                }
            }
            InputEvent::Cancel { .. } => {
                self.reset_debounce();
                Handled
            }
        }
    }

    #[state]
    fn pressed(
        &mut self,
        context: &mut DispatchContext,
        event: &InputEvent,
        pressed_at_ms: &u64,
    ) -> Outcome<State> {
        let pressed_at_ms = *pressed_at_ms;
        match *event {
            InputEvent::Sample {
                raw_pressed,
                now_ms,
            } => {
                let edge = self.edge(raw_pressed, now_ms);
                let held_ms = now_ms.saturating_sub(pressed_at_ms);
                if held_ms >= self.timing.long_press_ms {
                    self.emit(
                        context,
                        ButtonEventKind::LongPress,
                        pressed_at_ms + self.timing.long_press_ms,
                        self.timing.long_press_ms,
                    );
                    if matches!(edge, Some(DebouncedEdge::Released(_))) {
                        self.emit(context, ButtonEventKind::Released, now_ms, held_ms);
                        return Transition(State::idle());
                    }
                    return Transition(State::long_pressed(pressed_at_ms));
                }
                if let Some(DebouncedEdge::Released(released_at_ms)) = edge {
                    self.emit(context, ButtonEventKind::Released, released_at_ms, held_ms);
                    Transition(State::waiting_for_second(
                        pressed_at_ms,
                        released_at_ms,
                        held_ms,
                    ))
                } else {
                    Handled
                }
            }
            InputEvent::Cancel { now_ms } => {
                self.cancel(context, now_ms, now_ms.saturating_sub(pressed_at_ms));
                Transition(State::idle())
            }
        }
    }

    #[state]
    fn long_pressed(
        &mut self,
        context: &mut DispatchContext,
        event: &InputEvent,
        pressed_at_ms: &u64,
    ) -> Outcome<State> {
        let pressed_at_ms = *pressed_at_ms;
        match *event {
            InputEvent::Sample {
                raw_pressed,
                now_ms,
            } => {
                if let Some(DebouncedEdge::Released(released_at_ms)) =
                    self.edge(raw_pressed, now_ms)
                {
                    self.emit(
                        context,
                        ButtonEventKind::Released,
                        released_at_ms,
                        released_at_ms.saturating_sub(pressed_at_ms),
                    );
                    Transition(State::idle())
                } else {
                    Handled
                }
            }
            InputEvent::Cancel { now_ms } => {
                self.cancel(context, now_ms, now_ms.saturating_sub(pressed_at_ms));
                Transition(State::idle())
            }
        }
    }

    #[state]
    fn waiting_for_second(
        &mut self,
        context: &mut DispatchContext,
        event: &InputEvent,
        first_pressed_at_ms: &u64,
        first_released_at_ms: &u64,
        first_held_ms: &u64,
    ) -> Outcome<State> {
        let first_pressed_at_ms = *first_pressed_at_ms;
        let first_released_at_ms = *first_released_at_ms;
        let first_held_ms = *first_held_ms;
        let click_at_ms = first_released_at_ms + self.timing.double_click_ms;
        match *event {
            InputEvent::Sample {
                raw_pressed,
                now_ms,
            } => {
                if let Some(DebouncedEdge::Pressed(pressed_at_ms)) = self.edge(raw_pressed, now_ms)
                {
                    if pressed_at_ms <= click_at_ms {
                        self.emit(context, ButtonEventKind::Pressed, pressed_at_ms, 0);
                        return Transition(State::second_pressed(
                            first_pressed_at_ms,
                            first_released_at_ms,
                            first_held_ms,
                            pressed_at_ms,
                        ));
                    }
                    self.emit(context, ButtonEventKind::Click, click_at_ms, first_held_ms);
                    self.emit(context, ButtonEventKind::Pressed, pressed_at_ms, 0);
                    return Transition(State::pressed(pressed_at_ms));
                }
                if now_ms >= click_at_ms {
                    self.emit(context, ButtonEventKind::Click, click_at_ms, first_held_ms);
                    Transition(State::idle())
                } else {
                    Handled
                }
            }
            InputEvent::Cancel { now_ms } => {
                self.cancel(context, now_ms, first_held_ms);
                Transition(State::idle())
            }
        }
    }

    #[state]
    fn second_pressed(
        &mut self,
        context: &mut DispatchContext,
        event: &InputEvent,
        _first_pressed_at_ms: &u64,
        _first_released_at_ms: &u64,
        _first_held_ms: &u64,
        second_pressed_at_ms: &u64,
    ) -> Outcome<State> {
        let second_pressed_at_ms = *second_pressed_at_ms;
        match *event {
            InputEvent::Sample {
                raw_pressed,
                now_ms,
            } => {
                let edge = self.edge(raw_pressed, now_ms);
                let held_ms = now_ms.saturating_sub(second_pressed_at_ms);
                if held_ms >= self.timing.long_press_ms {
                    // A qualifying second press joins the first interaction.
                    // Once it becomes long, neither interaction receives a
                    // click classification. If this sample also confirms the
                    // release, both events fit within the per-sample bound of
                    // two; otherwise this waits in `long_pressed` for that
                    // release to arrive later, exactly like a lone press does.
                    self.emit(
                        context,
                        ButtonEventKind::LongPress,
                        second_pressed_at_ms + self.timing.long_press_ms,
                        self.timing.long_press_ms,
                    );
                    if matches!(edge, Some(DebouncedEdge::Released(_))) {
                        self.emit(context, ButtonEventKind::Released, now_ms, held_ms);
                        return Transition(State::idle());
                    }
                    return Transition(State::long_pressed(second_pressed_at_ms));
                }
                if let Some(DebouncedEdge::Released(released_at_ms)) = edge {
                    self.emit(context, ButtonEventKind::Released, released_at_ms, held_ms);
                    self.emit(
                        context,
                        ButtonEventKind::DoubleClick,
                        released_at_ms,
                        held_ms,
                    );
                    Transition(State::idle())
                } else {
                    Handled
                }
            }
            InputEvent::Cancel { now_ms } => {
                self.cancel(context, now_ms, now_ms.saturating_sub(second_pressed_at_ms));
                Transition(State::idle())
            }
        }
    }
}

/// Stateless caller-facing wrapper around the Statig interaction machine.
pub struct ButtonRecognizer {
    machine: statig::blocking::StateMachine<ButtonHsm>,
}

impl ButtonRecognizer {
    pub fn new(button: ButtonId, timing: ButtonTiming) -> Self {
        Self {
            machine: ButtonHsm {
                button,
                timing,
                stable_pressed: false,
                candidate: None,
            }
            .state_machine(),
        }
    }

    pub fn sample(&mut self, raw_pressed: bool, now_ms: u64) -> ButtonEvents {
        let mut context = DispatchContext::default();
        self.machine.handle_with_context(
            &InputEvent::Sample {
                raw_pressed,
                now_ms,
            },
            &mut context,
        );
        context.events
    }

    /// Ends an in-flight interaction before its GPIO is released or replaced.
    pub fn cancel(&mut self, now_ms: u64) -> ButtonEvents {
        let mut context = DispatchContext::default();
        self.machine
            .handle_with_context(&InputEvent::Cancel { now_ms }, &mut context);
        context.events
    }
}

/// A small product adapter over one [`ButtonRecognizer`]. The caller binds it
/// to the exact surface instance with [`Self::set_surface_instance`] and
/// supplies modal availability through [`Self::set_captured`]; every consumer
/// sees only [`Self::sample`]'s accepted edges.
pub struct CaptureGate {
    recognizer: ButtonRecognizer,
    captured: bool,
    /// Exact surface instance that owns the current interaction. The first
    /// binding establishes ownership; every later instance change cancels
    /// before the new owner can sample inherited recognition state.
    surface_instance: Option<SurfaceInstanceToken>,
    /// Most recent physical level, retained across capture changes.
    raw_pressed: bool,
    /// Prevents a hold from being inherited across an ownership boundary.
    suppressed_until_release: bool,
}

impl CaptureGate {
    /// Starts captured -- matches every product's own current behavior
    /// before any modal exists to take capture away.
    pub fn new(button: ButtonId, timing: ButtonTiming) -> Self {
        Self {
            recognizer: ButtonRecognizer::new(button, timing),
            captured: true,
            surface_instance: None,
            raw_pressed: false,
            suppressed_until_release: false,
        }
    }

    /// Binds recognition to the exact surface instance that will receive it.
    ///
    /// The initial binding is inert. A later instance change is an input
    /// ownership boundary: any pending hold, click, or double-click is
    /// cancelled, and a physically held button remains suppressed until its
    /// release. Comparing instance tokens, rather than surface roles or refs,
    /// also protects a rebuilt instance of the same surface.
    pub fn set_surface_instance(
        &mut self,
        surface_instance: SurfaceInstanceToken,
        now_ms: u64,
    ) -> ButtonEvents {
        match self.surface_instance {
            None => {
                self.surface_instance = Some(surface_instance);
                ButtonEvents::new()
            }
            Some(current) if current == surface_instance => ButtonEvents::new(),
            Some(_) => {
                self.surface_instance = Some(surface_instance);
                let events = self.recognizer.cancel(now_ms);
                self.arm_suppression_if_held();
                events
            }
        }
    }

    /// Samples one physical level. Uncaptured presses are cancelled and
    /// suppressed until release, including presses that begin while capture
    /// is unavailable.
    pub fn sample(&mut self, raw_pressed: bool, now_ms: u64) -> ButtonEvents {
        self.raw_pressed = raw_pressed;
        if self.suppressed_until_release {
            if !raw_pressed {
                self.suppressed_until_release = false;
            }
            return ButtonEvents::new();
        }
        if self.captured {
            self.recognizer.sample(raw_pressed, now_ms)
        } else {
            let events = self.recognizer.cancel(now_ms);
            self.arm_suppression_if_held();
            events
        }
    }

    /// Arms suppression if the button is physically down right now,
    /// per the most recent [`Self::sample`]. Shared by every path that
    /// cancels a held interaction, so a continuing hold is suppressed the
    /// same way regardless of what triggered the cancellation.
    fn arm_suppression_if_held(&mut self) {
        if self.raw_pressed {
            self.suppressed_until_release = true;
        }
    }

    /// Grants or revokes capture. Revocation cancels immediately; a held
    /// button remains suppressed across regrant until physical release.
    pub fn set_captured(&mut self, captured: bool, now_ms: u64) -> ButtonEvents {
        if self.captured == captured {
            return ButtonEvents::new();
        }
        self.captured = captured;
        if captured {
            ButtonEvents::new()
        } else {
            let events = self.recognizer.cancel(now_ms);
            self.arm_suppression_if_held();
            events
        }
    }

    /// Cancels without changing capture ownership, for system-wide suspend.
    pub fn cancel(&mut self, now_ms: u64) -> ButtonEvents {
        let events = self.recognizer.cancel(now_ms);
        self.arm_suppression_if_held();
        events
    }
}

/// Which of this tick's recognized KEY events matter to [`key_action`],
/// already classified by [`ButtonEventKind`] rather than a raw sample:
/// `Ambient`/`AppRoot` react to the immediate press edge; `Launcher` needs
/// the completed-tap/held-past-threshold distinction (`Click`/`LongPress`)
/// to both cycle its own selection and open one with a single physical
/// button, once more than one app is launchable.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Default)]
pub struct KeyEdges {
    pub pressed: bool,
    pub clicked: bool,
    pub long_pressed: bool,
}

/// What KEY means for the currently active role -- product policy (shared
/// input routing plan: "Products choose actions"), kept as one pure,
/// host-testable function so the decision is separate from actually
/// executing it (a coordinator transition, or Hourglass's own rotation
/// handling, both stateful and target-specific -- `targets/medinote-waveshare`
/// still owns those).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyAction {
    Navigate(NavIntent),
    /// A completed short tap on the Launcher: move its own highlighted
    /// selection to the next launchable entry (wrapping). The target
    /// resolves and applies this against its own selection state --
    /// `key_action` only ever decides *that* the tap means "cycle," not
    /// which entry comes next.
    CycleLauncherSelection,
    /// Hourglass gives KEY a different meaning (rotation input, driven
    /// directly off the raw press edge's timestamp) than every other role,
    /// where it means "open the next surface."
    AppInput,
    /// This tick's edges mean nothing for the currently active role -- e.g.
    /// a `Click`/`LongPress` completing for `Ambient`/`AppRoot`, which only
    /// react to the immediate press edge, or a `Click` arriving for
    /// `Launcher` with no long-press.
    None,
}

/// `_ => unreachable!` for any role outside Medinote's three (Home,
/// Launcher, Hourglass) matches every other product binding in this module.
/// `launch_target` is the Launcher's *currently selected* entry, already
/// resolved by the caller against its own selection index -- this function
/// stays a pure decision over `role`/`edges` and does not itself hold or
/// advance that index.
pub fn key_action(
    role: SurfaceRole,
    edges: KeyEdges,
    launcher: SurfaceRef,
    launch_target: SurfaceRef,
) -> KeyAction {
    match role {
        SurfaceRole::Ambient => {
            if edges.pressed {
                KeyAction::Navigate(NavIntent::OpenLauncher(launcher))
            } else {
                KeyAction::None
            }
        }
        SurfaceRole::Launcher => {
            if edges.long_pressed {
                KeyAction::Navigate(NavIntent::Launch(launch_target))
            } else if edges.clicked {
                KeyAction::CycleLauncherSelection
            } else {
                KeyAction::None
            }
        }
        SurfaceRole::AppRoot => {
            if edges.pressed {
                KeyAction::AppInput
            } else {
                KeyAction::None
            }
        }
        _ => unreachable!("Medinote has only Home, Launcher, and Hourglass"),
    }
}

/// What BOOT means for the currently active role, including the hardware
/// action it drives on Ambient, expressed as its own binding decision rather
/// than an inline check at the call site. `deep_sleep_enabled` is the
/// product's own runtime-power configuration
/// (`targets/medinote-waveshare`'s `BOOT_ENTERS_DEEP_SLEEP`), threaded in
/// rather than read as a free-standing constant, so this function alone
/// answers "what does BOOT do here" for either configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootAction {
    Navigate(NavIntent),
    Sleep,
    DeepSleep,
}

pub fn boot_action(role: SurfaceRole, deep_sleep_enabled: bool) -> BootAction {
    match role {
        SurfaceRole::Ambient => {
            if deep_sleep_enabled {
                BootAction::DeepSleep
            } else {
                BootAction::Sleep
            }
        }
        SurfaceRole::Launcher | SurfaceRole::AppRoot => BootAction::Navigate(NavIntent::Back),
        _ => unreachable!("Medinote has only Home, Launcher, and Hourglass"),
    }
}

#[cfg(test)]
#[path = "input/tests.rs"]
mod tests;
