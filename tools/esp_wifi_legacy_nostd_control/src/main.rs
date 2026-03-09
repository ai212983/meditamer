#![no_std]
#![no_main]

use core::sync::atomic::{AtomicU32, Ordering};

use esp_backtrace as _;
use esp_hal::{rng::Rng, timer::timg::TimerGroup};
use esp_println::println;
use esp_wifi::wifi::{PromiscuousPkt, WifiMode};
use esp_wifi_sys::include::{
    esp_wifi_get_channel, esp_wifi_set_channel, wifi_promiscuous_pkt_type_t_WIFI_PKT_CTRL,
    wifi_promiscuous_pkt_type_t_WIFI_PKT_DATA, wifi_promiscuous_pkt_type_t_WIFI_PKT_MGMT,
    wifi_promiscuous_pkt_type_t_WIFI_PKT_MISC, wifi_second_chan_t,
    wifi_second_chan_t_WIFI_SECOND_CHAN_NONE,
};
unsafe extern "C" {
    fn esp_rom_delay_us(us: u32);
}

const PROMISC_CHANNELS: [u8; 4] = [8, 1, 6, 11];
const PROMISC_DWELL_US: u32 = 120_000;
const WIFI_PKT_MGMT: u32 = wifi_promiscuous_pkt_type_t_WIFI_PKT_MGMT;
const WIFI_PKT_CTRL: u32 = wifi_promiscuous_pkt_type_t_WIFI_PKT_CTRL;
const WIFI_PKT_DATA: u32 = wifi_promiscuous_pkt_type_t_WIFI_PKT_DATA;
const WIFI_PKT_MISC: u32 = wifi_promiscuous_pkt_type_t_WIFI_PKT_MISC;

static PROMISC_TOTAL: AtomicU32 = AtomicU32::new(0);
static PROMISC_MGMT: AtomicU32 = AtomicU32::new(0);
static PROMISC_CTRL: AtomicU32 = AtomicU32::new(0);
static PROMISC_DATA: AtomicU32 = AtomicU32::new(0);
static PROMISC_MISC: AtomicU32 = AtomicU32::new(0);

fn format_legacy_task_role(task_ptr: usize, main_task_ptr: usize) -> &'static str {
    if task_ptr == 0 {
        return "none";
    }
    if task_ptr == main_task_ptr {
        "main"
    } else {
        "other"
    }
}

fn promisc_rx(pkt: PromiscuousPkt<'_>) {
    PROMISC_TOTAL.fetch_add(1, Ordering::Relaxed);
    match pkt.frame_type {
        WIFI_PKT_MGMT => {
            PROMISC_MGMT.fetch_add(1, Ordering::Relaxed);
        }
        WIFI_PKT_CTRL => {
            PROMISC_CTRL.fetch_add(1, Ordering::Relaxed);
        }
        WIFI_PKT_DATA => {
            PROMISC_DATA.fetch_add(1, Ordering::Relaxed);
        }
        WIFI_PKT_MISC => {
            PROMISC_MISC.fetch_add(1, Ordering::Relaxed);
        }
        _ => {}
    }
}

fn reset_promisc_counts() {
    for counter in [&PROMISC_TOTAL, &PROMISC_MGMT, &PROMISC_CTRL, &PROMISC_DATA, &PROMISC_MISC] {
        counter.store(0, Ordering::Relaxed);
    }
}

fn read_promisc_counts() -> (u32, u32, u32, u32, u32) {
    (
        PROMISC_TOTAL.load(Ordering::Relaxed),
        PROMISC_MGMT.load(Ordering::Relaxed),
        PROMISC_CTRL.load(Ordering::Relaxed),
        PROMISC_DATA.load(Ordering::Relaxed),
        PROMISC_MISC.load(Ordering::Relaxed),
    )
}

fn run_promisc_diag(sniffer: &mut esp_wifi::wifi::Sniffer) {
    sniffer.set_receive_cb(promisc_rx);
    if let Err(err) = sniffer.set_promiscuous_mode(true) {
        println!("legacy_nostd_wifi_control: promisc_enable=err err={:?}", err);
        return;
    }

    let mut orig_primary = 0u8;
    let mut orig_second = wifi_second_chan_t_WIFI_SECOND_CHAN_NONE;
    let get_channel_rc = unsafe {
        esp_wifi_get_channel(
            &mut orig_primary as *mut u8,
            &mut orig_second as *mut wifi_second_chan_t,
        )
    };

    let mut agg_total = 0u32;
    let mut agg_mgmt = 0u32;
    let mut agg_ctrl = 0u32;
    let mut agg_data = 0u32;
    let mut agg_misc = 0u32;
    for channel in PROMISC_CHANNELS {
        let set_channel_rc = unsafe {
            esp_wifi_set_channel(channel, wifi_second_chan_t_WIFI_SECOND_CHAN_NONE)
        };
        reset_promisc_counts();
        unsafe { esp_rom_delay_us(PROMISC_DWELL_US) };
        let (total, mgmt, ctrl, data, misc) = read_promisc_counts();
        agg_total = agg_total.saturating_add(total);
        agg_mgmt = agg_mgmt.saturating_add(mgmt);
        agg_ctrl = agg_ctrl.saturating_add(ctrl);
        agg_data = agg_data.saturating_add(data);
        agg_misc = agg_misc.saturating_add(misc);
        println!(
            "legacy_nostd_wifi_control: promisc_window channel={} dwell_us={} set_channel_rc={} total={} mgmt={} ctrl={} data={} misc={}",
            channel, PROMISC_DWELL_US, set_channel_rc, total, mgmt, ctrl, data, misc,
        );
    }

    let restore_rc = if get_channel_rc == 0 {
        unsafe { esp_wifi_set_channel(orig_primary, orig_second) }
    } else {
        -1
    };
    let disable = sniffer.set_promiscuous_mode(false);
    println!(
        "legacy_nostd_wifi_control: promisc_diag get_channel_rc={} restore_rc={} disable_ok={} total={} mgmt={} ctrl={} data={} misc={}",
        get_channel_rc,
        restore_rc,
        disable.is_ok(),
        agg_total,
        agg_mgmt,
        agg_ctrl,
        agg_data,
        agg_misc,
    );
}

fn print_legacy_diag(label: &str) {
    let send_count = esp_wifi::diagnostic_queue_send_count();
    let recv_count = esp_wifi::diagnostic_queue_recv_count();
    let current_task_ptr = esp_wifi::diagnostic_current_task_ptr();
    let thread_sem_ptr = esp_wifi::diagnostic_thread_sem_ptr();
    println!(
        "legacy_nostd_wifi_control: queue_diag label={} send_count={} recv_count={} current_task_ptr=0x{:08x} thread_sem_ptr=0x{:08x} thread_sem_get_count={} thread_sem_first_ptr=0x{:08x} thread_sem_last_ptr=0x{:08x} thread_sem_ptr_change_count={} thread_sem_first_task_ptr=0x{:08x} thread_sem_last_task_ptr=0x{:08x} thread_sem_task_change_count={} task_get_current_count={} task_get_current_first_ptr=0x{:08x} task_get_current_last_ptr=0x{:08x} task_get_current_change_count={}",
        label,
        send_count,
        recv_count,
        current_task_ptr,
        thread_sem_ptr,
        esp_wifi::diagnostic_thread_sem_get_count(),
        esp_wifi::diagnostic_thread_sem_first_ptr(),
        esp_wifi::diagnostic_thread_sem_last_ptr(),
        esp_wifi::diagnostic_thread_sem_ptr_change_count(),
        esp_wifi::diagnostic_thread_sem_first_task_ptr(),
        esp_wifi::diagnostic_thread_sem_last_task_ptr(),
        esp_wifi::diagnostic_thread_sem_task_change_count(),
        esp_wifi::diagnostic_task_get_current_count(),
        esp_wifi::diagnostic_task_get_current_first_ptr(),
        esp_wifi::diagnostic_task_get_current_last_ptr(),
        esp_wifi::diagnostic_task_get_current_change_count(),
    );
    for idx in 0..8 {
        let (ordinal, task_ptr) = esp_wifi::diagnostic_task_get_current_recent(idx);
        if ordinal == 0 && task_ptr == 0 {
            continue;
        }
        println!(
            "legacy_nostd_wifi_control: task_get_current_recent label={} idx={} ordinal={} task_ptr=0x{:08x} task_role={}",
            label,
            idx,
            ordinal,
            task_ptr,
            format_legacy_task_role(task_ptr, current_task_ptr),
        );
    }
    for idx in 0..8 {
        let (ordinal, item_word0, pointee_word0, pointee_word1) =
            esp_wifi::diagnostic_queue_send_recent(idx);
        if ordinal != 0 {
            println!(
                "legacy_nostd_wifi_control: queue_send_recent label={} idx={} ordinal={} item_word0=0x{:08x} pointee_word0=0x{:08x} pointee_word1=0x{:08x}",
                label,
                idx,
                ordinal,
                item_word0,
                pointee_word0,
                pointee_word1,
            );
        }
    }
    for idx in 0..8 {
        let (ordinal, item_word0, pointee_word0, pointee_word1) =
            esp_wifi::diagnostic_queue_recv_recent(idx);
        if ordinal != 0 {
            println!(
                "legacy_nostd_wifi_control: queue_recv_recent label={} idx={} ordinal={} item_word0=0x{:08x} pointee_word0=0x{:08x} pointee_word1=0x{:08x}",
                label,
                idx,
                ordinal,
                item_word0,
                pointee_word0,
                pointee_word1,
            );
        }
    }
}

fn print_wifi_mac_isr_diag(label: &str) {
    println!(
        "legacy_nostd_wifi_control: wifi_mac_isr_diag label={} count={}",
        label,
        esp_wifi::diagnostic_wifi_mac_isr_count(),
    );
}

fn print_wifi_init_config_diag(label: &str) {
    let diag = esp_wifi::diagnostic_wifi_init_config_diag();
    println!(
        "legacy_nostd_wifi_control: wifi_init_config_diag label={} config_ptr=0x{:08x} osi_funcs_ptr=0x{:08x} static_rx_buf_num={} dynamic_rx_buf_num={} static_tx_buf_num={} dynamic_tx_buf_num={} rx_mgmt_buf_type={} rx_mgmt_buf_num={} cache_tx_buf_num={} ampdu_rx_enable={} ampdu_tx_enable={} amsdu_tx_enable={} nvs_enable={} nano_enable={} rx_ba_win={} wifi_task_core_id={} feature_caps=0x{:016x} sta_disconnected_pm={} tx_hetb_queue_num={} dump_hesigb_enable={} magic=0x{:08x}",
        label,
        diag.config_ptr,
        diag.osi_funcs_ptr,
        diag.static_rx_buf_num,
        diag.dynamic_rx_buf_num,
        diag.static_tx_buf_num,
        diag.dynamic_tx_buf_num,
        diag.rx_mgmt_buf_type,
        diag.rx_mgmt_buf_num,
        diag.cache_tx_buf_num,
        diag.ampdu_rx_enable,
        diag.ampdu_tx_enable,
        diag.amsdu_tx_enable,
        diag.nvs_enable,
        diag.nano_enable,
        diag.rx_ba_win,
        diag.wifi_task_core_id,
        diag.feature_caps,
        diag.sta_disconnected_pm,
        diag.tx_hetb_queue_num,
        diag.dump_hesigb_enable,
        diag.magic,
    );
    println!(
        "legacy_nostd_wifi_control: wifi_osi_diag label={} set_isr=0x{:08x} queue_create=0x{:08x} queue_recv=0x{:08x} task_create=0x{:08x} task_create_pinned=0x{:08x} task_get_current=0x{:08x} wifi_thread_semphr_get=0x{:08x} timer_arm_us=0x{:08x} event_post=0x{:08x} malloc_internal=0x{:08x}",
        label,
        diag.osi_set_isr_ptr,
        diag.osi_queue_create_ptr,
        diag.osi_queue_recv_ptr,
        diag.osi_task_create_ptr,
        diag.osi_task_create_pinned_ptr,
        diag.osi_task_get_current_ptr,
        diag.osi_wifi_thread_semphr_get_ptr,
        diag.osi_timer_arm_us_ptr,
        diag.osi_event_post_ptr,
        diag.osi_malloc_internal_ptr,
    );
}

fn print_wifi_task_create_diag(label: &str) {
    let diag = esp_wifi::diagnostic_task_create_diag();
    println!(
        "legacy_nostd_wifi_control: wifi_task_create_diag label={} count={}",
        label, diag.count,
    );
    for idx in 0..diag.recent_ordinals.len() {
        let ordinal = diag.recent_ordinals[idx];
        if ordinal == 0 {
            continue;
        }
        println!(
            "legacy_nostd_wifi_control: wifi_task_create_recent label={} idx={} ordinal={} task_func_ptr=0x{:08x} name_tag=0x{:08x} name_len={} stack_depth={} param_ptr=0x{:08x} prio={} core_id={} task_ptr=0x{:08x}",
            label,
            idx,
            ordinal,
            diag.recent_task_func_ptrs[idx],
            diag.recent_name_tags[idx],
            diag.recent_name_lens[idx],
            diag.recent_stack_depths[idx],
            diag.recent_param_ptrs[idx],
            diag.recent_prios[idx],
            diag.recent_core_ids[idx],
            diag.recent_task_ptrs[idx],
        );
    }
}

#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default());
    esp_alloc::heap_allocator!(size: 64 * 1024);

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let init = match esp_wifi::init(timg0.timer0, Rng::new(peripherals.RNG)) {
        Ok(ctrl) => ctrl,
        Err(err) => panic!("legacy_nostd_wifi_control: esp_wifi::init err={:?}", err),
    };
    println!("legacy_nostd_wifi_control: init=ok");
    esp_wifi::diagnostic_queue_diag_reset();
    esp_wifi::diagnostic_reset_wifi_mac_isr_count();
    print_wifi_init_config_diag("after_init");
    print_wifi_task_create_diag("after_init");
    print_wifi_mac_isr_diag("after_init");
    print_legacy_diag("after_init");
    let (mut controller, mut ifaces) = match esp_wifi::wifi::new(&init, peripherals.WIFI) {
        Ok(parts) => parts,
        Err(err) => panic!("legacy_nostd_wifi_control: wifi_new err={:?}", err),
    };
    println!("legacy_nostd_wifi_control: wifi_new=ok");
    print_wifi_init_config_diag("after_wifi_new");
    print_wifi_task_create_diag("after_wifi_new");
    print_wifi_mac_isr_diag("after_wifi_new");
    print_legacy_diag("after_wifi_new");
    if let Err(err) = controller.set_mode(WifiMode::Sta) {
        panic!("legacy_nostd_wifi_control: set_mode err={:?}", err);
    }
    println!("legacy_nostd_wifi_control: set_mode=sta");
    if let Err(err) = controller.start() {
        panic!("legacy_nostd_wifi_control: start err={:?}", err);
    }
    println!("legacy_nostd_wifi_control: start=ok");
    print_wifi_task_create_diag("after_start");
    print_wifi_mac_isr_diag("after_start");
    print_legacy_diag("after_start");
    esp_wifi::diagnostic_queue_diag_reset();
    run_promisc_diag(&mut ifaces.sniffer);

    match controller.scan_n(16) {
        Ok(results) => {
            println!("legacy_nostd_wifi_control: scan=ok count={}", results.len());
            for (idx, ap) in results.iter().take(10).enumerate() {
                println!(
                    "legacy_nostd_wifi_control: ap idx={} ssid={} channel={} bssid={:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x} rssi={} auth={:?}",
                    idx,
                    ap.ssid,
                    ap.channel,
                    ap.bssid[0], ap.bssid[1], ap.bssid[2], ap.bssid[3], ap.bssid[4], ap.bssid[5],
                    ap.signal_strength,
                    ap.auth_method,
                );
            }
        }
        Err(err) => println!("legacy_nostd_wifi_control: scan=err err={:?}", err),
    }
    print_wifi_mac_isr_diag("after_scan");
    print_legacy_diag("after_scan");

    match controller.stop() {
        Ok(()) => println!("legacy_nostd_wifi_control: stop=ok"),
        Err(err) => println!("legacy_nostd_wifi_control: stop=err err={:?}", err),
    }

    loop {
        print_legacy_diag("steady");
        unsafe { esp_rom_delay_us(1_000_000) };
    }
}
