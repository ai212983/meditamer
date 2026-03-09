#[cfg(any(feature = "wifi", feature = "ble"))]
#[allow(unused_imports)]
use crate::hal::{interrupt, peripherals};
#[cfg(feature = "wifi")]
use portable_atomic::{AtomicU32, Ordering};

#[cfg(feature = "wifi")]
static WIFI_MAC_ISR_COUNT: AtomicU32 = AtomicU32::new(0);

pub(crate) fn setup_radio_isr() {
    #[cfg(feature = "ble")]
    {
        // It's a mystery why these interrupts are enabled now since it worked without
        // this before Now at least without disabling these nothing will work
        interrupt::disable(
            crate::hal::system::Cpu::ProCpu,
            peripherals::Interrupt::ETH_MAC,
        );
        interrupt::disable(
            crate::hal::system::Cpu::ProCpu,
            peripherals::Interrupt::UART0,
        );
    }
}

pub(crate) fn shutdown_radio_isr() {
    #[cfg(feature = "ble")]
    {
        interrupt::disable(
            crate::hal::system::Cpu::ProCpu,
            peripherals::Interrupt::RWBT,
        );
        interrupt::disable(
            crate::hal::system::Cpu::ProCpu,
            peripherals::Interrupt::BT_BB,
        );
    }
}

#[cfg(feature = "wifi")]
pub(crate) fn wifi_mac_isr_count() -> u32 {
    WIFI_MAC_ISR_COUNT.load(Ordering::Relaxed)
}

#[cfg(feature = "wifi")]
pub(crate) fn reset_wifi_mac_isr_count() {
    WIFI_MAC_ISR_COUNT.store(0, Ordering::Relaxed);
}

#[cfg(feature = "ble")]
#[allow(non_snake_case)]
#[unsafe(no_mangle)]
fn Software0() {
    unsafe {
        let (fnc, arg) = crate::ble::btdm::ble_os_adapter_chip_specific::ISR_INTERRUPT_7;
        trace!("interrupt Software0 {:?} {:?}", fnc, arg);

        if !fnc.is_null() {
            let fnc: fn(*mut crate::binary::c_types::c_void) = core::mem::transmute(fnc);
            fnc(arg);
        }
    }
}

#[cfg(feature = "wifi")]
#[unsafe(no_mangle)]
extern "C" fn WIFI_MAC() {
    unsafe {
        WIFI_MAC_ISR_COUNT.fetch_add(1, Ordering::Relaxed);
        let (fnc, arg) = crate::wifi::os_adapter::ISR_INTERRUPT_1;
        trace!("interrupt WIFI_MAC {:?} {:?}", fnc, arg);

        if !fnc.is_null() {
            let fnc: fn(*mut crate::binary::c_types::c_void) = core::mem::transmute(fnc);
            fnc(arg);
        }
    }
}

#[cfg(feature = "ble")]
#[unsafe(no_mangle)]
extern "C" fn RWBT() {
    unsafe {
        let (fnc, arg) = crate::ble::btdm::ble_os_adapter_chip_specific::ISR_INTERRUPT_5;
        trace!("interrupt RWBT {:?} {:?}", fnc, arg);

        if !fnc.is_null() {
            let fnc: fn(*mut crate::binary::c_types::c_void) = core::mem::transmute(fnc);
            fnc(arg);
        }
    }
}

#[cfg(feature = "ble")]
#[unsafe(no_mangle)]
extern "C" fn RWBLE() {
    unsafe {
        let (fnc, arg) = crate::ble::btdm::ble_os_adapter_chip_specific::ISR_INTERRUPT_5;
        trace!("interrupt RWBLE {:?} {:?}", fnc, arg);

        if !fnc.is_null() {
            let fnc: fn(*mut crate::binary::c_types::c_void) = core::mem::transmute(fnc);
            fnc(arg);
        }
    }
}

#[cfg(feature = "ble")]
#[unsafe(no_mangle)]
extern "C" fn BT_BB() {
    unsafe {
        let (fnc, arg) = crate::ble::btdm::ble_os_adapter_chip_specific::ISR_INTERRUPT_8;
        trace!("interrupt BT_BB {:?} {:?}", fnc, arg);

        if !fnc.is_null() {
            let fnc: fn(*mut crate::binary::c_types::c_void) = core::mem::transmute(fnc);
            fnc(arg);
        }
    }
}
