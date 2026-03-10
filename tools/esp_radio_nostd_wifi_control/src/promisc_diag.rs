use core::sync::atomic::{AtomicU32, Ordering};

use esp_println::println;
use esp_wifi_sys::include::{
    esp_wifi_get_promiscuous, esp_wifi_set_promiscuous, esp_wifi_set_promiscuous_filter,
    esp_wifi_set_promiscuous_rx_cb,
    esp_wifi_get_channel, esp_wifi_set_channel, wifi_promiscuous_pkt_type_t_WIFI_PKT_CTRL,
    wifi_promiscuous_pkt_type_t_WIFI_PKT_DATA, wifi_promiscuous_pkt_type_t_WIFI_PKT_MGMT,
    wifi_promiscuous_pkt_type_t_WIFI_PKT_MISC, wifi_second_chan_t,
    wifi_second_chan_t_WIFI_SECOND_CHAN_NONE, wifi_promiscuous_filter_t,
    WIFI_PROMIS_FILTER_MASK_CTRL, WIFI_PROMIS_FILTER_MASK_DATA, WIFI_PROMIS_FILTER_MASK_MGMT,
};

unsafe extern "C" {
    fn esp_rom_delay_us(us: u32);
}

const CHANNELS: [u8; 4] = [8, 1, 6, 11];
const DWELL_US: u32 = 120_000;
const WIFI_PKT_MGMT: u32 = wifi_promiscuous_pkt_type_t_WIFI_PKT_MGMT;
const WIFI_PKT_CTRL: u32 = wifi_promiscuous_pkt_type_t_WIFI_PKT_CTRL;
const WIFI_PKT_DATA: u32 = wifi_promiscuous_pkt_type_t_WIFI_PKT_DATA;
const WIFI_PKT_MISC: u32 = wifi_promiscuous_pkt_type_t_WIFI_PKT_MISC;

static TOTAL: AtomicU32 = AtomicU32::new(0);
static MGMT: AtomicU32 = AtomicU32::new(0);
static CTRL: AtomicU32 = AtomicU32::new(0);
static DATA: AtomicU32 = AtomicU32::new(0);
static MISC: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn on_packet(
    _buf: *mut esp_wifi_sys::c_types::c_void,
    pkt_type: esp_wifi_sys::include::wifi_promiscuous_pkt_type_t,
) {
    TOTAL.fetch_add(1, Ordering::Relaxed);
    match pkt_type {
        WIFI_PKT_MGMT => MGMT.fetch_add(1, Ordering::Relaxed),
        WIFI_PKT_CTRL => CTRL.fetch_add(1, Ordering::Relaxed),
        WIFI_PKT_DATA => DATA.fetch_add(1, Ordering::Relaxed),
        WIFI_PKT_MISC => MISC.fetch_add(1, Ordering::Relaxed),
        _ => 0,
    };
}

fn reset_counts() {
    for counter in [&TOTAL, &MGMT, &CTRL, &DATA, &MISC] {
        counter.store(0, Ordering::Relaxed);
    }
}

fn counts() -> (u32, u32, u32, u32, u32) {
    (
        TOTAL.load(Ordering::Relaxed),
        MGMT.load(Ordering::Relaxed),
        CTRL.load(Ordering::Relaxed),
        DATA.load(Ordering::Relaxed),
        MISC.load(Ordering::Relaxed),
    )
}

fn run_windows(label: &str) -> (u32, u32, u32, u32, u32) {
    let mut primary = 0u8;
    let mut second = wifi_second_chan_t_WIFI_SECOND_CHAN_NONE;
    let get_channel_rc =
        unsafe { esp_wifi_get_channel(&mut primary as *mut u8, &mut second as *mut wifi_second_chan_t) };

    let mut agg = (0u32, 0u32, 0u32, 0u32, 0u32);
    for channel in CHANNELS {
        let set_channel_rc =
            unsafe { esp_wifi_set_channel(channel, wifi_second_chan_t_WIFI_SECOND_CHAN_NONE) };
        reset_counts();
        unsafe { esp_rom_delay_us(DWELL_US) };
        let (total, mgmt, ctrl, data, misc) = counts();
        agg.0 += total;
        agg.1 += mgmt;
        agg.2 += ctrl;
        agg.3 += data;
        agg.4 += misc;
        println!(
            "nostd_wifi_control: promisc_window label={} channel={} dwell_us={} set_channel_rc={} total={} mgmt={} ctrl={} data={} misc={}",
            label, channel, DWELL_US, set_channel_rc, total, mgmt, ctrl, data, misc,
        );
    }

    let restore_rc = if get_channel_rc == 0 {
        unsafe { esp_wifi_set_channel(primary, second) }
    } else {
        -1
    };
    println!(
        "nostd_wifi_control: promisc_window_restore label={} get_channel_rc={} restore_rc={}",
        label, get_channel_rc, restore_rc
    );
    agg
}

pub fn run() {
    println!("nostd_wifi_control: promisc_enter=true");
    let mut was_enabled = false;
    let get_before_rc = unsafe { esp_wifi_get_promiscuous(&mut was_enabled as *mut bool) };
    println!(
        "nostd_wifi_control: promisc_get_before rc={} was_enabled={}",
        get_before_rc, was_enabled,
    );
    if get_before_rc != 0 {
        println!(
            "nostd_wifi_control: promisc_enable=err get_before_rc={}",
            get_before_rc,
        );
        return;
    }
    if was_enabled {
        println!("nostd_wifi_control: promisc_enable=skip already_enabled=true");
        return;
    }

    let cb_rc = unsafe { esp_wifi_set_promiscuous_rx_cb(Some(on_packet)) };
    let filter = wifi_promiscuous_filter_t {
        filter_mask: WIFI_PROMIS_FILTER_MASK_MGMT
            | WIFI_PROMIS_FILTER_MASK_CTRL
            | WIFI_PROMIS_FILTER_MASK_DATA,
    };
    let filter_rc = unsafe { esp_wifi_set_promiscuous_filter(&filter) };
    let enable_rc = unsafe { esp_wifi_set_promiscuous(true) };
    let mut enabled_after = false;
    let get_after_rc = unsafe { esp_wifi_get_promiscuous(&mut enabled_after as *mut bool) };
    println!(
        "nostd_wifi_control: promisc_enable_attempt cb_rc={} filter_rc={} enable_rc={} get_after_rc={} enabled_after={}",
        cb_rc, filter_rc, enable_rc, get_after_rc, enabled_after,
    );
    if cb_rc != 0 || filter_rc != 0 || enable_rc != 0 {
        let disable_rc = unsafe { esp_wifi_set_promiscuous(false) };
        let clear_cb_rc = unsafe { esp_wifi_set_promiscuous_rx_cb(None) };
        println!(
            "nostd_wifi_control: promisc_enable=err cb_rc={} filter_rc={} enable_rc={} disable_rc={} clear_cb_rc={}",
            cb_rc, filter_rc, enable_rc, disable_rc, clear_cb_rc,
        );
        return;
    }

    let mut agg = run_windows("filtered");
    if agg.0 == 0 {
        let all_filter = wifi_promiscuous_filter_t { filter_mask: u32::MAX };
        let all_filter_rc = unsafe { esp_wifi_set_promiscuous_filter(&all_filter) };
        println!(
            "nostd_wifi_control: promisc_filter_retry filter_rc={} mask=0xffffffff",
            all_filter_rc
        );
        agg = run_windows("all_bits");
    }
    let disable_rc = unsafe { esp_wifi_set_promiscuous(false) };
    let clear_cb_rc = unsafe { esp_wifi_set_promiscuous_rx_cb(None) };
    let disable_ok = disable_rc == 0 && clear_cb_rc == 0;
    println!(
        "nostd_wifi_control: promisc_diag disable_ok={} disable_rc={} clear_cb_rc={} total={} mgmt={} ctrl={} data={} misc={}",
        disable_ok, disable_rc, clear_cb_rc, agg.0, agg.1, agg.2, agg.3, agg.4,
    );
}
