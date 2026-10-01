use core::{
    ffi::c_void,
    ptr,
    sync::atomic::{AtomicBool, AtomicI32, AtomicU32, Ordering},
};

use lightvgl_sys as lv;

const LVGL_POOL_BYTES: usize = 128 * 1024;

#[repr(align(16))]
struct AlignedPool([u8; LVGL_POOL_BYTES]);

static mut LVGL_POOL: AlignedPool = AlignedPool([0; LVGL_POOL_BYTES]);
static POINTER_PRESSED: AtomicBool = AtomicBool::new(false);
static POINTER_X: AtomicI32 = AtomicI32::new(40);
static POINTER_Y: AtomicI32 = AtomicI32::new(40);
static UNDERLAY_CLICKS: AtomicU32 = AtomicU32::new(0);
static INTERACTIVE_CLICKS: AtomicU32 = AtomicU32::new(0);
static MODAL_CAPTURE_CLICKS: AtomicU32 = AtomicU32::new(0);
static MODAL_BUTTON_CLICKS: AtomicU32 = AtomicU32::new(0);

#[no_mangle]
extern "C" fn meditamer_lvgl_alloc_pool(size: usize) -> *mut c_void {
    if size > LVGL_POOL_BYTES {
        return ptr::null_mut();
    }
    unsafe { ptr::addr_of_mut!(LVGL_POOL.0).cast() }
}

unsafe fn object(
    parent: *mut lv::lv_obj_t,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) -> *mut lv::lv_obj_t {
    let object = unsafe { lv::lv_obj_create(parent) };
    assert!(!object.is_null());
    unsafe {
        lv::lv_obj_set_pos(object, x, y);
        lv::lv_obj_set_size(object, width, height);
    }
    object
}

unsafe fn make_passive(object: *mut lv::lv_obj_t) {
    unsafe {
        lv::lv_obj_remove_flag(object, lv::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE);
        lv::lv_obj_remove_flag(object, lv::lv_obj_flag_t_LV_OBJ_FLAG_CLICK_FOCUSABLE);
        lv::lv_obj_remove_flag(object, lv::lv_obj_flag_t_LV_OBJ_FLAG_SCROLLABLE);
    }
}

unsafe fn pointer_target(point: &mut lv::lv_point_t) -> *mut lv::lv_obj_t {
    for layer in unsafe {
        [
            lv::lv_layer_sys(),
            lv::lv_layer_top(),
            lv::lv_screen_active(),
        ]
    } {
        let target = unsafe { lv::lv_indev_search_obj(layer, point) };
        if !target.is_null() {
            return target;
        }
    }
    ptr::null_mut()
}

unsafe extern "C" fn read_pointer(_indev: *mut lv::lv_indev_t, data: *mut lv::lv_indev_data_t) {
    unsafe {
        (*data).point = lv::lv_point_t {
            x: POINTER_X.load(Ordering::Acquire),
            y: POINTER_Y.load(Ordering::Acquire),
        };
        (*data).state = if POINTER_PRESSED.load(Ordering::Acquire) {
            lv::lv_indev_state_t_LV_INDEV_STATE_PRESSED
        } else {
            lv::lv_indev_state_t_LV_INDEV_STATE_RELEASED
        };
        (*data).continue_reading = false;
    }
}

unsafe extern "C" fn underlay_clicked(_event: *mut lv::lv_event_t) {
    UNDERLAY_CLICKS.fetch_add(1, Ordering::AcqRel);
}

unsafe extern "C" fn interactive_clicked(_event: *mut lv::lv_event_t) {
    INTERACTIVE_CLICKS.fetch_add(1, Ordering::AcqRel);
}

unsafe extern "C" fn modal_clicked(_event: *mut lv::lv_event_t) {
    MODAL_CAPTURE_CLICKS.fetch_add(1, Ordering::AcqRel);
}

unsafe extern "C" fn modal_button_clicked(_event: *mut lv::lv_event_t) {
    MODAL_BUTTON_CLICKS.fetch_add(1, Ordering::AcqRel);
}

unsafe fn press(indev: *mut lv::lv_indev_t, x: i32, y: i32) {
    POINTER_X.store(x, Ordering::Release);
    POINTER_Y.store(y, Ordering::Release);
    POINTER_PRESSED.store(true, Ordering::Release);
    unsafe { lv::lv_indev_read(indev) };
}

unsafe fn release(indev: *mut lv::lv_indev_t) {
    POINTER_PRESSED.store(false, Ordering::Release);
    unsafe { lv::lv_indev_read(indev) };
}

unsafe fn click(indev: *mut lv::lv_indev_t, x: i32, y: i32) {
    unsafe { press(indev, x, y) };
    unsafe { release(indev) };
}

#[test]
fn passive_interactive_and_modal_overlays_route_pointer_input_by_contract() {
    unsafe {
        lv::lv_init();
        let display = lv::lv_display_create(320, 240);
        assert!(!display.is_null());
        let screen = lv::lv_screen_active();
        let system = lv::lv_layer_sys();
        let top = lv::lv_layer_top();
        assert!(!screen.is_null());
        assert!(!system.is_null());
        assert!(!top.is_null());
        let indev = lv::lv_indev_create();
        assert!(!indev.is_null());
        lv::lv_indev_set_type(indev, lv::lv_indev_type_t_LV_INDEV_TYPE_POINTER);
        lv::lv_indev_set_display(indev, display);
        lv::lv_indev_set_read_cb(indev, Some(read_pointer));
        UNDERLAY_CLICKS.store(0, Ordering::Release);
        INTERACTIVE_CLICKS.store(0, Ordering::Release);
        MODAL_CAPTURE_CLICKS.store(0, Ordering::Release);
        MODAL_BUTTON_CLICKS.store(0, Ordering::Release);

        let underlay = object(screen, 20, 20, 270, 80);
        assert!(!lv::lv_obj_add_event_cb(
            underlay,
            Some(underlay_clicked),
            lv::lv_event_code_t_LV_EVENT_CLICKED,
            ptr::null_mut(),
        )
        .is_null());
        let passive_root = object(system, 20, 20, 120, 80);
        let passive_child = object(passive_root, 0, 0, 120, 80);
        make_passive(passive_root);
        make_passive(passive_child);
        let interactive = object(system, 170, 20, 120, 80);
        assert!(!lv::lv_obj_add_event_cb(
            interactive,
            Some(interactive_clicked),
            lv::lv_event_code_t_LV_EVENT_CLICKED,
            ptr::null_mut(),
        )
        .is_null());
        lv::lv_obj_update_layout(screen);
        lv::lv_obj_update_layout(system);

        let mut point = lv::lv_point_t { x: 40, y: 40 };
        assert!(lv::lv_indev_search_obj(system, &mut point).is_null());
        assert!(lv::lv_indev_search_obj(top, &mut point).is_null());
        assert_eq!(lv::lv_indev_search_obj(screen, &mut point), underlay);
        assert_eq!(pointer_target(&mut point), underlay);
        click(indev, 40, 40);
        assert_eq!(UNDERLAY_CLICKS.load(Ordering::Acquire), 1);
        assert_eq!(INTERACTIVE_CLICKS.load(Ordering::Acquire), 0);
        assert_eq!(MODAL_CAPTURE_CLICKS.load(Ordering::Acquire), 0);
        assert_eq!(MODAL_BUTTON_CLICKS.load(Ordering::Acquire), 0);

        let mut interactive_point = lv::lv_point_t { x: 220, y: 40 };
        assert_eq!(pointer_target(&mut interactive_point), interactive);
        click(indev, 220, 40);
        assert_eq!(UNDERLAY_CLICKS.load(Ordering::Acquire), 1);
        assert_eq!(INTERACTIVE_CLICKS.load(Ordering::Acquire), 1);

        let modal_panel = object(system, 170, 90, 120, 100);
        let modal_button = object(modal_panel, 10, 10, 80, 60);
        assert!(!lv::lv_obj_add_event_cb(
            modal_button,
            Some(modal_button_clicked),
            lv::lv_event_code_t_LV_EVENT_CLICKED,
            ptr::null_mut(),
        )
        .is_null());
        lv::lv_obj_add_flag(system, lv::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE);
        assert!(!lv::lv_obj_add_event_cb(
            system,
            Some(modal_clicked),
            lv::lv_event_code_t_LV_EVENT_CLICKED,
            ptr::null_mut(),
        )
        .is_null());
        lv::lv_obj_update_layout(system);
        assert_eq!(lv::lv_indev_search_obj(system, &mut point), system);
        assert!(lv::lv_indev_search_obj(top, &mut point).is_null());
        assert_eq!(lv::lv_indev_search_obj(screen, &mut point), underlay);
        assert_eq!(pointer_target(&mut point), system);
        click(indev, 40, 40);
        assert_eq!(UNDERLAY_CLICKS.load(Ordering::Acquire), 1);
        assert_eq!(MODAL_CAPTURE_CLICKS.load(Ordering::Acquire), 1);
        assert_eq!(MODAL_BUTTON_CLICKS.load(Ordering::Acquire), 0);

        let mut button_point = lv::lv_point_t { x: 220, y: 130 };
        assert_eq!(
            lv::lv_indev_search_obj(system, &mut button_point),
            modal_button
        );
        click(indev, 220, 130);
        assert_eq!(UNDERLAY_CLICKS.load(Ordering::Acquire), 1);
        assert_eq!(MODAL_BUTTON_CLICKS.load(Ordering::Acquire), 1);

        press(indev, 220, 130);
        lv::lv_obj_delete(modal_panel);
        assert!(!lv::lv_obj_is_valid(modal_panel));
        lv::lv_obj_remove_flag(system, lv::lv_obj_flag_t_LV_OBJ_FLAG_CLICKABLE);
        release(indev);
        assert_eq!(MODAL_BUTTON_CLICKS.load(Ordering::Acquire), 1);
        assert_eq!(
            lv::lv_mem_test(),
            lv::lv_result_t_LV_RESULT_OK,
            "synchronous pressed-target deletion must preserve LVGL allocator integrity"
        );
        assert!(lv::lv_indev_search_obj(system, &mut point).is_null());
        click(indev, 40, 40);
        assert_eq!(UNDERLAY_CLICKS.load(Ordering::Acquire), 2);
        assert_eq!(INTERACTIVE_CLICKS.load(Ordering::Acquire), 1);
        assert_eq!(MODAL_CAPTURE_CLICKS.load(Ordering::Acquire), 1);
        assert_eq!(MODAL_BUTTON_CLICKS.load(Ordering::Acquire), 1);

        lv::lv_obj_delete(interactive);
        assert!(!lv::lv_obj_is_valid(interactive));
        assert_eq!(pointer_target(&mut interactive_point), underlay);
        click(indev, 220, 40);
        assert_eq!(UNDERLAY_CLICKS.load(Ordering::Acquire), 3);
        assert_eq!(INTERACTIVE_CLICKS.load(Ordering::Acquire), 1);

        lv::lv_display_delete(display);
        lv::lv_deinit();
    }
}
