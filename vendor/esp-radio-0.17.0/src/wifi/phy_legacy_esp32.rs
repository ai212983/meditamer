use portable_atomic::{AtomicBool, AtomicU32, Ordering};
use esp_phy::PhyController;

use crate::{
    ESP_RADIO_LOCK,
    binary::include::{
        phy_close_rf,
        phy_dig_reg_backup,
        phy_wakeup_init,
    },
    hal::peripherals::WIFI,
};

const SOC_PHY_DIG_REGS_MEM_SIZE: usize = 21 * 4;

static mut SOC_PHY_DIG_REGS_MEM: [u8; SOC_PHY_DIG_REGS_MEM_SIZE] = [0; SOC_PHY_DIG_REGS_MEM_SIZE];
static mut G_PHY_DIGITAL_REGS_MEM: *mut u32 = core::ptr::null_mut();
static mut G_IS_PHY_CALIBRATED: bool = false;
static mut S_IS_PHY_REG_STORED: bool = false;
static PHY_ACCESS_REF: AtomicU32 = AtomicU32::new(0);
static LEGACY_PHY_TRACE_ONCE: AtomicBool = AtomicBool::new(false);

#[inline]
fn trace_once(tag: &str) {
    if !LEGACY_PHY_TRACE_ONCE.swap(true, Ordering::Relaxed) {
        warn!("esp_radio: legacy_phy_esp32_diag path={tag}");
    }
}

#[inline]
fn phy_mem_init() {
    unsafe {
        if G_PHY_DIGITAL_REGS_MEM.is_null() {
            G_PHY_DIGITAL_REGS_MEM = core::ptr::addr_of_mut!(SOC_PHY_DIG_REGS_MEM).cast();
        }
    }
}

pub(crate) fn phy_mem_init_diag() {
    trace_once("mem_init");
    phy_mem_init();
}

#[inline]
fn phy_digital_regs_load() {
    unsafe {
        if S_IS_PHY_REG_STORED && !G_PHY_DIGITAL_REGS_MEM.is_null() {
            phy_dig_reg_backup(false, G_PHY_DIGITAL_REGS_MEM);
        }
    }
}

#[inline]
fn phy_digital_regs_store() {
    unsafe {
        if !G_PHY_DIGITAL_REGS_MEM.is_null() {
            phy_dig_reg_backup(true, G_PHY_DIGITAL_REGS_MEM);
            S_IS_PHY_REG_STORED = true;
        }
    }
}

pub(crate) unsafe fn phy_enable() {
    trace_once("enable");
    phy_mem_init();

    let count = PHY_ACCESS_REF.fetch_add(1, Ordering::SeqCst);
    if count != 0 {
        return;
    }

    ESP_RADIO_LOCK.lock(|| unsafe {
        if !G_IS_PHY_CALIBRATED {
            core::mem::forget(WIFI::steal().enable_phy());
            G_IS_PHY_CALIBRATED = true;
        } else {
            crate::common_adapter::phy_enable_clock();
            phy_wakeup_init();
            phy_digital_regs_load();
        }
    });
}

pub(crate) unsafe fn phy_disable() {
    trace_once("disable");

    let count = PHY_ACCESS_REF.fetch_sub(1, Ordering::SeqCst);
    if count != 1 {
        return;
    }

    ESP_RADIO_LOCK.lock(|| unsafe {
        phy_digital_regs_store();
        phy_close_rf();
        crate::common_adapter::phy_disable_clock();
    });
}
