//! Real-LVGL coverage for Medinote's checked-adapter screens and overlays.
//!
//! LVGL and the adapter's checked-handle registries are
//! process-global, so a second concurrently-running test would corrupt both.

use core::ffi::CStr;

use render::lvgl_adapter::{
    RuntimeSession, Widget, WidgetCreateError, WidgetKind, WIDGET_REGISTRY_CAPACITY,
};
use shell::types::{
    InstanceGeneration, OverlayBand, OverlayInput, OverlayInstance, OverlayLifetime,
    ProviderGeneration, ProviderId, ProviderToken, SurfaceId, SurfaceInstanceToken, SurfaceRef,
};

use super::overlay::{ble_status, settings};
use super::screen::{counter, home, hourglass, launcher, power};
// `super::screen::hourglass` (the checked-LVGL screen module, imported
// above) shadows the `hourglass` crate name in this file's scope; the
// leading `::` reaches the crate unambiguously.
use ::hourglass::model::HourglassModel;
use ::hourglass::presentation::FrameData;

fn owner() -> ProviderToken {
    ProviderToken {
        id: ProviderId(1),
        generation: ProviderGeneration(1),
    }
}

fn surface(owner: ProviderToken, id: u16) -> SurfaceRef {
    SurfaceRef {
        owner,
        id: SurfaceId(id),
    }
}

fn instance(
    owner: ProviderToken,
    surface_id: u16,
    band: OverlayBand,
    input: OverlayInput,
    lifetime: OverlayLifetime,
) -> OverlayInstance {
    OverlayInstance {
        token: SurfaceInstanceToken {
            surface: surface(owner, surface_id),
            generation: InstanceGeneration(1),
        },
        request_owner: owner,
        band,
        input,
        lifetime,
        rank: 1,
    }
}

#[test]
fn checked_adapter_screens_and_overlays() {
    let session = RuntimeSession::initialize().expect("LVGL runtime session");
    let _display = session.create_display(64, 64).expect("LVGL display");
    let token = session.access_token();
    let owner = owner();

    let home = home::create(&token, c"Medinote", c"Home").expect("home creation");
    assert!(home.set_heading(&token, c"Sleep", c"Awake"));
    assert!(home.set_clock(&token, c"12:34"));
    assert!(home.set_reading(&token, c"21.4 C   47.8 %"));
    assert!(home.set_status(&token, c"ok"));
    assert!(home.set_battery(&token, c"84%  4.05V"));
    home.destroy(&token)
        .map_err(|_| "home cleanup blocked")
        .unwrap();

    let launcher_labels: [&CStr; 2] = [c"Hourglass", c"Counter"];
    let launcher = launcher::create(&token, &launcher_labels).expect("launcher creation");
    assert!(launcher.set_selected(&token, &launcher_labels, 1));
    launcher
        .destroy(&token)
        .map_err(|_| "launcher cleanup blocked")
        .unwrap();

    let mut clock = flipclock::FlipClock::new(0);
    let counter_screen = counter::create(&token, c"0", &clock).expect("counter creation");
    assert!(counter_screen.set_count(&token, c"4"));
    // Repainting the flip canvas must survive the checked-handle contract the
    // rest of this walk exercises; it writes through the same UI token.
    clock.start();
    clock.advance(flipclock::FLIP_DURATION_MS / 3);
    counter_screen.set_flip(&token, &clock);
    counter_screen
        .destroy(&token)
        .map_err(|_| "counter cleanup blocked")
        .unwrap();

    let sleep = power::create_sleep(&token).expect("sleep screen creation");
    sleep
        .destroy(&token)
        .map_err(|_| "sleep cleanup blocked")
        .unwrap();
    let deep_sleep = power::create_deep_sleep(&token).expect("deep sleep screen creation");
    deep_sleep
        .destroy(&token)
        .map_err(|_| "deep sleep cleanup blocked")
        .unwrap();

    // -- Hourglass: construction, one real render from a fresh model,
    // destruction. The canvas buffer stays static across this -- render
    // only ever writes into it, never reallocates it. --
    let glass = hourglass::create(&token).expect("hourglass creation");
    let model = HourglassModel::default();
    glass.render(&token, &FrameData::capture(&model));
    glass
        .destroy(&token)
        .map_err(|_| "hourglass cleanup blocked")
        .unwrap();

    assert!(settings::create(
        &token,
        instance(
            owner,
            10,
            OverlayBand::BaseSystem,
            OverlayInput::Passive,
            OverlayLifetime::Transient,
        ),
    )
    .is_none());
    let panel = settings::create(
        &token,
        instance(
            owner,
            10,
            OverlayBand::BaseSystem,
            OverlayInput::Modal,
            OverlayLifetime::Transient,
        ),
    )
    .expect("settings creation");
    assert!(
        panel.is_hidden(&token),
        "settings must stage hidden until the coordinator commits and shows it"
    );
    panel.show(&token);
    assert!(!panel.is_hidden(&token), "show() must reveal the panel");
    panel.hide(&token);
    assert!(panel.is_hidden(&token), "hide() must re-hide the panel");
    panel
        .destroy(&token)
        .map_err(|_| "settings cleanup blocked")
        .unwrap();

    assert!(ble_status::create(
        &token,
        instance(
            owner,
            11,
            OverlayBand::BaseSystem,
            OverlayInput::Modal,
            OverlayLifetime::Sticky,
        ),
        ble_status::BleStatus::Active,
    )
    .is_none());
    let mut chip = ble_status::create(
        &token,
        instance(
            owner,
            11,
            OverlayBand::BaseSystem,
            OverlayInput::Passive,
            OverlayLifetime::Sticky,
        ),
        ble_status::BleStatus::Active,
    )
    .expect("ble status creation");
    assert!(
        chip.is_hidden(&token),
        "ble status must stage hidden until the coordinator commits and shows it"
    );
    chip.show(&token);
    assert!(!chip.is_hidden(&token), "show() must reveal the chip");
    chip.hide(&token);
    assert!(chip.is_hidden(&token), "hide() must re-hide the chip");
    chip.show(&token);
    assert!(
        !chip.update(&token, ble_status::BleStatus::Active),
        "no-op on an unchanged state"
    );
    assert!(chip.update(&token, ble_status::BleStatus::Stopped));
    chip.destroy(&token)
        .map_err(|_| "ble status cleanup blocked")
        .unwrap();

    // -- Partial construction: exhausting the checked-handle registry
    // mid-build (rather than at the very first widget) proves cleanup
    // walks back through whatever a failed attempt already registered,
    // not just its root. Filling the registry, then freeing exactly
    // enough slots for Launcher's root but not its full 8-widget tree
    // (root + heading + card + two entry labels + cycle/open/back
    // hints), forces the failure partway through `launcher::create`. --
    let scratch_root = Widget::screen(&token).expect("scratch root");
    // Match the adapter capacity so its registry, not this local buffer,
    // is the source of `RegistryFull`.
    let mut filler = heapless::Vec::<Widget, WIDGET_REGISTRY_CAPACITY>::new();
    loop {
        match scratch_root.child(&token, WidgetKind::Container) {
            Ok(child) => {
                if filler.push(child).is_err() {
                    break;
                }
            }
            Err(WidgetCreateError::RegistryFull) => break,
            Err(other) => panic!("unexpected scratch failure: {other:?}"),
        }
    }
    for _ in 0..3 {
        if let Some(child) = filler.pop() {
            child.delete(&token).expect("scratch deletion");
        }
    }
    assert!(
        launcher::create(&token, &launcher_labels).is_none(),
        "construction must fail cleanly when the registry cannot hold the whole tree"
    );
    // Draining the rest of the filler must still succeed -- proving the
    // failed Launcher attempt released everything it had already
    // registered rather than holding onto orphaned entries.
    while let Some(child) = filler.pop() {
        child.delete(&token).expect("scratch deletion");
    }
    scratch_root.delete(&token).expect("scratch root deletion");

    assert!(
        session.memory_snapshot().integrity_ok,
        "the adapter must leave LVGL's allocator consistent"
    );
}
