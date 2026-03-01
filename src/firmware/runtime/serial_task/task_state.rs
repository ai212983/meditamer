use super::io::{cache_sd_result, write_sd_result, write_tap_trace_sample, SD_RESULT_CACHE_CAP};
use crate::firmware::{
    config::{SD_RESULTS, TAP_TRACE_ENABLED, TAP_TRACE_SAMPLES},
    touch::{
        config::{
            TOUCH_EVENT_TRACE_ENABLED, TOUCH_EVENT_TRACE_SAMPLES, TOUCH_TRACE_ENABLED,
            TOUCH_TRACE_SAMPLES, TOUCH_WIZARD_RAW_TRACE_SAMPLES, TOUCH_WIZARD_SESSION_EVENTS,
            TOUCH_WIZARD_SWIPE_TRACE_SAMPLES, TOUCH_WIZARD_TRACE_ENABLED,
        },
        debug_log::{
            uart_write_all, write_touch_event_trace_sample, write_touch_trace_sample,
            write_touch_wizard_swipe_trace_sample, TouchWizardSessionLog,
        },
    },
    types::{SdResult, SerialUart},
};

pub(super) struct SerialTaskState {
    touch_wizard_log: TouchWizardSessionLog,
    next_sd_request_id: u32,
    next_state_request_id: u16,
    last_sd_request_id: Option<u32>,
    sd_result_cache: heapless::Vec<SdResult, SD_RESULT_CACHE_CAP>,
}

impl SerialTaskState {
    pub(super) fn new() -> Self {
        Self {
            touch_wizard_log: TouchWizardSessionLog::new(),
            next_sd_request_id: 1,
            next_state_request_id: 1,
            last_sd_request_id: None,
            sd_result_cache: heapless::Vec::new(),
        }
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

    pub(super) async fn write_touch_wizard_dump(&mut self, uart: &mut SerialUart) {
        self.touch_wizard_log.write_dump(uart).await;
    }

    pub(super) async fn write_trace_headers(&mut self, uart: &mut SerialUart) {
        if TAP_TRACE_ENABLED {
            let _ = uart_write_all(
                uart,
                b"tap_trace,ms,tap_src,seq,cand,csrc,state,reject,score,window,cooldown,jerk,veto,gyro,int1,int2,pgood,batt_pct,gx,gy,gz,ax,ay,az\r\n",
            )
            .await;
        }
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
        if TOUCH_WIZARD_TRACE_ENABLED {
            let _ = uart_write_all(
                uart,
                b"touch_wizard_swipe,ms,case,attempt,expected_dir,expected_speed,verdict,class_dir,start_x,start_y,end_x,end_y,duration_ms,move_count,max_travel_px,release_debounce_ms,dropout_count\r\n",
            )
            .await;
        }
    }

    pub(super) async fn drain_runtime_samples(&mut self, uart: &mut SerialUart) {
        while let Ok(session_event) = TOUCH_WIZARD_SESSION_EVENTS.try_receive() {
            self.touch_wizard_log.on_session_event(session_event);
        }

        if TOUCH_EVENT_TRACE_ENABLED {
            while let Ok(event) = TOUCH_EVENT_TRACE_SAMPLES.try_receive() {
                self.touch_wizard_log.on_touch_event(event);
                write_touch_event_trace_sample(uart, event).await;
            }
        }

        while let Ok(sample) = TOUCH_WIZARD_SWIPE_TRACE_SAMPLES.try_receive() {
            self.touch_wizard_log.on_swipe_sample(sample);
            if TOUCH_WIZARD_TRACE_ENABLED {
                write_touch_wizard_swipe_trace_sample(uart, sample).await;
            }
        }
        while let Ok(sample) = TOUCH_WIZARD_RAW_TRACE_SAMPLES.try_receive() {
            self.touch_wizard_log.on_touch_sample(sample);
        }

        if self.touch_wizard_log.settle_pending_end() {
            self.touch_wizard_log.write_dump(uart).await;
        }

        if TOUCH_TRACE_ENABLED {
            while let Ok(sample) = TOUCH_TRACE_SAMPLES.try_receive() {
                write_touch_trace_sample(uart, sample).await;
            }
        }

        if TAP_TRACE_ENABLED {
            while let Ok(sample) = TAP_TRACE_SAMPLES.try_receive() {
                write_tap_trace_sample(uart, sample).await;
            }
        }

        while let Ok(result) = SD_RESULTS.try_receive() {
            cache_sd_result(&mut self.sd_result_cache, result);
            write_sd_result(uart, result).await;
        }
    }
}
