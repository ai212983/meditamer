#![allow(unsafe_code)]

use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};

use super::*;

#[path = "packed_tests.rs"]
mod packed_tests;

const LVGL_POOL_BYTES: usize = 128 * 1024;

#[repr(align(16))]
struct AlignedPool([u8; LVGL_POOL_BYTES]);

static mut LVGL_POOL: AlignedPool = AlignedPool([0; LVGL_POOL_BYTES]);
static ARC_POINTS: StaticLinePoints<3> = StaticLinePoints::new([
    LinePoint::new(0.0, 0.0),
    LinePoint::new(50.0, 25.0),
    LinePoint::new(100.0, 0.0),
]);
static EMPTY_POINTS: StaticLinePoints<0> = StaticLinePoints::new([]);
static ONE_PIXEL: StaticL8CanvasBuffer<1> = StaticL8CanvasBuffer::new([0xFF]);
static CANVAS_BUFFER: StaticL8CanvasBuffer<64> = StaticL8CanvasBuffer::new([0xFF; 64]);
// Boot-lifetime byte backing for the external-canvas path, four-byte aligned
// the way a PSRAM allocation handed to the adapter must be.
#[repr(align(4))]
struct AlignedExtBytes([u8; 64]);
static mut EXT_BYTES: AlignedExtBytes = AlignedExtBytes([0xAA; 64]);
static DRAW_BUFFER: StaticL8DrawBuffer<80> = StaticL8DrawBuffer::new();
static BLIT_MARKER: AtomicU32 = AtomicU32::new(0);
// LVGL and the intent-bridge queues are process-global: every test touching
// them holds this lock so parallel execution cannot interleave two runtimes.
static TEST_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn first_blitter(_: crate::DirtyArea, _: &[u8], _: &mut [u8]) -> bool {
    BLIT_MARKER.store(1, Ordering::Release);
    true
}

fn second_blitter(_: crate::DirtyArea, _: &[u8], _: &mut [u8]) -> bool {
    BLIT_MARKER.store(2, Ordering::Release);
    true
}

#[cfg(feature = "lvgl-gestures")]
fn pointer_sample() -> PointerSample {
    PointerSample::single(0, 0, PointerState::Released)
}

#[cfg(feature = "lvgl-gestures")]
fn gesture_event(_: GestureEvent) {}

#[no_mangle]
extern "C" fn meditamer_lvgl_alloc_pool(size: usize) -> *mut c_void {
    if size > LVGL_POOL_BYTES {
        return ptr::null_mut();
    }
    unsafe { ptr::addr_of_mut!(LVGL_POOL.0).cast() }
}

fn test_source() -> shell::types::SurfaceInstanceToken {
    // `SurfaceInstanceToken::issued` is crate-private, so construct the
    // equivalent first surface token from its public fields.
    shell::types::SurfaceInstanceToken {
        surface: shell::types::SurfaceRef {
            owner: shell::types::ProviderToken {
                id: shell::types::ProviderId(1),
                generation: shell::types::ProviderGeneration(1),
            },
            id: shell::types::SurfaceId(1),
        },
        generation: shell::types::InstanceGeneration(1),
    }
}

// LVGL and this module's registries are process-global, so a second
// concurrently-running test would corrupt both.
#[test]
fn safety_boundary() {
    let _serial = TEST_SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    assert!(!is_initialized());
    let session = RuntimeSession::initialize().expect("fresh runtime session");
    assert!(matches!(
        RuntimeSession::initialize(),
        Err(RuntimeSessionError::AlreadyClaimed)
    ));
    assert!(is_initialized());
    let _display = session.create_display(320, 240).expect("display");
    assert!(matches!(
        session.create_l8_display(320, 240, &DRAW_BUFFER, 1),
        Err(DisplayCreateError::InvalidBuffer)
    ));
    let token = session.access_token();
    packed_tests::render_packed_canvas(&session, &token);
    _display.make_default(&token).unwrap();
    let boot_screen = token
        .capture_unmanaged_active_screen()
        .expect("current runtime")
        .expect("implicit boot screen");
    assert!(token
        .capture_unmanaged_active_screen()
        .expect("current runtime")
        .is_none());

    #[cfg(feature = "lvgl-gestures")]
    let callbacks = PointerInputCallbacks {
        read: pointer_sample,
        gesture: gesture_event,
    };
    #[cfg(feature = "lvgl-gestures")]
    {
        assert!(matches!(
            session.create_pointer_input(0.0, callbacks),
            Err(PointerInputCreateError::InvalidThreshold)
        ));
        assert!(matches!(
            session.create_pointer_input(-0.1, callbacks),
            Err(PointerInputCreateError::InvalidThreshold)
        ));
        assert!(matches!(
            session.create_pointer_input(f32::NAN, callbacks),
            Err(PointerInputCreateError::InvalidThreshold)
        ));
        let input = session
            .create_pointer_input(0.1, callbacks)
            .expect("pointer input");
        let stale_access = input.access();
        assert!(matches!(
            session.create_pointer_input(0.1, callbacks),
            Err(PointerInputCreateError::AlreadyRegistered)
        ));
        drop(input);
        assert_eq!(stale_access.read(&token), Err(AccessFault::Stale));
        drop(
            session
                .create_pointer_input(0.1, callbacks)
                .expect("replacement pointer input"),
        );
    }

    let mut framebuffer = [0u8; 4];
    let mut nested_framebuffer = [0u8; 4];
    let frame = token
        .publish_l8_frame(&mut framebuffer, first_blitter)
        .expect("first frame publication");
    assert!(matches!(
        token.publish_l8_frame(&mut nested_framebuffer, second_blitter),
        Err(FlushFrameError::AlreadyPublished)
    ));
    BLIT_MARKER.store(0, Ordering::Release);
    let mut blit_scratch = [0u8; 1];
    assert!(runtime_io::blit_flush(
        token.runtime_id,
        crate::DirtyArea {
            x1: 0,
            y1: 0,
            x2: 0,
            y2: 0,
        },
        &[0],
        &mut blit_scratch,
    ));
    assert_eq!(BLIT_MARKER.load(Ordering::Acquire), 1);
    frame.finish();

    // Exercise the style, geometry, callback, and text operations used by
    // the presentation modules.
    let screen = Widget::screen(&token).expect("screen creation");
    screen
        .set_bg_color(&token, white(), StyleState::Default)
        .unwrap();
    screen.set_bg_opa(&token, 255, StyleState::Default).unwrap();
    screen
        .set_text_color(&token, black(), StyleState::Default)
        .unwrap();

    let button = screen
        .child(&token, WidgetKind::Button)
        .expect("button creation");
    button.remove_style_all(&token).unwrap();
    button.set_size(&token, 180, 64).unwrap();
    button.set_pos(&token, 210, 42).unwrap();
    button
        .set_bg_color(&token, white(), StyleState::Default)
        .unwrap();
    button
        .set_bg_color(&token, black(), StyleState::Pressed)
        .unwrap();
    button
        .set_border_color(&token, black(), StyleState::Default)
        .unwrap();
    button
        .set_border_width(&token, 3, StyleState::Default)
        .unwrap();
    button.set_radius(&token, 8, StyleState::Default).unwrap();
    button.set_ext_click_area(&token, 24).unwrap();
    // `set_clickable` narrows to just the one flag; `is_interactive`
    // reads back all three `set_non_interactive` clears, so exercise
    // each against the assertion it can actually support.
    button.set_clickable(&token, false).unwrap();
    button.set_clickable(&token, true).unwrap();
    button.set_non_interactive(&token).unwrap();
    assert!(
        !button.is_interactive(&token).unwrap(),
        "set_non_interactive must clear every flag is_interactive reads"
    );
    button.set_clickable(&token, true).unwrap();
    button.set_scrollable(&token, true).unwrap();
    assert!(button.is_scrollable(&token).unwrap());
    button.set_scrollable(&token, false).unwrap();
    assert!(!button.is_scrollable(&token).unwrap());
    assert!(
        button.is_interactive(&token).unwrap(),
        "disabling scroll must retain clicks"
    );
    button.set_non_interactive(&token).unwrap();

    let label = button
        .child(&token, WidgetKind::Label)
        .expect("label creation");
    label.set_text(&token, c"TOP TEST").unwrap();
    label
        .set_text_font(&token, Font::Size18, StyleState::Default)
        .unwrap();
    label.center(&token).unwrap();

    let hint = screen
        .child(&token, WidgetKind::Label)
        .expect("hint label creation");
    hint.set_text(&token, c"Use the arrows to browse test pages.")
        .unwrap();
    hint.set_text_font(&token, Font::Size18, StyleState::Default)
        .unwrap();
    hint.set_pos(&token, 146, 376).unwrap();

    // Exercise `Font::Size20`, a custom font (represented by a compiled-in
    // Montserrat table -- nothing about `Font::custom`
    // cares which `'static` table it points at), `set_width` alone,
    // `set_text_align`, and an `lv_line` drawn from a caller-owned
    // point buffer with `RADIUS_CIRCLE` exercised on a sibling
    // widget.
    let reading = screen
        .child(&token, WidgetKind::Label)
        .expect("environment reading label creation");
    reading.set_text(&token, c"21.0 C   40% RH").unwrap();
    reading
        .set_text_font(&token, Font::Size20, StyleState::Default)
        .unwrap();
    reading.set_width(&token, 600).unwrap();
    reading
        .set_text_align(&token, TextAlign::Center, StyleState::Default)
        .unwrap();
    // SAFETY: this built-in LVGL font and its descriptor graph are compiled
    // into static storage by LVGL itself.
    let custom_font = Font::custom(unsafe {
        StaticFontRef::from_generated(&*core::ptr::addr_of!(lv::lv_font_montserrat_24))
    });
    reading
        .set_text_font(&token, custom_font, StyleState::Default)
        .unwrap();

    let arc = screen
        .child(&token, WidgetKind::Line)
        .expect("arc line creation");
    assert_eq!(
        core::mem::size_of::<LinePoint>(),
        core::mem::size_of::<lv::lv_point_precise_t>()
    );
    assert_eq!(
        core::mem::align_of::<LinePoint>(),
        core::mem::align_of::<lv::lv_point_precise_t>()
    );
    assert_eq!(
        core::mem::size_of::<StaticLinePoints<3>>(),
        core::mem::size_of::<[lv::lv_point_precise_t; 3]>()
    );
    assert_eq!(
        ARC_POINTS.with_writer(&token, |writer| {
            writer.set(1, LinePoint::new(50.0, 25.0))
        }),
        Ok(true)
    );
    assert_eq!(
        arc.set_line_points(&token, &EMPTY_POINTS),
        Err(RetainedResourceError::EmptyLinePoints)
    );
    arc.set_line_points(&token, &ARC_POINTS).unwrap();
    arc.set_line_color(&token, black(), StyleState::Default)
        .unwrap();
    arc.set_line_width(&token, 4, StyleState::Default).unwrap();
    arc.set_line_rounded(&token, true, StyleState::Default)
        .unwrap();
    arc.set_clickable(&token, false).unwrap();

    let circle = screen
        .child(&token, WidgetKind::Container)
        .expect("circle creation");
    circle.remove_style_all(&token).unwrap();
    circle.set_size(&token, 40, 40).unwrap();
    circle
        .set_radius(&token, RADIUS_CIRCLE, StyleState::Default)
        .unwrap();

    // Widget kind is part of the checked handle, not just a
    // creation-time parameter: a canvas-only or line-only operation
    // on this plain container must be rejected before LVGL is
    // touched.
    assert_eq!(
        circle.set_l8_canvas_buffer(&token, &ONE_PIXEL, 1, 1),
        Err(RetainedResourceError::Access(AccessFault::WrongKind))
    );
    assert_eq!(
        circle.set_line_points(&token, &ARC_POINTS),
        Err(RetainedResourceError::Access(AccessFault::WrongKind))
    );
    assert_eq!(
        arc.set_l8_canvas_buffer(&token, &ONE_PIXEL, 1, 1),
        Err(RetainedResourceError::Access(AccessFault::WrongKind)),
        "a line handle is not a canvas handle either"
    );
    assert_eq!(
        circle.set_text(&token, c"not a label"),
        Err(AccessFault::WrongKind)
    );

    // Exercise `align` for horizontally centered dynamic text, `set_text`
    // from a short-lived buffer
    // (not a `'static` literal), and a canvas painted through a
    // caller-owned L8 buffer retained across per-frame repaints.
    let clock = screen
        .child(&token, WidgetKind::Label)
        .expect("clock label creation");
    clock
        .set_text_font(&token, Font::Size24, StyleState::Default)
        .unwrap();
    clock.align(&token, Align::TopMid, 0, 55).unwrap();
    let mut clock_text = *b"12:34\0";
    let clock_cstr = CStr::from_bytes_until_nul(&clock_text).unwrap();
    clock.set_text(&token, clock_cstr).unwrap();
    // The buffer is free to change (or go out of scope) immediately
    // after `set_text` returns -- LVGL already copied it -- unlike a
    // canvas buffer, which LVGL keeps reading from after this call.
    clock_text = *b"23:59\0";
    clock
        .set_text(&token, CStr::from_bytes_until_nul(&clock_text).unwrap())
        .unwrap();

    const CANVAS_W: i32 = 8;
    const CANVAS_H: i32 = 8;
    let canvas = screen
        .child(&token, WidgetKind::Canvas)
        .expect("canvas creation");
    assert_eq!(core::mem::size_of::<StaticL8CanvasBuffer<64>>(), 64);
    assert_eq!(core::mem::align_of::<StaticL8CanvasBuffer<64>>(), 4);
    assert_eq!(CANVAS_BUFFER.as_mut_ptr() as usize % 4, 0);
    assert_eq!(
        canvas.set_l8_canvas_buffer(&token, &CANVAS_BUFFER, 0, CANVAS_H),
        Err(RetainedResourceError::InvalidCanvasDimensions)
    );
    assert_eq!(
        canvas.set_l8_canvas_buffer(&token, &ONE_PIXEL, CANVAS_W, CANVAS_H),
        Err(RetainedResourceError::CanvasBufferTooSmall)
    );
    canvas
        .set_l8_canvas_buffer(&token, &CANVAS_BUFFER, CANVAS_W, CANVAS_H)
        .unwrap();
    canvas.align(&token, Align::Center, 0, -15).unwrap();
    assert_eq!(
        CANVAS_BUFFER.with_writer(&token, |writer| {
            // Re-entering a checked LVGL operation is safe: the writer
            // never exposes or keeps a Rust reference into retained
            // storage across user code.
            hint.set_pos(&token, 146, 376).unwrap();
            writer.set(0, 0x00)
        }),
        Ok(true),
        "the retained resource owns the same buffer LVGL reads"
    );
    canvas.invalidate(&token).unwrap();

    // Boot-lifetime external canvas: one leaked byte buffer plus the small
    // wrapper, allocated separately — the shape the SD-clock ambient canvas
    // will use at 360KB scale, exercised here at 8x8.
    let ext_bytes: &'static mut [u8] = unsafe { &mut *core::ptr::addr_of_mut!(EXT_BYTES.0) };
    assert_eq!(ext_bytes.as_ptr() as usize % 4, 0);
    let ext: &'static ExternalL8CanvasBuffer = Box::leak(Box::new(
        ExternalL8CanvasBuffer::new(ext_bytes).expect("aligned non-empty storage"),
    ));
    assert_eq!(ext.len(), 64);
    assert!(!ext.is_empty());
    assert_eq!(
        circle.set_external_l8_canvas_buffer(&token, ext, CANVAS_W, CANVAS_H),
        Err(RetainedResourceError::Access(AccessFault::WrongKind))
    );
    assert_eq!(
        canvas.set_external_l8_canvas_buffer(&token, ext, 0, CANVAS_H),
        Err(RetainedResourceError::InvalidCanvasDimensions)
    );
    assert_eq!(
        canvas.set_external_l8_canvas_buffer(&token, ext, CANVAS_W + 1, CANVAS_H),
        Err(RetainedResourceError::CanvasBufferTooSmall)
    );
    canvas
        .set_external_l8_canvas_buffer(&token, ext, CANVAS_W, CANVAS_H)
        .unwrap();
    let foreign_token = UiAccessToken {
        runtime_id: token.runtime_id.wrapping_add(1),
        _not_send_or_sync: core::marker::PhantomData,
    };
    assert_eq!(
        ext.with_writer(&foreign_token, |_| ()),
        Err(AccessFault::WrongRuntime)
    );
    ext.with_writer(&token, |writer| {
        // Re-entering a checked LVGL operation is safe here too: the writer
        // holds no Rust reference into the retained bytes across user code.
        hint.set_pos(&token, 146, 376).unwrap();
        assert!(!writer.write(64, 0x00), "write past the end must fail");
        assert!(writer.write(63, 0x00));
        assert!(
            !writer.copy_from(63, &[0x11, 0x22]),
            "overflowing copy must fail without mutation"
        );
        assert!(!writer.copy_from(usize::MAX, &[0x01]));
        assert!(writer.copy_from(62, &[0x11, 0x22]));
        assert!(writer.fill(0xFF));
        // MSB-first ink expansion: 0b1010 paints ink, paper, ink.
        assert_eq!(writer.write_ink_bits(&[0b1010_0000], 3), 3);
        assert_eq!(
            writer.write_ink_bits(&[0xFF; 8], usize::MAX),
            64,
            "ink writes clamp to storage capacity"
        );
        assert_eq!(writer.write_ink_bits(&[], 8), 0);
    })
    .unwrap();
    canvas.invalidate(&token).unwrap();

    let overlay = Widget::on_system_layer(&token, WidgetKind::Button).expect("overlay creation");
    overlay.set_hidden(&token, true).unwrap();
    overlay.remove_style_all(&token).unwrap();
    overlay.set_size(&token, 112, 38).unwrap();
    overlay.set_pos(&token, 474, 12).unwrap();

    // A click routed through the adapter must reach the queued intent path.
    let source = test_source();
    let mut actions = [None; crate::intent_bridge::SCREEN_NAVIGATION_CAPACITY];
    actions[0] = Some(crate::intent_bridge::ScreenAction::Navigate(
        shell::types::NavIntent::Home,
    ));
    let bindings = crate::intent_bridge::IntentBindings::Screen {
        source,
        actions,
        show_confirm: shell::types::CompositionIntent::DismissActiveModal,
    };
    let lease = crate::intent_bridge::claim(bindings).expect("callback route claim");
    crate::intent_bridge::enable(&lease).expect("callback route enable");
    let routed = screen
        .child(&token, WidgetKind::Button)
        .expect("routed button creation");
    crate::intent_bridge::bind_click(
        &routed,
        &token,
        crate::intent_bridge::RouteCallback::Navigation { index: 0 },
        &lease,
    )
    .unwrap();
    assert!(
        routed.send_click(&token).unwrap(),
        "synthetic click dispatch must report success"
    );
    assert!(matches!(
        crate::intent_bridge::take_intent(),
        Some(shell::types::OwnedShellIntent::Navigate(_))
    ));
    crate::intent_bridge::release(&lease).expect("callback route release");
    // The LVGL object and callback registration outlive release of the
    // route. A callback firing afterward must no
    // longer deliver anything, proving teardown is enforced by the
    // route table, not merely by the widget also being gone.
    assert!(routed.send_click(&token).unwrap());
    assert_eq!(
        crate::intent_bridge::take_intent(),
        None,
        "a released callback route must not deliver after teardown"
    );
    routed.delete(&token).expect("routed button deletion");

    // Every typed callback variant must preserve its route/payload
    // pairing across the raw registration gateway.
    let confirm_lease = crate::intent_bridge::claim(bindings).expect("confirm route claim");
    crate::intent_bridge::enable(&confirm_lease).expect("confirm route enable");
    let confirm = screen
        .child(&token, WidgetKind::Button)
        .expect("confirm button creation");
    crate::intent_bridge::bind_click(
        &confirm,
        &token,
        crate::intent_bridge::RouteCallback::ShowConfirm,
        &confirm_lease,
    )
    .unwrap();
    assert!(confirm.send_click(&token).unwrap());
    assert!(matches!(
        crate::intent_bridge::take_intent(),
        Some(shell::types::OwnedShellIntent::Compose(_))
    ));
    crate::intent_bridge::release(&confirm_lease).unwrap();
    confirm.delete(&token).unwrap();

    let dismiss = shell::types::OwnedCompositionIntent {
        source,
        intent: shell::types::CompositionIntent::DismissActiveModal,
    };
    let modal_navigation = shell::types::OwnedNavIntent {
        source,
        intent: shell::types::NavIntent::Back,
    };
    let dismiss_lease = crate::intent_bridge::claim(crate::intent_bridge::IntentBindings::Modal {
        dismiss,
        navigation: Some(modal_navigation),
    })
    .expect("dismiss route claim");
    crate::intent_bridge::enable(&dismiss_lease).expect("dismiss route enable");
    let dismiss_button = screen
        .child(&token, WidgetKind::Button)
        .expect("dismiss button creation");
    crate::intent_bridge::bind_click(
        &dismiss_button,
        &token,
        crate::intent_bridge::RouteCallback::DismissModal,
        &dismiss_lease,
    )
    .unwrap();
    assert!(dismiss_button.send_click(&token).unwrap());
    assert_eq!(
        crate::intent_bridge::take_intent(),
        Some(shell::types::OwnedShellIntent::Compose(dismiss))
    );
    let launcher_button = screen.child(&token, WidgetKind::Button).unwrap();
    crate::intent_bridge::bind_click(
        &launcher_button,
        &token,
        crate::intent_bridge::RouteCallback::Navigation { index: 0 },
        &dismiss_lease,
    )
    .unwrap();
    assert!(launcher_button.send_click(&token).unwrap());
    assert_eq!(
        crate::intent_bridge::take_intent(),
        Some(shell::types::OwnedShellIntent::Navigate(modal_navigation))
    );
    crate::intent_bridge::release(&dismiss_lease).unwrap();
    assert!(launcher_button.send_click(&token).unwrap());
    assert_eq!(crate::intent_bridge::take_intent(), None);
    launcher_button.delete(&token).unwrap();
    dismiss_button.delete(&token).unwrap();

    let repaint = shell::types::OwnedRefreshIntent {
        source,
        intent: shell::types::RefreshIntent::FullRepaint,
    };
    let repaint_lease =
        crate::intent_bridge::claim(crate::intent_bridge::IntentBindings::Refresh {
            request: repaint,
        })
        .expect("repaint route claim");
    crate::intent_bridge::enable(&repaint_lease).expect("repaint route enable");
    let repaint_button = screen
        .child(&token, WidgetKind::Button)
        .expect("repaint button creation");
    crate::intent_bridge::bind_click(
        &repaint_button,
        &token,
        crate::intent_bridge::RouteCallback::FullRepaint,
        &repaint_lease,
    )
    .unwrap();
    assert!(repaint_button.send_click(&token).unwrap());
    assert_eq!(
        crate::intent_bridge::take_intent(),
        Some(shell::types::OwnedShellIntent::ScreenUpdate(repaint.into()))
    );
    crate::intent_bridge::release(&repaint_lease).unwrap();
    repaint_button.delete(&token).unwrap();

    let action_lease =
        crate::intent_bridge::claim(crate::intent_bridge::IntentBindings::Action { source })
            .expect("action route claim");
    crate::intent_bridge::enable(&action_lease).expect("action route enable");
    let action_button = screen
        .child(&token, WidgetKind::Button)
        .expect("action button creation");
    crate::intent_bridge::bind_click(
        &action_button,
        &token,
        crate::intent_bridge::RouteCallback::Action { index: 4 },
        &action_lease,
    )
    .unwrap();
    assert!(action_button.send_click(&token).unwrap());
    assert_eq!(
        crate::intent_bridge::take_action(),
        Some(crate::intent_bridge::IndexedAction { source, index: 4 })
    );
    assert!(action_button.send_click(&token).unwrap());
    assert_eq!(crate::intent_bridge::purge_instance(source), 1);
    assert_eq!(crate::intent_bridge::take_action(), None);
    crate::intent_bridge::release(&action_lease).unwrap();
    assert!(action_button.send_click(&token).unwrap());
    assert_eq!(crate::intent_bridge::take_action(), None);
    action_button.delete(&token).unwrap();

    let ambient = screen
        .child(&token, WidgetKind::Button)
        .expect("ambient button creation");
    crate::intent_bridge::bind_ambient_tap(&ambient, &token).unwrap();
    assert!(ambient.send_click(&token).unwrap());
    assert!(crate::intent_bridge::take_ambient_tap_requested());
    ambient.delete(&token).unwrap();

    // A wrong-runtime handle must be rejected before LVGL is touched.
    // Built directly with a mismatched `runtime_id` rather than via a
    // second `UiAccessToken::issue()`: `issue()` clears the
    // checked-handle registries for the new epoch it represents (this
    // test's own registry-exhaustion case below exercises that), and
    // this session is still very much live -- issuing a second token
    // for it would incorrectly simulate the real reinit that never
    // happened here.
    let other_token = UiAccessToken {
        runtime_id: token.runtime_id.wrapping_add(1),
        _not_send_or_sync: core::marker::PhantomData,
    };
    assert_eq!(
        hint.set_pos(&other_token, 0, 0),
        Err(AccessFault::WrongRuntime)
    );
    assert_eq!(
        CANVAS_BUFFER.with_writer(&other_token, |_| ()),
        Err(AccessFault::WrongRuntime)
    );
    hint.set_pos(&token, 146, 376).unwrap();

    // Loading a screen must update the adapter's active-screen view.
    assert!(screen.activate(&token).unwrap(), "screen must load");
    assert!(boot_screen.delete(&token).is_ok());
    assert!(screen.is_active_screen(&token).unwrap());
    assert_eq!(label.activate(&token), Err(AccessFault::NotScreen));
    let other_screen = Widget::screen(&token).expect("second screen creation");
    assert!(other_screen.activate(&token).unwrap());
    assert!(!screen.is_active_screen(&token).unwrap());
    assert!(other_screen.is_active_screen(&token).unwrap());
    other_screen.delete(&token).expect("second screen deletion");
    screen.activate(&token).unwrap();

    // Confirmed parent deletion invalidates every live descendant:
    // `button`'s subtree
    // (`label`) goes stale the instant `screen` is torn down, and
    // `button` itself -- destroyed as `screen`'s child, never
    // through its own `delete` -- is equally stale afterward.
    screen.delete(&token).expect("screen deletion");
    assert_eq!(label.set_pos(&token, 0, 0), Err(AccessFault::Stale));
    assert_eq!(button.set_pos(&token, 0, 0), Err(AccessFault::Stale));
    assert_eq!(hint.delete(&token), Err(WidgetDeleteFailure::AlreadyGone));

    // Exhaust the registry while a live "active" widget remains registered
    // and confirm it
    // stays usable throughout the failed candidate build, matching the
    // origin-stays-usable contract in
    // `shell::lifecycle::execute_transition`.
    let active = Widget::on_system_layer(&token, WidgetKind::Container).expect("active widget");
    let scratch = Widget::on_system_layer(&token, WidgetKind::Container).expect("scratch parent");
    let mut filler = heapless::Vec::<Widget, WIDGET_REGISTRY_CAPACITY>::new();
    let mut exhausted = false;
    for _ in 0..WIDGET_REGISTRY_CAPACITY + 1 {
        match scratch.child(&token, WidgetKind::Container) {
            Ok(child) => {
                active.set_pos(&token, 1, 1).unwrap();
                filler.push(child).ok();
            }
            Err(WidgetCreateError::RegistryFull) => {
                exhausted = true;
                break;
            }
            Err(other) => panic!("unexpected candidate failure: {other:?}"),
        }
    }
    assert!(
        exhausted,
        "registry capacity must be reachable and reported"
    );
    active.set_pos(&token, 2, 2).unwrap();
    while let Some(child) = filler.pop() {
        child.delete(&token).expect("filler deletion");
    }
    scratch.delete(&token).expect("scratch deletion");
    active.delete(&token).expect("active deletion");

    overlay.delete(&token).expect("overlay deletion");

    // `lv_mem_test` proves allocator consistency, not peak composition fit.
    // The high-water mark records how close the worst-case composition
    // this test just drove (the registry filled to capacity while a
    // full Home-shaped tree, an Ambient-View-shaped tree, and the
    // failed-replacement candidates were all live at once) came to
    // exhausting the LVGL heap. `lv_mem_monitor`'s `max_used` is a
    // running high-water mark since `lv_init`, so it already captured
    // that peak regardless of every object since freed; querying it
    // here turns "it didn't panic" into a measured capacity check.
    let monitor = token.memory_snapshot().expect("current runtime");
    assert!(
        monitor.max_used > 0,
        "the monitor itself must have observed some allocation activity"
    );
    assert!(
        monitor.max_used <= LVGL_POOL_BYTES,
        "peak LVGL heap usage ({} bytes) must fit the {}-byte pool this test allocates",
        monitor.max_used,
        LVGL_POOL_BYTES
    );
    println!(
            "lvgl_adapter safety_boundary: peak heap usage {} / {} bytes ({}% used, {}% fragmented at query time)",
            monitor.max_used,
            LVGL_POOL_BYTES,
            monitor.used_percent,
            monitor.fragmentation_percent
        );

    // LVGL may reuse a freed object's address. Drive `(addr, generation)`
    // bookkeeping directly
    // (rather than hoping a real LVGL alloc/free cycle happens to
    // reuse an address, which is not guaranteed) to prove a stale
    // generation at a reused address is rejected while the fresh
    // generation at that address is accepted.
    let reused_addr = 0xDEAD_BEEF_usize;
    let stale_generation = next_generation(&WIDGET_GENERATION).expect("stale generation");
    assert!(registry_upsert(
        &WIDGET_REGISTRY,
        reused_addr,
        stale_generation
    ));
    assert!(registry_check(
        &WIDGET_REGISTRY,
        reused_addr,
        stale_generation
    ));
    registry_remove(&WIDGET_REGISTRY, reused_addr);
    let fresh_generation = next_generation(&WIDGET_GENERATION).expect("fresh generation");
    assert_ne!(stale_generation, fresh_generation);
    assert!(registry_upsert(
        &WIDGET_REGISTRY,
        reused_addr,
        fresh_generation
    ));
    assert!(
        !registry_check(&WIDGET_REGISTRY, reused_addr, stale_generation),
        "a stale generation must not match its old address once reused"
    );
    assert!(registry_check(
        &WIDGET_REGISTRY,
        reused_addr,
        fresh_generation
    ));
    registry_remove(&WIDGET_REGISTRY, reused_addr);

    // A construction failure that cannot delete what it built parks the
    // widget instead of
    // dropping it, and a later retry reclaims it once it becomes
    // deletable. Exercised here against an ordinary (always-deletable)
    // widget -- forcing a genuine LVGL delete refusal would need
    // undocumented internal behavior -- so this proves the park/retry
    // bookkeeping itself, not LVGL's refusal semantics.
    let park_me = Widget::on_system_layer(&token, WidgetKind::Container).expect("park_me widget");
    park_orphaned_widget(park_me).expect("first park must succeed under capacity");
    assert_eq!(
        retry_parked_widgets(&token),
        0,
        "an ordinary (deletable) parked widget must be reclaimed on retry"
    );

    // Capacity is enforced: filling every slot then parking one more
    // hands the widget straight back instead of silently discarding it.
    for _ in 0..ORPHANED_WIDGET_CAPACITY {
        let filler = Widget::on_system_layer(&token, WidgetKind::Container).expect("filler widget");
        park_orphaned_widget(filler).expect("park under capacity must succeed");
    }
    let overflow = Widget::on_system_layer(&token, WidgetKind::Container).expect("overflow widget");
    let returned = park_orphaned_widget(overflow)
        .expect_err("parking past capacity must hand the widget back");
    assert_eq!(
        retry_parked_widgets(&token),
        0,
        "every filler must still be reclaimed once retried"
    );
    returned
        .delete(&token)
        .expect("the handed-back widget is still ours to delete directly");

    // A token whose epoch matches a widget's recorded runtime must still
    // be rejected once a later `issue()` has moved `WIDGET_RUNTIME_ID`
    // past it -- the gap a bare
    // `self.runtime_id != token.runtime_id` comparison alone would miss,
    // since a hand-built token sharing the widget's original epoch would
    // otherwise pass.
    let epoch_widget =
        Widget::on_system_layer(&token, WidgetKind::Container).expect("epoch widget");
    let stale_epoch_token = UiAccessToken {
        runtime_id: token.runtime_id,
        _not_send_or_sync: core::marker::PhantomData,
    };
    packed_tests::reject_expired_writers_inside_closures(&token);
    let reinit_token = UiAccessToken::issue();
    packed_tests::reject_stale_writer(&stale_epoch_token);
    assert_eq!(
        ext.with_writer(&token, |_| ()),
        Err(AccessFault::WrongRuntime),
        "the pre-reinit token must not drive the external writer once the epoch moved on"
    );
    assert!(!UNMANAGED_SCREEN_CAPTURED.load(Ordering::Acquire));
    assert_eq!(
            epoch_widget.set_pos(&stale_epoch_token, 0, 0),
            Err(AccessFault::WrongRuntime),
            "a token sharing the widget's original epoch must still be rejected once that epoch is no longer live"
        );
    assert_eq!(
        epoch_widget.set_pos(&reinit_token, 0, 0),
        Err(AccessFault::WrongRuntime)
    );

    // `issue` must clear `ORPHANED_WIDGETS`, not just `WIDGET_REGISTRY`:
    // a widget parked under a prior epoch has no LVGL identity worth
    // retrying once that epoch is gone, so a later `issue()` must not
    // leave it sitting there for `retry_parked_widgets` to find.
    let orphan_before_reinit =
        Widget::on_system_layer(&reinit_token, WidgetKind::Container).expect("orphan widget");
    park_orphaned_widget(orphan_before_reinit).expect("park under capacity must succeed");
    let post_reinit_token = UiAccessToken::issue();
    assert_eq!(
        retry_parked_widgets(&post_reinit_token),
        0,
        "issue() must clear ORPHANED_WIDGETS so a prior epoch's parked widget is not retried"
    );

    assert!(
        post_reinit_token
            .memory_snapshot()
            .expect("current runtime")
            .integrity_ok,
        "the adapter must leave LVGL's allocator consistent"
    );

    drop(session);
    assert!(!is_initialized());
    assert_eq!(
        post_reinit_token.memory_snapshot(),
        Err(AccessFault::WrongRuntime)
    );

    // Stale guards from an ended epoch must not clear a later epoch's
    // framebuffer or pointer callback registration, even if LVGL reuses an
    // input address after deinit/reinit.
    let old_session = RuntimeSession::initialize().expect("second owned runtime");
    let old_token = old_session.access_token();
    let mut old_framebuffer = [0u8; 4];
    let old_frame = old_token
        .publish_l8_frame(&mut old_framebuffer, first_blitter)
        .expect("old frame");
    #[cfg(feature = "lvgl-gestures")]
    let old_input = old_session
        .create_pointer_input(0.1, callbacks)
        .expect("old input");
    drop(old_session);

    let new_session = RuntimeSession::initialize().expect("third owned runtime");
    let new_token = new_session.access_token();
    let mut new_framebuffer = [0u8; 4];
    let mut competing_framebuffer = [0u8; 4];
    let new_frame = new_token
        .publish_l8_frame(&mut new_framebuffer, second_blitter)
        .expect("new frame");
    #[cfg(feature = "lvgl-gestures")]
    let new_input = new_session
        .create_pointer_input(0.1, callbacks)
        .expect("new input");
    drop(old_frame);
    #[cfg(feature = "lvgl-gestures")]
    drop(old_input);
    assert!(matches!(
        new_token.publish_l8_frame(&mut competing_framebuffer, first_blitter),
        Err(FlushFrameError::AlreadyPublished)
    ));
    #[cfg(feature = "lvgl-gestures")]
    assert!(matches!(
        new_session.create_pointer_input(0.1, callbacks),
        Err(PointerInputCreateError::AlreadyRegistered)
    ));
    new_frame.finish();
    #[cfg(feature = "lvgl-gestures")]
    drop(new_input);
    drop(new_session);

    // Board ownership is a process-lifetime handoff: releasing the Rust
    // facade cannot make the still-live LVGL objects safe to adopt again.
    raw::init();
    let adopted = RuntimeSession::adopt_initialized().expect("first board adoption");
    drop(adopted);
    assert!(matches!(
        RuntimeSession::adopt_initialized(),
        Err(RuntimeSessionError::AlreadyAdopted)
    ));
    raw::deinit();
}

// Constructor-only checks touch no LVGL or adapter globals, so this runs
// safely alongside `safety_boundary`: it proves the boot-time gate rejects
// storage LVGL could never retain before any runtime exists to attach it to.
#[test]
fn external_canvas_constructor_rejects_unusable_storage() {
    let empty: &'static mut [u8] = Box::leak(Box::new([0u8; 0]));
    assert!(matches!(
        ExternalL8CanvasBuffer::new(empty),
        Err(RetainedResourceError::InvalidExternalCanvasBuffer)
    ));

    // Skew a boot-lifetime slice off alignment inside an aligned heap block.
    let block: &'static mut [u8; 67] = Box::leak(Box::new([0xAA; 67]));
    let base = block.as_ptr() as usize;
    let skew = (0..4)
        .find(|offset| !(base + offset).is_multiple_of(4))
        .expect("one of four consecutive offsets misaligns");
    let skewed: &'static mut [u8] =
        unsafe { &mut *core::ptr::slice_from_raw_parts_mut(block.as_mut_ptr().add(skew), 64) };
    assert!(!(skewed.as_ptr() as usize).is_multiple_of(4));
    assert!(matches!(
        ExternalL8CanvasBuffer::new(skewed),
        Err(RetainedResourceError::InvalidExternalCanvasBuffer)
    ));
}

// Focused slider coverage: kind-checked range/set/get, typed
// `LV_EVENT_VALUE_CHANGED` routing into the single latest-value slot through
// the shared `Action` lease, and the coalescing/conflicting-owner/
// release/purge/stale-route behavior of the value path. Serialized against
// `safety_boundary` through `TEST_SERIAL`: both drive the same
// process-global LVGL runtime and intent-bridge queues.
#[test]
fn slider_range_value_and_coalesced_routing() {
    let _serial = TEST_SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // Leave the shared queues as found (empty) so serial order never matters.
    while crate::intent_bridge::take_intent().is_some() {}
    while crate::intent_bridge::take_action().is_some() {}
    while crate::intent_bridge::take_value_action().is_some() {}
    assert!(
        !crate::intent_bridge::take_overflowed(),
        "no overflow may leak in from another test"
    );

    let session = RuntimeSession::initialize().expect("fresh runtime session");
    let _display = session.create_display(320, 240).expect("display");
    let token = session.access_token();
    let source = test_source();

    let screen = Widget::screen(&token).expect("screen");
    let plain = screen
        .child(&token, WidgetKind::Container)
        .expect("plain container");

    // Kind-checked range/set/get: non-sliders are rejected before LVGL.
    assert_eq!(
        plain.set_slider_range(&token, 0, 100),
        Err(AccessFault::WrongKind)
    );
    assert_eq!(
        plain.set_slider_value(&token, 1),
        Err(AccessFault::WrongKind)
    );
    assert_eq!(plain.slider_value(&token), Err(AccessFault::WrongKind));
    assert_eq!(plain.slider_range(&token), Err(AccessFault::WrongKind));
    assert_eq!(
        plain.send_value_changed(&token),
        Err(AccessFault::WrongKind)
    );
    let probe_lease =
        crate::intent_bridge::claim(crate::intent_bridge::IntentBindings::Action { source })
            .expect("probe route");
    assert!(matches!(
        crate::intent_bridge::bind_click(
            &plain,
            &token,
            crate::intent_bridge::RouteCallback::Value { index: 0 },
            &probe_lease,
        ),
        Err(WidgetCallbackError::Access(AccessFault::WrongKind))
    ));
    crate::intent_bridge::release(&probe_lease).expect("probe release");

    let slider = screen.child(&token, WidgetKind::Slider).expect("slider");

    // Range/set/get, including LVGL's own clamping to the stored range.
    slider.set_slider_range(&token, 0, 200).unwrap();
    assert_eq!(slider.slider_range(&token), Ok((0, 200)));
    slider.set_slider_value(&token, 150).unwrap();
    assert_eq!(slider.slider_value(&token), Ok(150));
    slider.set_slider_value(&token, 999).unwrap();
    assert_eq!(slider.slider_value(&token), Ok(200));
    slider.set_slider_value(&token, -5).unwrap();
    assert_eq!(slider.slider_value(&token), Ok(0));

    // Typed value routing through the shared `Action` lease into the single
    // latest-value slot.
    let lease =
        crate::intent_bridge::claim(crate::intent_bridge::IntentBindings::Action { source })
            .expect("value route");
    crate::intent_bridge::enable(&lease).expect("value route enable");
    crate::intent_bridge::bind_click(
        &slider,
        &token,
        crate::intent_bridge::RouteCallback::Value { index: 2 },
        &lease,
    )
    .unwrap();
    slider.set_slider_value(&token, 70).unwrap();
    assert!(slider.send_value_changed(&token).unwrap());
    assert_eq!(
        crate::intent_bridge::take_value_action(),
        Some(crate::intent_bridge::IndexedValueAction {
            source,
            index: 2,
            value: 70,
        })
    );
    // Three rapid drags collapse to one latest value ...
    for value in [10, 20, 30] {
        slider.set_slider_value(&token, value).unwrap();
        assert!(slider.send_value_changed(&token).unwrap());
    }
    assert_eq!(
        crate::intent_bridge::take_value_action(),
        Some(crate::intent_bridge::IndexedValueAction {
            source,
            index: 2,
            value: 30,
        })
    );
    assert_eq!(crate::intent_bridge::take_value_action(), None);
    // ... and the reliable click queue is untouched by value traffic.
    assert_eq!(crate::intent_bridge::take_action(), None);
    assert_eq!(crate::intent_bridge::take_intent(), None);
    assert!(!crate::intent_bridge::take_overflowed());

    // Single-slot ownership: a second (route, index) owner firing while one
    // level is pending raises the shared overflow diagnostic and leaves the
    // pending level untouched -- values never silently cross owners.
    let lease_b =
        crate::intent_bridge::claim(crate::intent_bridge::IntentBindings::Action { source })
            .expect("second action route");
    crate::intent_bridge::enable(&lease_b).expect("second route enable");
    let slider2 = screen
        .child(&token, WidgetKind::Slider)
        .expect("second slider");
    crate::intent_bridge::bind_click(
        &slider2,
        &token,
        crate::intent_bridge::RouteCallback::Value { index: 3 },
        &lease_b,
    )
    .unwrap();
    slider.set_slider_value(&token, 1).unwrap();
    assert!(slider.send_value_changed(&token).unwrap());
    slider2.set_slider_value(&token, 2).unwrap();
    assert!(slider2.send_value_changed(&token).unwrap());
    assert!(
        crate::intent_bridge::take_overflowed(),
        "a conflicting owner must raise the shared diagnostics flag"
    );
    assert_eq!(
        crate::intent_bridge::take_value_action(),
        Some(crate::intent_bridge::IndexedValueAction {
            source,
            index: 2,
            value: 1,
        }),
        "the conflicting value must not overwrite the pending owner"
    );
    assert_eq!(crate::intent_bridge::take_value_action(), None);
    // With the slot free, the other owner delivers normally.
    slider2.set_slider_value(&token, 2).unwrap();
    assert!(slider2.send_value_changed(&token).unwrap());
    assert_eq!(
        crate::intent_bridge::take_value_action(),
        Some(crate::intent_bridge::IndexedValueAction {
            source,
            index: 3,
            value: 2,
        })
    );
    assert_eq!(crate::intent_bridge::take_value_action(), None);
    assert!(!crate::intent_bridge::take_overflowed());
    crate::intent_bridge::release(&lease_b).expect("second route release");

    // Release drops the route: the LVGL registration outlives it, but firing
    // afterward must not deliver.
    crate::intent_bridge::release(&lease).expect("value route release");
    assert!(slider.send_value_changed(&token).unwrap());
    assert!(slider2.send_value_changed(&token).unwrap());
    assert_eq!(crate::intent_bridge::take_value_action(), None);
    // Re-claiming the slot (new generation) does not resurrect the stale
    // binding, while a fresh binding on the new route delivers.
    let lease2 =
        crate::intent_bridge::claim(crate::intent_bridge::IntentBindings::Action { source })
            .expect("second action route");
    crate::intent_bridge::enable(&lease2).expect("second enable");
    assert!(slider.send_value_changed(&token).unwrap());
    assert_eq!(crate::intent_bridge::take_value_action(), None);
    crate::intent_bridge::bind_click(
        &slider2,
        &token,
        crate::intent_bridge::RouteCallback::Value { index: 3 },
        &lease2,
    )
    .unwrap();
    slider2.set_slider_value(&token, 55).unwrap();
    assert!(slider2.send_value_changed(&token).unwrap());
    assert_eq!(
        crate::intent_bridge::take_value_action(),
        Some(crate::intent_bridge::IndexedValueAction {
            source,
            index: 3,
            value: 55,
        })
    );
    crate::intent_bridge::release(&lease2).expect("second release");
    assert_eq!(crate::intent_bridge::take_value_action(), None);

    // Purge drops the pending level for the instance ...
    let lease3 =
        crate::intent_bridge::claim(crate::intent_bridge::IntentBindings::Action { source })
            .expect("third action route");
    crate::intent_bridge::enable(&lease3).expect("third enable");
    crate::intent_bridge::bind_click(
        &slider,
        &token,
        crate::intent_bridge::RouteCallback::Value { index: 2 },
        &lease3,
    )
    .unwrap();
    slider.set_slider_value(&token, 77).unwrap();
    assert!(slider.send_value_changed(&token).unwrap());
    assert_eq!(crate::intent_bridge::purge_instance(source), 1);
    assert_eq!(crate::intent_bridge::take_value_action(), None);
    // ... and release with a pending level drops it rather than delivering
    // it stale.
    slider.set_slider_value(&token, 78).unwrap();
    assert!(slider.send_value_changed(&token).unwrap());
    crate::intent_bridge::release(&lease3).expect("third release");
    assert_eq!(
        crate::intent_bridge::take_value_action(),
        None,
        "release must drop the pending level from its own route"
    );

    slider2.delete(&token).expect("second slider deletion");
    slider.delete(&token).expect("slider deletion");
    plain.delete(&token).expect("plain deletion");
    screen.delete(&token).expect("screen deletion");

    assert_eq!(crate::intent_bridge::take_value_action(), None);
    assert_eq!(crate::intent_bridge::take_action(), None);
    assert_eq!(crate::intent_bridge::take_intent(), None);
    assert!(!crate::intent_bridge::take_overflowed());
    drop(session);
}
