use super::{
    io::{cache_sd_result, write_sd_result, write_tap_trace_sample, SD_RESULT_CACHE_CAP},
    status::format_status,
};
#[cfg(not(feature = "wifi-debug-slim-app"))]
use crate::firmware::touch::{
    config::{TOUCH_TRACE_ENABLED, TOUCH_TRACE_SAMPLES},
    debug_log::write_touch_trace_sample,
};
use crate::firmware::{
    config::{
        SD_RESULTS, SD_SERIAL_LINES, SERIAL_STATUS_EVENTS, TAP_TRACE_ENABLED, TAP_TRACE_SAMPLES,
    },
    touch::{
        config::{TOUCH_EVENT_TRACE_ENABLED, TOUCH_EVENT_TRACE_SAMPLES},
        debug_log::{uart_write_all, write_touch_event_trace_sample},
    },
    types::{InkplateRtcDriver, SdResult, SerialUart, SharedI2cDevice},
};
use embassy_futures::yield_now;

pub(super) struct SerialTaskState {
    next_sd_request_id: u32,
    next_state_request_id: u16,
    last_sd_request_id: Option<u32>,
    sd_result_cache: heapless::Vec<SdResult, SD_RESULT_CACHE_CAP>,
    firmware_update_clients_suspended: bool,
    rtc: InkplateRtcDriver,
}

impl SerialTaskState {
    pub(super) fn new(rtc_i2c: SharedI2cDevice) -> Self {
        Self {
            next_sd_request_id: 1,
            next_state_request_id: 1,
            last_sd_request_id: None,
            sd_result_cache: heapless::Vec::new(),
            firmware_update_clients_suspended: false,
            rtc: InkplateRtcDriver::new(rtc_i2c),
        }
    }

    /// The serial task is the sole owner of RTC access: no cross-task cache,
    /// just fresh reads/writes through this driver on demand.
    pub(super) fn rtc_mut(&mut self) -> &mut InkplateRtcDriver {
        &mut self.rtc
    }

    pub(super) fn end_firmware_update_hardware_lease(&mut self) -> bool {
        core::mem::take(&mut self.firmware_update_clients_suspended)
    }

    pub(super) fn firmware_update_hardware_lease_active(&self) -> bool {
        self.firmware_update_clients_suspended
    }

    pub(super) fn next_sd_request_id(&mut self) -> u32 {
        let request_id = self.next_sd_request_id;
        self.next_sd_request_id = self.next_sd_request_id.wrapping_add(1);
        request_id
    }

    pub(super) fn next_state_request_id(&mut self) -> u16 {
        let request_id = self.next_state_request_id;
        self.next_state_request_id = self.next_state_request_id.wrapping_add(1);
        request_id
    }

    pub(super) fn set_last_sd_request_id(&mut self, request_id: u32) {
        self.last_sd_request_id = Some(request_id);
    }

    pub(super) fn last_sd_request_id(&self) -> Option<u32> {
        self.last_sd_request_id
    }

    pub(super) fn sd_result_cache_mut(
        &mut self,
    ) -> &mut heapless::Vec<SdResult, SD_RESULT_CACHE_CAP> {
        &mut self.sd_result_cache
    }

    pub(super) async fn write_trace_headers(&mut self, uart: &mut SerialUart) {
        if TAP_TRACE_ENABLED {
            let _ = uart_write_all(
                uart,
                b"tap_trace,ms,tap_src,seq,cand,csrc,state,reject,score,window,cooldown,jerk,veto,gyro,int1,int2,pgood,batt_pct,gx,gy,gz,ax,ay,az\r\n",
            )
            .await;
        }
        #[cfg(not(feature = "wifi-debug-slim-app"))]
        if TOUCH_TRACE_ENABLED {
            let _ = uart_write_all(
                uart,
                b"touch_trace,ms,count,x0,y0,x1,y1,raw0,raw1,raw2,raw3,raw4,raw5,raw6,raw7\r\n",
            )
            .await;
        }
        if TOUCH_EVENT_TRACE_ENABLED {
            let _ = uart_write_all(
                uart,
                b"touch_event,ms,kind,x,y,start_x,start_y,duration_ms,count,move_count,max_travel_px,release_debounce_ms,dropout_count\r\n",
            )
            .await;
        }
    }

    pub(super) async fn drain_runtime_samples(&mut self, uart: &mut SerialUart) {
        let quiet = crate::firmware::update::transport_quiet();
        while let Ok(event) = SERIAL_STATUS_EVENTS.try_receive() {
            if !quiet {
                let line = format_status(event);
                let _ = uart_write_all(uart, line.as_bytes()).await;
            }
        }

        if TOUCH_EVENT_TRACE_ENABLED {
            while let Ok(event) = TOUCH_EVENT_TRACE_SAMPLES.try_receive() {
                if !quiet {
                    write_touch_event_trace_sample(uart, event).await;
                }
            }
        }

        #[cfg(not(feature = "wifi-debug-slim-app"))]
        if TOUCH_TRACE_ENABLED {
            while let Ok(sample) = TOUCH_TRACE_SAMPLES.try_receive() {
                if !quiet {
                    write_touch_trace_sample(uart, sample).await;
                }
            }
        }

        if TAP_TRACE_ENABLED {
            while let Ok(sample) = TAP_TRACE_SAMPLES.try_receive() {
                if !quiet {
                    write_tap_trace_sample(uart, sample).await;
                }
            }
        }

        while let Ok(line) = SD_SERIAL_LINES.try_receive() {
            if !quiet {
                let _ = uart_write_all(uart, line.as_bytes()).await;
                yield_now().await;
            }
        }

        while let Ok(result) = SD_RESULTS.try_receive() {
            cache_sd_result(&mut self.sd_result_cache, result);
            if !quiet {
                write_sd_result(uart, result).await;
                yield_now().await;
            }
        }
    }
}
