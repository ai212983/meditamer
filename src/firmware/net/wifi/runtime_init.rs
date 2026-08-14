use super::*;

fn wifi_setup_stage_trace_enabled() -> bool {
    match option_env!("MEDITAMER_WIFI_SETUP_STAGE_TRACE") {
        Some(raw) if raw != "0" => true,
        Some(_) => false,
        None => matches!(option_env!("WIFI_SETUP_STAGE_TRACE"), Some(raw) if raw != "0"),
    }
}

fn wifi_setup_stage_trace(stage: &str) {
    if wifi_setup_stage_trace_enabled() {
        println!("upload_http: wifi_setup_stage stage={stage}");
    }
}

fn wifi_setup_reinit_diag_enabled() -> bool {
    match option_env!("MEDITAMER_WIFI_SETUP_REINIT_DIAG") {
        Some(raw) if raw != "0" => true,
        Some(_) => false,
        None => matches!(option_env!("WIFI_SETUP_REINIT_DIAG"), Some(raw) if raw != "0"),
    }
}

fn wifi_precreate_timer_task_diag_enabled() -> bool {
    match option_env!("MEDITAMER_WIFI_PRECREATE_TIMER_TASK_DIAG") {
        Some(raw) if raw != "0" => true,
        Some(_) => false,
        None => matches!(
            option_env!("WIFI_PRECREATE_TIMER_TASK_DIAG"),
            Some(raw) if raw != "0"
        ),
    }
}

fn early_driver_state_diag_enabled() -> bool {
    match option_env!("MEDITAMER_WIFI_EARLY_DRIVER_STATE_DIAG") {
        Some(raw) if raw != "0" => true,
        Some(_) => false,
        None => matches!(option_env!("WIFI_EARLY_DRIVER_STATE_DIAG"), Some(raw) if raw != "0"),
    }
}

fn wifi_force_storage_ram_enabled() -> bool {
    match option_env!("MEDITAMER_WIFI_FORCE_STORAGE_RAM") {
        Some(raw) if raw != "0" => true,
        Some(_) => false,
        None => matches!(option_env!("WIFI_FORCE_STORAGE_RAM"), Some(raw) if raw != "0"),
    }
}

fn country_us_override_enabled() -> bool {
    option_env!("MEDITAMER_WIFI_COUNTRY_US_OVERRIDE")
        .or(option_env!("WIFI_COUNTRY_US_OVERRIDE"))
        .is_some_and(|raw| raw != "0")
}

fn maybe_apply_wifi_storage_override() -> (&'static str, i32) {
    if !wifi_force_storage_ram_enabled() {
        return ("default", i32::MIN);
    }
    let rc = unsafe {
        esp_wifi_sys::include::esp_wifi_set_storage(
            esp_wifi_sys::include::wifi_storage_t_WIFI_STORAGE_RAM,
        )
    };
    ("ram", rc)
}

fn maybe_log_runtime_setup_driver_state(
    country_us_override: bool,
    storage_mode: &'static str,
    storage_rc: i32,
) {
    if !early_driver_state_diag_enabled() {
        return;
    }

    let mut mode = 0u32;
    let mode_rc = unsafe { esp_wifi_sys::include::esp_wifi_get_mode(&mut mode as *mut u32) };
    let mut ps = 0u32;
    let ps_rc = unsafe { esp_wifi_sys::include::esp_wifi_get_ps(&mut ps as *mut u32) };
    let mut protocol_bitmap = 0u8;
    let protocol_rc = unsafe {
        esp_wifi_sys::include::esp_wifi_get_protocol(
            esp_wifi_sys::include::wifi_interface_t_WIFI_IF_STA,
            &mut protocol_bitmap as *mut u8,
        )
    };
    let mut event_mask = 0u32;
    let event_mask_rc =
        unsafe { esp_wifi_sys::include::esp_wifi_get_event_mask(&mut event_mask as *mut u32) };
    let mut country = core::mem::MaybeUninit::<esp_wifi_sys::include::wifi_country_t>::uninit();
    let country_rc = unsafe { esp_wifi_sys::include::esp_wifi_get_country(country.as_mut_ptr()) };
    let (cc0, cc1, schan, nchan) = if country_rc == esp_wifi_sys::include::ESP_OK as i32 {
        let country = unsafe { country.assume_init() };
        (
            country.cc[0] as char,
            country.cc[1] as char,
            country.schan,
            country.nchan,
        )
    } else {
        ('.', '.', 0, 0)
    };
    println!(
        "upload_http: runtime_setup_driver_state country_us_override={} storage_mode={} storage_rc={} mode_rc={} mode={} ps_rc={} ps={} protocol_rc={} protocol_bitmap=0x{:02x} event_mask_rc={} event_mask=0x{:08x} country_rc={} cc={}{} schan={} nchan={}",
        country_us_override,
        storage_mode,
        storage_rc,
        mode_rc,
        mode,
        ps_rc,
        ps,
        protocol_rc,
        protocol_bitmap,
        event_mask_rc,
        event_mask,
        country_rc,
        cc0,
        cc1,
        schan,
        nchan,
    );
}

pub(crate) fn apply_runtime_setup_overrides_and_log() {
    let (storage_mode, storage_rc) = maybe_apply_wifi_storage_override();
    if storage_mode != "default" {
        println!(
            "upload_http: wifi_storage_override mode={} rc={}",
            storage_mode, storage_rc
        );
    }
    maybe_log_runtime_setup_driver_state(country_us_override_enabled(), storage_mode, storage_rc);
}

pub(crate) fn initialize_runtime_sta<'d>(
    wifi: esp_hal::peripherals::WIFI<'d>,
) -> Result<(WifiController<'d>, WifiDevice), &'static str> {
    if wifi_setup_reinit_diag_enabled() || wifi_precreate_timer_task_diag_enabled() {
        println!("upload_http: legacy wifi reinit/timer diagnostics unavailable on esp-radio 1.0");
    }
    wifi_setup_stage_trace("esp_radio_wifi_new.before");
    let result = super::backend::initialize_runtime_sta(wifi, country_us_override_enabled());
    wifi_setup_stage_trace("esp_radio_wifi_new.after");
    result
}
