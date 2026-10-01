use super::io::{cache_sd_result, write_sd_result, SD_RESULT_CACHE_CAP};
#[cfg(not(feature = "wifi-debug-slim-app"))]
use crate::firmware::touch::config::TOUCH_TRACE_ENABLED;
use crate::firmware::{
    config::{SD_RESULTS, TAP_TRACE_ENABLED, WALL_CLOCK_REQUESTS},
    touch::{config::TOUCH_EVENT_TRACE_ENABLED, debug_log::uart_write_all},
    types::{
        rtc_diagnostics::InkplateRtcDiagnostics, InkplateRtcDriver, RtcI2cDevice, SdResult,
        SerialWriter,
    },
};
use embassy_futures::yield_now;

pub(super) struct SerialTaskState {
    next_sd_request_id: u32,
    next_state_request_id: u16,
    last_sd_request_id: Option<u32>,
    sd_result_cache: heapless::Vec<SdResult, SD_RESULT_CACHE_CAP>,
    firmware_update_clients_suspended: bool,
    rtc: InkplateRtcDriver,
    wall_clock: wall_clock::session::Coordinator,
    next_wall_clock_nonce: u32,
}

impl SerialTaskState {
    pub(super) fn new(rtc_i2c: RtcI2cDevice) -> Self {
        Self {
            next_sd_request_id: 1,
            next_state_request_id: 1,
            last_sd_request_id: None,
            sd_result_cache: heapless::Vec::new(),
            firmware_update_clients_suspended: false,
            rtc: InkplateRtcDriver::with_diagnostics(rtc_i2c, InkplateRtcDiagnostics),
            wall_clock: wall_clock::session::Coordinator::new(),
            next_wall_clock_nonce: 1,
        }
    }

    /// The serial task is the sole owner of RTC access: no cross-task cache,
    /// just fresh reads/writes through this driver on demand.
    pub(super) fn rtc_mut(&mut self) -> &mut InkplateRtcDriver {
        &mut self.rtc
    }

    pub(super) fn wall_clock_mut(&mut self) -> &mut wall_clock::session::Coordinator {
        &mut self.wall_clock
    }

    /// Disjoint borrows of the RTC driver and the wall-clock coordinator,
    /// for `apply_and_verify`, which needs both live at once.
    pub(super) fn rtc_and_wall_clock_mut(
        &mut self,
    ) -> (
        &mut InkplateRtcDriver,
        &mut wall_clock::session::Coordinator,
    ) {
        (&mut self.rtc, &mut self.wall_clock)
    }

    /// A small wrapping counter, never zero
    /// (`Coordinator::open_session` requires a nonzero nonce) -- sufficient
    /// to distinguish one session's reply from a stale one on Inkplate's
    /// single trusted UART link, where the session id is already the
    /// primary admission check.
    pub(super) fn next_wall_clock_nonce(&mut self) -> u32 {
        if self.next_wall_clock_nonce == 0 {
            self.next_wall_clock_nonce = 1;
        }
        let nonce = self.next_wall_clock_nonce;
        self.next_wall_clock_nonce = self.next_wall_clock_nonce.wrapping_add(1);
        nonce
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

    pub(super) async fn write_trace_headers(&mut self, uart: &mut SerialWriter) {
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

    pub(super) async fn drain_runtime_samples(&mut self, uart: &mut SerialWriter) {
        let quiet = crate::firmware::update::transport_quiet();
        if let Ok(result) = SD_RESULTS.try_receive() {
            cache_sd_result(&mut self.sd_result_cache, result);
            if !quiet {
                write_sd_result(uart, result).await;
                yield_now().await;
            }
        }
    }

    /// Register every enabled source without consuming its payload. Remaining
    /// work after a bounded drain makes the next iteration immediately ready.
    pub(super) fn poll_runtime_work(cx: &mut core::task::Context<'_>) -> core::task::Poll<()> {
        let mut ready = WALL_CLOCK_REQUESTS.poll_ready_to_receive(cx).is_ready();
        ready |= SD_RESULTS.poll_ready_to_receive(cx).is_ready();
        if ready {
            core::task::Poll::Ready(())
        } else {
            core::task::Poll::Pending
        }
    }
}
