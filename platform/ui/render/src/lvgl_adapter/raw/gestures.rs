//! LVGL input and gesture callbacks for the checked raw gateway.

use super::*;

#[cfg(feature = "lvgl-gestures")]
pub(super) fn delete_input(input: *mut lv::lv_indev_t) {
    // SAFETY: callers pass either a newly created unpublished input or the
    // matching epoch's sole registered input after unpublishing it.
    unsafe { lv::lv_indev_delete(input) };
}

#[cfg(feature = "lvgl-gestures")]
pub(super) unsafe extern "C" fn input_callback(
    input: *mut lv::lv_indev_t,
    data: *mut lv::lv_indev_data_t,
) {
    // SAFETY: LVGL supplies writable data for the callback duration.
    let Some(data) = (unsafe { data.as_mut() }) else {
        return;
    };
    let runtime_id = ACTIVE_INPUT_RUNTIME_ID.load(Ordering::Acquire);
    if runtime_id == 0 || ACTIVE_INPUT.load(Ordering::Acquire) != input {
        return;
    }
    match super::super::runtime_io::pointer_sample(runtime_id).kind {
        PointerSampleKind::Single { point, state } => {
            data.point.x = point.0;
            data.point.y = point.1;
            data.state = match state {
                PointerState::Released => lv::lv_indev_state_t_LV_INDEV_STATE_RELEASED,
                PointerState::Pressed => lv::lv_indev_state_t_LV_INDEV_STATE_PRESSED,
            };
        }
        PointerSampleKind::Multi { contacts } => {
            let empty = lv::lv_indev_touch_data_t {
                point: lv::lv_point_t { x: 0, y: 0 },
                state: lv::lv_indev_state_t_LV_INDEV_STATE_RELEASED,
                id: 0,
                timestamp: 0,
            };
            let mut touches = [empty; 4];
            let mut count = 0usize;
            let mut primary = None;
            for contact in contacts.into_iter().flatten() {
                touches[count] = lv::lv_indev_touch_data_t {
                    point: lv::lv_point_t {
                        x: contact.point.0,
                        y: contact.point.1,
                    },
                    state: match contact.state {
                        PointerState::Released => lv::lv_indev_state_t_LV_INDEV_STATE_RELEASED,
                        PointerState::Pressed => lv::lv_indev_state_t_LV_INDEV_STATE_PRESSED,
                    },
                    id: contact.id,
                    timestamp: contact.timestamp,
                };
                if primary.is_none() && contact.state == PointerState::Pressed {
                    primary = Some(contact.point);
                }
                count += 1;
            }
            // SAFETY: touches contains count initialized entries, input/data
            // are the pointers LVGL supplied to this registered callback.
            unsafe {
                lv::lv_indev_gesture_recognizers_update(input, touches.as_mut_ptr(), count as u16);
                lv::lv_indev_gesture_recognizers_set_data(input, data);
            }
            if let Some(primary) = primary {
                data.point.x = primary.0;
                data.point.y = primary.1;
            }
        }
    }
}

#[cfg(feature = "lvgl-gestures")]
pub(super) unsafe extern "C" fn gesture_callback(event: *mut lv::lv_event_t) {
    if event.is_null() {
        return;
    }
    let registered_input = ACTIVE_INPUT.load(Ordering::Acquire);
    let runtime_id = ACTIVE_INPUT_RUNTIME_ID.load(Ordering::Acquire);
    // SAFETY: event is non-null and valid for this LVGL callback.
    let event_param = unsafe { lv::lv_event_get_param(event) };
    let current_target = unsafe { lv::lv_event_get_current_target(event) };
    if registered_input.is_null()
        || event_param != registered_input.cast()
        || current_target != registered_input.cast()
    {
        return;
    }

    // SAFETY: target/parameter validation above proves this is a recognizer
    // event for the registered input rather than a legacy object gesture.
    let gesture_type = unsafe { lv::lv_event_get_gesture_type(event) };
    if !matches!(
        gesture_type,
        lv::lv_indev_gesture_type_t_LV_INDEV_GESTURE_PINCH
            | lv::lv_indev_gesture_type_t_LV_INDEV_GESTURE_ROTATE
            | lv::lv_indev_gesture_type_t_LV_INDEV_GESTURE_TWO_FINGERS_SWIPE
    ) {
        return;
    }
    // SAFETY: gesture_type is one of the supported initialized recognizers.
    if unsafe { lv::lv_event_get_gesture_state(event, gesture_type) }
        != lv::lv_indev_gesture_state_t_LV_INDEV_GESTURE_STATE_ENDED
    {
        return;
    }
    let decoded = match gesture_type {
        lv::lv_indev_gesture_type_t_LV_INDEV_GESTURE_PINCH => GestureEvent::Pinch {
            // SAFETY: validated completed pinch recognizer event.
            scale: unsafe { lv::lv_event_get_pinch_scale(event) },
        },
        lv::lv_indev_gesture_type_t_LV_INDEV_GESTURE_ROTATE => GestureEvent::Rotation {
            // SAFETY: validated completed rotation recognizer event.
            radians: unsafe { lv::lv_event_get_rotation(event) },
        },
        lv::lv_indev_gesture_type_t_LV_INDEV_GESTURE_TWO_FINGERS_SWIPE => {
            // SAFETY: validated completed two-finger swipe recognizer event.
            let direction = unsafe { lv::lv_event_get_two_fingers_swipe_dir(event) };
            let direction = match direction {
                lv::lv_dir_t_LV_DIR_LEFT => GestureDirection::Left,
                lv::lv_dir_t_LV_DIR_RIGHT => GestureDirection::Right,
                lv::lv_dir_t_LV_DIR_TOP => GestureDirection::Up,
                lv::lv_dir_t_LV_DIR_BOTTOM => GestureDirection::Down,
                _ => GestureDirection::Unknown,
            };
            GestureEvent::TwoFingerSwipe {
                direction,
                // SAFETY: validated completed two-finger swipe recognizer event.
                distance_px: unsafe { lv::lv_event_get_two_fingers_swipe_distance(event) },
            }
        }
        _ => return,
    };
    super::super::runtime_io::emit_gesture(runtime_id, decoded);
}
