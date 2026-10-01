use super::*;

#[derive(Clone, Copy)]
enum Step {
    Sample { raw_pressed: bool, at_ms: u64 },
    Cancel { at_ms: u64 },
}

struct Case {
    name: &'static str,
    steps: &'static [Step],
    expected: &'static [ButtonEventKind],
}

fn run(case: &Case) -> Vec<ButtonEventKind, 16> {
    let mut recognizer = ButtonRecognizer::new(ButtonId::Key, ButtonTiming::default());
    let mut kinds = Vec::new();
    for step in case.steps {
        let events = match *step {
            Step::Sample { raw_pressed, at_ms } => recognizer.sample(raw_pressed, at_ms),
            Step::Cancel { at_ms } => recognizer.cancel(at_ms),
        };
        assert!(
            events.len() <= MAX_EVENTS_PER_SAMPLE,
            "{} exceeded its inline event capacity",
            case.name
        );
        for event in events {
            kinds.push(event.kind).unwrap();
        }
    }
    kinds
}

#[test]
fn table_driven_button_sequences() {
    let cases = [
        Case {
            name: "bounce",
            steps: &[
                Step::Sample {
                    raw_pressed: true,
                    at_ms: 0,
                },
                Step::Sample {
                    raw_pressed: false,
                    at_ms: 10,
                },
                Step::Sample {
                    raw_pressed: true,
                    at_ms: 20,
                },
                Step::Sample {
                    raw_pressed: true,
                    at_ms: 49,
                },
                Step::Sample {
                    raw_pressed: true,
                    at_ms: 50,
                },
            ],
            expected: &[ButtonEventKind::Pressed],
        },
        Case {
            name: "single click",
            steps: &[
                Step::Sample {
                    raw_pressed: true,
                    at_ms: 0,
                },
                Step::Sample {
                    raw_pressed: true,
                    at_ms: 30,
                },
                Step::Sample {
                    raw_pressed: false,
                    at_ms: 40,
                },
                Step::Sample {
                    raw_pressed: false,
                    at_ms: 70,
                },
                Step::Sample {
                    raw_pressed: false,
                    at_ms: 420,
                },
            ],
            expected: &[
                ButtonEventKind::Pressed,
                ButtonEventKind::Released,
                ButtonEventKind::Click,
            ],
        },
        Case {
            name: "double click",
            steps: &[
                Step::Sample {
                    raw_pressed: true,
                    at_ms: 0,
                },
                Step::Sample {
                    raw_pressed: true,
                    at_ms: 30,
                },
                Step::Sample {
                    raw_pressed: false,
                    at_ms: 40,
                },
                Step::Sample {
                    raw_pressed: false,
                    at_ms: 70,
                },
                Step::Sample {
                    raw_pressed: true,
                    at_ms: 100,
                },
                Step::Sample {
                    raw_pressed: true,
                    at_ms: 130,
                },
                Step::Sample {
                    raw_pressed: false,
                    at_ms: 140,
                },
                Step::Sample {
                    raw_pressed: false,
                    at_ms: 170,
                },
            ],
            expected: &[
                ButtonEventKind::Pressed,
                ButtonEventKind::Released,
                ButtonEventKind::Pressed,
                ButtonEventKind::Released,
                ButtonEventKind::DoubleClick,
            ],
        },
        Case {
            name: "long press",
            steps: &[
                Step::Sample {
                    raw_pressed: true,
                    at_ms: 0,
                },
                Step::Sample {
                    raw_pressed: true,
                    at_ms: 30,
                },
                Step::Sample {
                    raw_pressed: true,
                    at_ms: 730,
                },
                Step::Sample {
                    raw_pressed: false,
                    at_ms: 740,
                },
                Step::Sample {
                    raw_pressed: false,
                    at_ms: 770,
                },
            ],
            expected: &[
                ButtonEventKind::Pressed,
                ButtonEventKind::LongPress,
                ButtonEventKind::Released,
            ],
        },
        Case {
            name: "cancelled",
            steps: &[
                Step::Sample {
                    raw_pressed: true,
                    at_ms: 0,
                },
                Step::Sample {
                    raw_pressed: true,
                    at_ms: 30,
                },
                Step::Cancel { at_ms: 40 },
            ],
            expected: &[ButtonEventKind::Pressed, ButtonEventKind::Cancelled],
        },
    ];

    for case in &cases {
        let actual = run(case);
        assert_eq!(actual.as_slice(), case.expected, "{}", case.name);
    }
}

#[test]
fn output_capacity_covers_every_two_event_transition() {
    let mut recognizer = ButtonRecognizer::new(ButtonId::Key, ButtonTiming::default());

    assert!(recognizer.sample(true, 0).is_empty());
    assert_eq!(recognizer.sample(true, 30).len(), 1);
    assert!(recognizer.sample(false, 40).is_empty());
    assert_eq!(recognizer.sample(false, 70).len(), 1);
    // The new press began before the click deadline but is not confirmed
    // until after it, producing the delayed click and press together.
    assert!(recognizer.sample(true, 390).is_empty());
    assert_eq!(
        recognizer.sample(true, 450).as_slice(),
        &[
            ButtonEvent {
                button: ButtonId::Key,
                kind: ButtonEventKind::Click,
                at_ms: 420,
                held_ms: 40,
            },
            ButtonEvent {
                button: ButtonId::Key,
                kind: ButtonEventKind::Pressed,
                at_ms: 450,
                held_ms: 0,
            },
        ]
    );

    let mut recognizer = ButtonRecognizer::new(ButtonId::Key, ButtonTiming::default());
    assert!(recognizer.sample(true, 0).is_empty());
    assert_eq!(recognizer.sample(true, 30).len(), 1);
    assert!(recognizer.sample(false, 700).is_empty());
    assert_eq!(
        recognizer.sample(false, 730).as_slice(),
        &[
            ButtonEvent {
                button: ButtonId::Key,
                kind: ButtonEventKind::LongPress,
                at_ms: 730,
                held_ms: 700,
            },
            ButtonEvent {
                button: ButtonId::Key,
                kind: ButtonEventKind::Released,
                at_ms: 730,
                held_ms: 700,
            },
        ]
    );
}

#[test]
fn exact_timing_boundaries_are_inclusive() {
    let mut recognizer = ButtonRecognizer::new(ButtonId::Boot, ButtonTiming::default());
    assert!(recognizer.sample(true, 0).is_empty());
    assert_eq!(
        recognizer.sample(true, 30).as_slice(),
        &[ButtonEvent {
            button: ButtonId::Boot,
            kind: ButtonEventKind::Pressed,
            at_ms: 30,
            held_ms: 0,
        }]
    );
    assert_eq!(
        recognizer.sample(true, 730).as_slice(),
        &[ButtonEvent {
            button: ButtonId::Boot,
            kind: ButtonEventKind::LongPress,
            at_ms: 730,
            held_ms: 700,
        }]
    );
}

/// Edge-only consumption: a
/// consumer reading *only* `Pressed`/`Released` -- never `Click`,
/// `DoubleClick`, or `LongPress` -- sees exactly one Pressed-then-Released
/// pair per physical press/release, regardless of how the interaction later
/// resolves. Reuses `table_driven_button_sequences`'s own cases (a bounce
/// that never confirms, a click, a double-click, and a long press) so this
/// checks the same fixtures from the edge-only consumer's point of view.
#[test]
fn edge_only_consumption_never_loses_or_duplicates_a_press_release_pair() {
    fn edges_only(events: ButtonEvents) -> Vec<ButtonEventKind, 16> {
        let mut kinds = Vec::new();
        for event in events {
            if matches!(
                event.kind,
                ButtonEventKind::Pressed | ButtonEventKind::Released
            ) {
                kinds.push(event.kind).unwrap();
            }
        }
        kinds
    }

    // Single click: an edge-only consumer sees Pressed, Released -- and
    // never sees the later Click at all, so it needs no "ignore this"
    // logic; the edges alone are already everything held-button behavior
    // needs.
    let mut recognizer = ButtonRecognizer::new(ButtonId::Key, ButtonTiming::default());
    let mut seen = Vec::<ButtonEventKind, 16>::new();
    for (raw_pressed, at_ms) in [
        (true, 0),
        (true, 30),
        (false, 40),
        (false, 70),
        (false, 420),
    ] {
        seen.extend(edges_only(recognizer.sample(raw_pressed, at_ms)));
    }
    assert_eq!(
        seen.as_slice(),
        &[ButtonEventKind::Pressed, ButtonEventKind::Released]
    );

    // Double-click: two full Pressed/Released pairs reach the edge-only
    // consumer, in order, with the DoubleClick classification itself never
    // appearing -- proving "one interaction does not trigger the same
    // action through both an edge and a derived event" the other direction:
    // a derived-event consumer's classification never collapses what the
    // edge-only consumer already saw as two separate holds.
    let mut recognizer = ButtonRecognizer::new(ButtonId::Key, ButtonTiming::default());
    let mut seen = Vec::<ButtonEventKind, 16>::new();
    for (raw_pressed, at_ms) in [
        (true, 0),
        (true, 30),
        (false, 40),
        (false, 70),
        (true, 100),
        (true, 130),
        (false, 140),
        (false, 170),
    ] {
        seen.extend(edges_only(recognizer.sample(raw_pressed, at_ms)));
    }
    assert_eq!(
        seen.as_slice(),
        &[
            ButtonEventKind::Pressed,
            ButtonEventKind::Released,
            ButtonEventKind::Pressed,
            ButtonEventKind::Released,
        ]
    );

    // Long press: held-button behavior (a consumer that only cares "is it
    // down right now") reads directly off Pressed/Released -- it needs
    // neither to wait for `LongPress` nor to treat it specially, and the
    // eventual Released still arrives exactly once.
    let mut recognizer = ButtonRecognizer::new(ButtonId::Key, ButtonTiming::default());
    let mut seen = Vec::<ButtonEventKind, 16>::new();
    for (raw_pressed, at_ms) in [
        (true, 0),
        (true, 30),
        (true, 730),
        (false, 740),
        (false, 770),
    ] {
        seen.extend(edges_only(recognizer.sample(raw_pressed, at_ms)));
    }
    assert_eq!(
        seen.as_slice(),
        &[ButtonEventKind::Pressed, ButtonEventKind::Released]
    );
}

/// Modal routing and cancellation:
/// losing capture mid-hold cancels the interaction outright -- no
/// `Released`, no `Click` -- and a hold still physically down at that
/// instant stays suppressed across the gap *and* across regaining capture,
/// so it can never be reclassified as a new press. Only an actual physical
/// release ends the suppression; the next press after that is genuinely
/// fresh.
#[test]
fn capture_gate_cancels_on_revoke_and_suppresses_through_regrant_until_release() {
    let mut gate = CaptureGate::new(ButtonId::Key, ButtonTiming::default());

    // A normal press under capture behaves exactly like the bare recognizer.
    assert!(gate.sample(true, 0).is_empty());
    assert_eq!(
        gate.sample(true, 30).as_slice(),
        &[ButtonEvent {
            button: ButtonId::Key,
            kind: ButtonEventKind::Pressed,
            at_ms: 30,
            held_ms: 0,
        }]
    );

    // Capture is revoked mid-hold (a modal was just admitted, say) --
    // cancellation fires immediately, on the revoke itself, not on the next
    // sample. The button is still down at this instant, so suppression
    // arms right here.
    assert_eq!(
        gate.set_captured(false, 50).as_slice(),
        &[ButtonEvent {
            button: ButtonId::Key,
            kind: ButtonEventKind::Cancelled,
            at_ms: 50,
            held_ms: 20,
        }]
    );

    // The button is still physically held, and stays held, entirely outside
    // the capture window -- no Pressed, no Released, no Click reaches this
    // consumer for it. Nothing was silently queued either.
    assert!(gate.sample(true, 60).is_empty());
    assert!(gate.sample(true, 400).is_empty());
    assert!(gate.sample(true, 800).is_empty());

    // Capture returns while the button is still down -- this is exactly the
    // continuing hold, not a new press: suppression persists across the
    // regrant, so it produces nothing at all, no matter how long it's held,
    // until it is physically released. A new owner (here, the same one)
    // cannot inherit it.
    assert!(gate.set_captured(true, 900).is_empty());
    assert!(gate.sample(true, 900).is_empty());
    assert!(gate.sample(true, 930).is_empty());
    assert!(gate.sample(true, 2_000).is_empty());

    // The physical release ends suppression -- still no event of its own,
    // since there was never an accepted press to release.
    assert!(gate.sample(false, 2_030).is_empty());

    // Only now does a fresh physical press produce a genuinely new
    // interaction, subject to the same debounce as any other.
    assert!(gate.sample(true, 2_060).is_empty());
    assert_eq!(
        gate.sample(true, 2_090).as_slice(),
        &[ButtonEvent {
            button: ButtonId::Key,
            kind: ButtonEventKind::Pressed,
            at_ms: 2_090,
            held_ms: 0,
        }]
    );

    // Its release is delivered normally -- the gate does not affect
    // anything once capture is (and stays) granted. Debounce needs two
    // consistent samples to confirm the edge, same as the bare recognizer.
    assert!(gate.sample(false, 2_120).is_empty());
    assert_eq!(
        gate.sample(false, 2_150).as_slice(),
        &[ButtonEvent {
            button: ButtonId::Key,
            kind: ButtonEventKind::Released,
            at_ms: 2_150,
            held_ms: 60,
        }]
    );

    // A no-op toggle (already captured, granting it again) never touches
    // the recognizer at all, regardless of its internal state -- the early
    // return in `set_captured` is unconditional on `captured == captured`.
    assert!(gate.set_captured(true, 2_160).is_empty());

    // The recognizer is still waiting to see whether that completed click
    // becomes a double-click (it stays in that state until either the
    // window elapses or something ends it -- there is no separate timer,
    // only later samples/cancels observe elapsed time) -- so revoking
    // capture *now* correctly cancels that still-open classification rather
    // than leaving it dangling for a capture change with input this could
    // no longer belong to. The button is up at this point, so no
    // suppression arms.
    assert_eq!(
        gate.set_captured(false, 2_160).as_slice(),
        &[ButtonEvent {
            button: ButtonId::Key,
            kind: ButtonEventKind::Cancelled,
            at_ms: 2_160,
            held_ms: 60,
        }]
    );
    // With that settled, revoking again really is a clean no-op.
    assert!(gate.sample(false, 2_160).is_empty());
}

/// A hold still down when suppression would arm from an unconditional
/// [`CaptureGate::cancel`] (the "system is going to sleep" path, not a
/// capture handoff) is suppressed the same way as a capture-loss hold --
/// waking back up mid-hold cannot reclassify it as a new press either.
#[test]
fn capture_gate_unconditional_cancel_suppresses_held_button_too() {
    let mut gate = CaptureGate::new(ButtonId::Key, ButtonTiming::default());

    assert!(gate.sample(true, 0).is_empty());
    assert_eq!(
        gate.sample(true, 30).as_slice(),
        &[ButtonEvent {
            button: ButtonId::Key,
            kind: ButtonEventKind::Pressed,
            at_ms: 30,
            held_ms: 0,
        }]
    );

    // The system suspends while the button is still down.
    assert_eq!(
        gate.cancel(50).as_slice(),
        &[ButtonEvent {
            button: ButtonId::Key,
            kind: ButtonEventKind::Cancelled,
            at_ms: 50,
            held_ms: 20,
        }]
    );

    // It wakes back up with the button still physically held -- no
    // reclassification, even though capture was never revoked.
    assert!(gate.sample(true, 60).is_empty());
    assert!(gate.sample(true, 500).is_empty());

    // Only the physical release clears suppression.
    assert!(gate.sample(false, 530).is_empty());
    assert!(gate.sample(true, 560).is_empty());
    assert_eq!(
        gate.sample(true, 590).as_slice(),
        &[ButtonEvent {
            button: ButtonId::Key,
            kind: ButtonEventKind::Pressed,
            at_ms: 590,
            held_ms: 0,
        }]
    );
}

/// Surface ownership is an exact instance boundary, independent of product
/// role or navigation intent. An interaction accepted by one instance cannot
/// finish as a click or long press on the next instance, even when the two
/// instances represent the same surface.
#[test]
fn capture_gate_cancels_on_surface_instance_change() {
    let owner = provider(1);
    let surface = surface(owner, 1);
    let first_instance = shell::types::SurfaceInstanceToken {
        surface,
        generation: shell::types::InstanceGeneration(1),
    };
    let rebuilt_instance = shell::types::SurfaceInstanceToken {
        surface,
        generation: shell::types::InstanceGeneration(2),
    };
    let third_instance = shell::types::SurfaceInstanceToken {
        surface,
        generation: shell::types::InstanceGeneration(3),
    };
    let mut gate = CaptureGate::new(ButtonId::Key, ButtonTiming::default());

    // Initial ownership is only established; it does not manufacture a
    // cancellation before recognition has begun.
    assert!(gate.set_surface_instance(first_instance, 0).is_empty());
    assert!(gate.sample(true, 0).is_empty());
    assert_eq!(
        gate.sample(true, 30).as_slice(),
        &[ButtonEvent {
            button: ButtonId::Key,
            kind: ButtonEventKind::Pressed,
            at_ms: 30,
            held_ms: 0,
        }]
    );

    // A new surface instance takes ownership while KEY is still held. The
    // previous interaction is cancelled immediately and remains suppressed
    // through release, so it cannot resolve into a click on the new owner.
    assert_eq!(
        gate.set_surface_instance(rebuilt_instance, 40).as_slice(),
        &[ButtonEvent {
            button: ButtonId::Key,
            kind: ButtonEventKind::Cancelled,
            at_ms: 40,
            held_ms: 10,
        }]
    );
    assert!(gate.sample(true, 60).is_empty());
    assert!(gate.sample(false, 90).is_empty());
    assert!(gate.sample(false, 500).is_empty());

    // Repeating the same exact owner is a no-op, and a genuinely fresh press
    // is recognized normally once the inherited hold has been released.
    assert!(gate.set_surface_instance(rebuilt_instance, 500).is_empty());
    assert!(gate.sample(true, 530).is_empty());
    assert_eq!(
        gate.sample(true, 560).as_slice(),
        &[ButtonEvent {
            button: ButtonId::Key,
            kind: ButtonEventKind::Pressed,
            at_ms: 560,
            held_ms: 0,
        }]
    );

    // Ownership also fences a released interaction whose single-click
    // classification is still pending behind the double-click window.
    assert!(gate.sample(false, 590).is_empty());
    assert_eq!(
        gate.sample(false, 620).as_slice(),
        &[ButtonEvent {
            button: ButtonId::Key,
            kind: ButtonEventKind::Released,
            at_ms: 620,
            held_ms: 60,
        }]
    );
    assert_eq!(
        gate.set_surface_instance(third_instance, 630).as_slice(),
        &[ButtonEvent {
            button: ButtonId::Key,
            kind: ButtonEventKind::Cancelled,
            at_ms: 630,
            held_ms: 60,
        }]
    );
    assert!(gate.sample(false, 1_000).is_empty());
}

/// A press that begins entirely outside the capture window -- never seen by
/// the recognizer as a Pressed edge -- must stay suppressed once capture is
/// regranted for as long as it stays physically down, exactly like a hold
/// that started under capture and lost it mid-press.
#[test]
fn capture_gate_suppresses_a_hold_that_begins_while_uncaptured() {
    let mut gate = CaptureGate::new(ButtonId::Key, ButtonTiming::default());
    assert!(gate.set_captured(false, 0).is_empty());

    // The button goes down entirely outside the capture window.
    assert!(gate.sample(true, 10).is_empty());
    assert!(gate.sample(true, 40).is_empty());

    // Capture returns while that press is still physically held -- this must
    // not be reclassified as a fresh press just because the recognizer never
    // saw a Pressed edge for it.
    assert!(gate.set_captured(true, 100).is_empty());
    assert!(gate.sample(true, 100).is_empty());
    assert!(gate.sample(true, 500).is_empty());

    // Only the physical release ends suppression.
    assert!(gate.sample(false, 530).is_empty());
    assert!(gate.sample(true, 560).is_empty());
    assert_eq!(
        gate.sample(true, 590).as_slice(),
        &[ButtonEvent {
            button: ButtonId::Key,
            kind: ButtonEventKind::Pressed,
            at_ms: 590,
            held_ms: 0,
        }]
    );
}

/// A second press (within a `waiting_for_second` double-click window) that
/// crosses the long-press threshold before it is released must emit
/// `LongPress` exactly once and then still deliver the eventual physical
/// `Released` -- the same "long press, then wait for release" shape a lone
/// first press gets, not silence once the threshold fires.
#[test]
fn second_press_long_press_still_emits_its_later_release() {
    let mut recognizer = ButtonRecognizer::new(ButtonId::Key, ButtonTiming::default());

    // First click, released well inside the double-click window.
    assert!(recognizer.sample(true, 0).is_empty());
    assert_eq!(recognizer.sample(true, 30).len(), 1);
    assert!(recognizer.sample(false, 40).is_empty());
    assert_eq!(recognizer.sample(false, 70).len(), 1);

    // Second press starts before the click deadline, so it joins as a
    // candidate double-click rather than resolving the first as a click.
    assert!(recognizer.sample(true, 100).is_empty());
    assert_eq!(recognizer.sample(true, 130).len(), 1);

    // It crosses the long-press threshold before being released.
    assert_eq!(
        recognizer.sample(true, 830).as_slice(),
        &[ButtonEvent {
            button: ButtonId::Key,
            kind: ButtonEventKind::LongPress,
            at_ms: 830,
            held_ms: 700,
        }]
    );

    // No further LongPress fires while it stays down...
    assert!(recognizer.sample(true, 900).is_empty());

    // ...and the eventual physical release is still delivered, exactly once.
    assert!(recognizer.sample(false, 930).is_empty());
    assert_eq!(
        recognizer.sample(false, 960).as_slice(),
        &[ButtonEvent {
            button: ButtonId::Key,
            kind: ButtonEventKind::Released,
            at_ms: 960,
            held_ms: 830,
        }]
    );
}

fn provider(id: u16) -> shell::types::ProviderToken {
    shell::types::ProviderToken {
        id: shell::types::ProviderId(id),
        generation: shell::types::ProviderGeneration(1),
    }
}

fn surface(owner: shell::types::ProviderToken, id: u16) -> SurfaceRef {
    SurfaceRef::new(owner, id)
}

/// Local buttons and touch plan, "finish KEY/BOOT product bindings": every
/// role KEY/BOOT can be pressed on maps to exactly the action Medinote's
/// runtime loop already executed inline, now as one pure, host-tested
/// decision per button instead of match arms embedded in the loop itself.
#[test]
fn key_action_covers_every_role() {
    let base = provider(1);
    let launcher = surface(base, 2);
    let hourglass_owner = provider(2);
    let hourglass = surface(hourglass_owner, 3);
    let pressed = KeyEdges {
        pressed: true,
        ..KeyEdges::default()
    };

    assert_eq!(
        key_action(SurfaceRole::Ambient, pressed, launcher, hourglass),
        KeyAction::Navigate(NavIntent::OpenLauncher(launcher))
    );
    // Hourglass gives KEY a different meaning entirely -- not a "no
    // navigation" absence, but its own explicit binding.
    assert_eq!(
        key_action(SurfaceRole::AppRoot, pressed, launcher, hourglass),
        KeyAction::AppInput
    );
}

/// With more than one launchable entry, the Launcher needs KEY to mean two
/// different things off one physical button: a completed short tap cycles
/// its own selection, and a held-past-threshold press opens whichever entry
/// is currently selected. Neither reacts to the bare press edge alone (that
/// only starts the tap/hold the recognizer is still classifying).
#[test]
fn key_action_on_launcher_distinguishes_click_from_long_press() {
    let base = provider(1);
    let launcher = surface(base, 2);
    let selected = surface(provider(3), 4);

    assert_eq!(
        key_action(
            SurfaceRole::Launcher,
            KeyEdges {
                pressed: true,
                ..KeyEdges::default()
            },
            launcher,
            selected
        ),
        KeyAction::None,
        "the bare press edge alone must not yet cycle or open anything"
    );
    assert_eq!(
        key_action(
            SurfaceRole::Launcher,
            KeyEdges {
                clicked: true,
                ..KeyEdges::default()
            },
            launcher,
            selected
        ),
        KeyAction::CycleLauncherSelection
    );
    assert_eq!(
        key_action(
            SurfaceRole::Launcher,
            KeyEdges {
                long_pressed: true,
                ..KeyEdges::default()
            },
            launcher,
            selected
        ),
        KeyAction::Navigate(NavIntent::Launch(selected))
    );
    // A long press that also carries a stale `clicked` flag from the same
    // resolved tick (should not happen in practice, since the recognizer
    // never emits both for one edge) still prefers opening -- long-press is
    // the more deliberate, harder-to-trigger-by-accident action.
    assert_eq!(
        key_action(
            SurfaceRole::Launcher,
            KeyEdges {
                clicked: true,
                long_pressed: true,
                ..KeyEdges::default()
            },
            launcher,
            selected
        ),
        KeyAction::Navigate(NavIntent::Launch(selected))
    );
}

/// A `Click`/`LongPress` edge is meaningless off the roles that only react
/// to the immediate press -- it must resolve to `None`, not silently reuse
/// whatever `Ambient`/`AppRoot` would have done for a bare `pressed` edge.
#[test]
fn key_action_ignores_click_and_long_press_edges_outside_launcher() {
    let base = provider(1);
    let launcher = surface(base, 2);
    let hourglass = surface(provider(2), 3);
    let completed = KeyEdges {
        clicked: true,
        long_pressed: true,
        ..KeyEdges::default()
    };

    assert_eq!(
        key_action(SurfaceRole::Ambient, completed, launcher, hourglass),
        KeyAction::None
    );
    assert_eq!(
        key_action(SurfaceRole::AppRoot, completed, launcher, hourglass),
        KeyAction::None
    );
}

#[test]
fn boot_action_covers_every_role_and_sleep_configuration() {
    assert_eq!(
        boot_action(SurfaceRole::Launcher, true),
        BootAction::Navigate(NavIntent::Back)
    );
    assert_eq!(
        boot_action(SurfaceRole::AppRoot, true),
        BootAction::Navigate(NavIntent::Back)
    );
    // The sleep-vs-deep-sleep choice on Ambient is itself part of the
    // binding, not a bare `if` at the call site -- both configurations are
    // exercised here, independent of any particular deployment's default.
    assert_eq!(
        boot_action(SurfaceRole::Ambient, true),
        BootAction::DeepSleep
    );
    assert_eq!(boot_action(SurfaceRole::Ambient, false), BootAction::Sleep);
}

/// One small cross-source fixture. Local KEY edges (through `CaptureGate`)
/// and remote BLE edges (through `ble::input::InputPublisher`) each have
/// their own dedicated correctness tests above and in `platform/connectivity/ble`; this
/// one instead checks what happens once both land on a single
/// timestamp-ordered timeline, per the shared input contract's "preserve
/// interaction order across UI ticks and use one target monotonic unit
/// across local and BLE": source order comes from the timestamp, not
/// arrival order, and a stale-generation or suppressed remote report (a
/// disconnect/reconnect spanning a continuing hold) contributes nothing to
/// that timeline at all -- it neither duplicates nor reorders anything.
#[test]
fn cross_source_timeline_preserves_order_without_duplicating_or_reclassifying() {
    use ble::hid::DecodedReport;
    use ble::input::InputPublisher;

    #[derive(Debug, PartialEq, Eq)]
    enum TimelineEvent {
        Local(ButtonEventKind, u64),
        Remote {
            button: u8,
            pressed: bool,
            ticks: u64,
        },
    }

    fn report(buttons: u16) -> DecodedReport {
        DecodedReport {
            buttons,
            hat: None,
            axes: [None; 6],
        }
    }

    let mut timeline: heapless::Vec<TimelineEvent, 16> = heapless::Vec::new();

    // Local KEY press at t=30.
    let mut key = CaptureGate::new(ButtonId::Key, ButtonTiming::default());
    assert!(key.sample(true, 0).is_empty());
    for event in key.sample(true, 30) {
        timeline
            .push(TimelineEvent::Local(event.kind, event.at_ms))
            .unwrap();
    }

    // A remote gamepad reports a press at t=10 -- before the local KEY press
    // above, even though it is decoded and drained second here. Source
    // order must come from the timestamp once merged, not decode order.
    let mut remote = InputPublisher::new();
    remote.publish(0, 10, &report(0b1)).unwrap();
    for edge in remote.drain_edges() {
        timeline
            .push(TimelineEvent::Remote {
                button: edge.button,
                pressed: edge.pressed,
                ticks: edge.ticks,
            })
            .unwrap();
    }

    // The link drops and reconnects. A stale-generation report that arrives
    // right after the drop is rejected outright -- it never reaches the
    // timeline at all, not even as a dropped-but-recorded entry.
    remote.clear_and_reconcile(40);
    assert!(remote.publish(0, 45, &report(0b1)).is_err());

    // The reconnected peer's first live report still shows the same button
    // held from before the drop -- suppressed, not a fresh press, so it
    // contributes nothing either.
    remote.publish(1, 50, &report(0b1)).unwrap();
    assert!(remote.drain_edges().is_empty());

    // A genuinely neutral report ends suppression, itself contributing
    // nothing (there was never an accepted press for it to release)...
    remote.publish(1, 55, &report(0)).unwrap();
    assert!(remote.drain_edges().is_empty());

    // ...and only the real change after that lands on the timeline.
    remote.publish(1, 60, &report(0b10)).unwrap();
    for edge in remote.drain_edges() {
        timeline
            .push(TimelineEvent::Remote {
                button: edge.button,
                pressed: edge.pressed,
                ticks: edge.ticks,
            })
            .unwrap();
    }

    // A second local KEY edge, released at t=90 -- well after both remote
    // edges in wall-clock time, so it belongs at the end once sorted.
    assert!(key.sample(false, 60).is_empty());
    for event in key.sample(false, 90) {
        timeline
            .push(TimelineEvent::Local(event.kind, event.at_ms))
            .unwrap();
    }

    timeline.sort_by_key(|event| match *event {
        TimelineEvent::Local(_, at_ms) => at_ms,
        TimelineEvent::Remote { ticks, .. } => ticks,
    });

    assert_eq!(
        timeline.as_slice(),
        &[
            TimelineEvent::Remote {
                button: 0,
                pressed: true,
                ticks: 10,
            },
            TimelineEvent::Local(ButtonEventKind::Pressed, 30),
            TimelineEvent::Remote {
                button: 1,
                pressed: true,
                ticks: 60,
            },
            TimelineEvent::Local(ButtonEventKind::Released, 90),
        ]
    );
}
