use crate::binary::c_types;

unsafe extern "C" {
    fn cnx_connect_timeout(arg: *mut c_types::c_void);
    fn nan_dp_schedule_ndc_start();
    fn chm_mhz2num();
}

pub(crate) fn compat_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_ESP_RADIO_USE_LEGACY_TIMER_COMPAT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("ESP_RADIO_USE_LEGACY_TIMER_COMPAT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("MEDITAMER_WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_BACKEND_LEGACY_PORT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn suppress_connect_timeout_arm_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_BACKEND_LEGACY_PORT_SUPPRESS_CONNECT_TIMEOUT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_BACKEND_LEGACY_PORT_SUPPRESS_CONNECT_TIMEOUT_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn suppress_nan_dp_timer_arm_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_SUPPRESS_NAN_DP_TIMER_ARM_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_SUPPRESS_NAN_DP_TIMER_ARM_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn suppress_nan_dp_timer_arg1_arm_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_SUPPRESS_NAN_DP_TIMER_ARG1_ARM_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_SUPPRESS_NAN_DP_TIMER_ARG1_ARM_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn suppress_nan_dp_timer_arg0_arm_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_SUPPRESS_NAN_DP_TIMER_ARG0_ARM_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_SUPPRESS_NAN_DP_TIMER_ARG0_ARM_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn suppress_nan_dp_timer_arg1_setfn_enabled() -> bool {
    matches!(
        option_env!("MEDITAMER_WIFI_SUPPRESS_NAN_DP_TIMER_ARG1_SETFN_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    ) || matches!(
        option_env!("WIFI_SUPPRESS_NAN_DP_TIMER_ARG1_SETFN_DIAG"),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn callback_is_nan_dp_timer_family(callback_ptr: usize) -> bool {
    if callback_ptr == 0 {
        return false;
    }
    let start = nan_dp_schedule_ndc_start as usize;
    let end = chm_mhz2num as usize;
    callback_ptr >= start && callback_ptr < end
}

pub(crate) fn should_suppress_callback_arm(callback_ptr: usize, arg_ptr: usize) -> bool {
    if suppress_connect_timeout_arm_enabled() && callback_ptr == cnx_connect_timeout as usize {
        return true;
    }

    if !callback_is_nan_dp_timer_family(callback_ptr) {
        return false;
    }

    if suppress_nan_dp_timer_arm_enabled() {
        return true;
    }
    if suppress_nan_dp_timer_arg1_arm_enabled() && arg_ptr == 1 {
        return true;
    }
    suppress_nan_dp_timer_arg0_arm_enabled() && arg_ptr == 0
}

pub(crate) fn should_suppress_callback_setfn(callback_ptr: usize, arg_ptr: usize) -> bool {
    suppress_nan_dp_timer_arg1_setfn_enabled()
        && callback_is_nan_dp_timer_family(callback_ptr)
        && arg_ptr == 1
}
