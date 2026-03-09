use super::backend_legacy_port;
use super::*;
use static_cell::StaticCell;

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

pub(crate) fn initialize_runtime_sta(
    wifi: esp_hal::peripherals::WIFI<'static>,
) -> Result<(WifiController<'static>, WifiDevice<'static>), &'static str> {
    if backend_legacy_port::legacy_port_runtime_enabled() {
        return backend_legacy_port::initialize_runtime_sta_legacy_port(
            wifi,
            country_us_override_enabled(),
        );
    }

    static RADIO_CTRL: StaticCell<RadioController> = StaticCell::new();

    wifi_setup_stage_trace("esp_radio_init.before");
    let radio_ctrl = match init_radio() {
        Ok(ctrl) => ctrl,
        Err(err) => {
            println!("asset-upload-http: esp_radio::init err={:?}", err);
            return Err("asset-upload-http: esp_radio::init failed");
        }
    };
    wifi_setup_stage_trace("esp_radio_init.after");

    if wifi_precreate_timer_task_diag_enabled() {
        esp_rtos::precreate_esp_radio_timer_task();
        esp_rtos::yield_for_esp_radio_diag();
        println!("upload_http: wifi_precreate_timer_task_diag result=ok");
    }

    let radio_ctrl = RADIO_CTRL.init(radio_ctrl);
    if wifi_setup_reinit_diag_enabled() {
        println!("upload_http: wifi_setup_reinit_diag phase=first_init begin=true");
        wifi_setup_stage_trace("esp_radio_wifi_new.first.before");
        let (first_controller, first_ifaces) =
            match new_runtime(radio_ctrl, wifi, wifi_runtime_config()) {
                Ok(parts) => parts,
                Err(err) => {
                    println!("asset-upload-http: wifi init err={:?}", err);
                    return Err("asset-upload-http: wifi init failed");
                }
            };
        wifi_setup_stage_trace("esp_radio_wifi_new.first.after");
        println!("upload_http: wifi_setup_reinit_diag phase=first_init result=ok");
        drop(first_ifaces);
        drop(first_controller);
        println!("upload_http: wifi_setup_reinit_diag phase=drop result=ok");

        wifi_setup_stage_trace("esp_radio_wifi_new.second.before");
        match new_runtime(
            radio_ctrl,
            unsafe { esp_hal::peripherals::WIFI::steal() },
            wifi_runtime_config(),
        ) {
            Ok((controller, ifaces)) => {
                wifi_setup_stage_trace("esp_radio_wifi_new.second.after");
                println!("upload_http: wifi_setup_reinit_diag phase=second_init result=ok");
                Ok((controller, ifaces.sta))
            }
            Err(err) => {
                println!("asset-upload-http: wifi reinit err={:?}", err);
                Err("asset-upload-http: wifi reinit failed")
            }
        }
    } else {
        wifi_setup_stage_trace("esp_radio_wifi_new.before");
        match new_runtime(radio_ctrl, wifi, wifi_runtime_config()) {
            Ok((controller, ifaces)) => {
                wifi_setup_stage_trace("esp_radio_wifi_new.after");
                Ok((controller, ifaces.sta))
            }
            Err(err) => {
                println!("asset-upload-http: wifi init err={:?}", err);
                Err("asset-upload-http: wifi init failed")
            }
        }
    }
}
