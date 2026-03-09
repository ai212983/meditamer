use core::sync::atomic::{AtomicU32, Ordering};

use esp_println::println;
use esp_radio::wifi::{PromiscuousPkt, Sniffer};
use esp_wifi_sys::include::{
    esp_wifi_get_channel, esp_wifi_set_channel, wifi_promiscuous_pkt_type_t_WIFI_PKT_CTRL,
    wifi_promiscuous_pkt_type_t_WIFI_PKT_DATA, wifi_promiscuous_pkt_type_t_WIFI_PKT_MGMT,
    wifi_promiscuous_pkt_type_t_WIFI_PKT_MISC, wifi_second_chan_t,
    wifi_second_chan_t_WIFI_SECOND_CHAN_NONE,
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

fn on_packet(pkt: PromiscuousPkt<'_>) {
    TOTAL.fetch_add(1, Ordering::Relaxed);
    match pkt.frame_type {
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

pub fn run(sniffer: &mut Sniffer<'_>) {
    sniffer.set_receive_cb(on_packet);
    if let Err(err) = sniffer.set_promiscuous_mode(true) {
        println!("radio016_nostd_wifi_control: promisc_enable=err err={:?}", err);
        return;
    }

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
            "radio016_nostd_wifi_control: promisc_window channel={} dwell_us={} set_channel_rc={} total={} mgmt={} ctrl={} data={} misc={}",
            channel, DWELL_US, set_channel_rc, total, mgmt, ctrl, data, misc,
        );
    }

    let restore_rc = if get_channel_rc == 0 {
        unsafe { esp_wifi_set_channel(primary, second) }
    } else {
        -1
    };
    let disable_ok = sniffer.set_promiscuous_mode(false).is_ok();
    println!(
        "radio016_nostd_wifi_control: promisc_diag get_channel_rc={} restore_rc={} disable_ok={} total={} mgmt={} ctrl={} data={} misc={}",
        get_channel_rc, restore_rc, disable_ok, agg.0, agg.1, agg.2, agg.3, agg.4,
    );
}
