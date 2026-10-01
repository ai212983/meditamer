use core::fmt::Write;

use crate::firmware::{observability, types::SerialWriter};

use super::{write_line, write_metrics_imu_line, write_metrics_net_lines};

#[derive(Clone, Copy)]
enum SnapshotLine {
    Wifi,
    WifiLink,
    WifiReassoc,
    WifiScanDiag,
    WifiReasonDiag,
    Upload,
    UploadPhase,
    UploadDecomp,
    UploadRtt,
}

pub(super) async fn write_metrics_lines(uart: &mut SerialWriter) {
    // Keep one coherent telemetry snapshot, but format and write one bounded line at a time.
    // Building every line in this outer future made the compiler retain their async states
    // together, inflating every serial-command allocation by several KiB.
    write_stack_line(uart).await;
    write_i2c_timing(uart).await;
    write_persistence_stage(uart).await;
    #[cfg(feature = "cpu-load")]
    write_cpu_load(uart).await;
    #[cfg(feature = "cpu-load")]
    write_cpu_profile(uart).await;
    write_touch_stack_line(uart).await;
    write_touch_scheduling_line(uart).await;
    write_acquisition_latency(uart).await;
    write_metrics_imu_line(uart).await;
    {
        let snapshot = observability::snapshot();
        for line in [
            SnapshotLine::Wifi,
            SnapshotLine::WifiLink,
            SnapshotLine::WifiReassoc,
            SnapshotLine::WifiScanDiag,
            SnapshotLine::WifiReasonDiag,
            SnapshotLine::Upload,
            SnapshotLine::UploadPhase,
            SnapshotLine::UploadDecomp,
            SnapshotLine::UploadRtt,
        ] {
            write_snapshot_line(uart, &snapshot, line).await;
        }
    }
    write_metrics_net_lines(uart).await;
}

async fn write_persistence_stage(uart: &mut SerialWriter) {
    let mut line = heapless::String::<64>::new();
    let _ = write!(
        line,
        "METRICS PERSIST flash={} store={}\r\n",
        crate::firmware::flash::write_stage(),
        crate::firmware::app_state::store::save_stage()
    );
    write_line(uart, &line).await;
}

async fn write_i2c_timing(uart: &mut SerialWriter) {
    let snapshot = crate::firmware::types::i2c_timing_snapshot();
    let mut line = heapless::String::<192>::new();
    let _ = write!(line,
        "METRICS I2C completed_waits={} queue_max_us={} completed_transfers={} transfer_max_us={} timeouts={} scan_masked_max_us={}\r\n",
        snapshot[0], snapshot[1], snapshot[2], snapshot[3], snapshot[4], inkplate_tempera::scan_masked_max_us());
    write_line(uart, &line).await;
    line.clear();
    let (local, remote) = crate::firmware::types::bus_owner::queue_timing();
    let _ = write!(
        line,
        "METRICS I2C_SERVICE local_queue_max_us={} remote_queue_max_us={}\r\n",
        local, remote
    );
    write_line(uart, &line).await;
    let (trace, holds) = crate::firmware::types::i2c_wait_snapshot();
    line.clear();
    let _ = write!(line,
        "METRICS I2C_WAIT us={} addr={:02x} holder={:02x} transfers={} last8={:016x} busy_us={} tail_us={} at_ms={} acquired={} started_us={} finished_us={}\r\n",
        trace.wait_us, trace.address, trace.initial_holder, trace.transfers,
        trace.addresses, trace.busy_us, trace.tail_us, trace.at_ms, trace.acquired,
        trace.started_us, trace.finished_us);
    write_line(uart, &line).await;
    line.clear();
    let _ = write!(
        line,
        "METRICS I2C_HOLDS at_ms={} count={} transfers={} overflow={}\r\n",
        trace.at_ms, holds.count, trace.transfers, holds.overflow
    );
    write_line(uart, &line).await;
    for (index, hold) in holds.spans.iter().take(holds.count as usize).enumerate() {
        line.clear();
        let _ = write!(
            line,
            "METRICS I2C_HOLD at_ms={} index={} addr={:02x} acquired_us={} released_us={} held_us={}\r\n",
            trace.at_ms,
            index,
            hold.address,
            hold.acquired_us,
            hold.released_us,
            hold.released_us.wrapping_sub(hold.acquired_us)
        );
        write_line(uart, &line).await;
    }
    line.clear();
    let _ = write!(
        line,
        "METRICS I2C_ADMISSION us={} admission_us={} mutex_us={} woke={} wake_to_admit_us={}\r\n",
        trace.wait_us, trace.admission_us, trace.mutex_us, trace.woke, trace.wake_to_admit_us
    );
    write_line(uart, &line).await;
    line.clear();
    let _ = write!(
        line,
        "METRICS I2C_FIFO holder={:02x} head={:02x} head_age_us={} head_turn_us={} turned={} cancelled={}\r\n",
        trace.fifo_holder, trace.fifo_head, trace.fifo_head_age_us,
        trace.fifo_head_turn_us, trace.fifo_head_turned, trace.fifo_head_cancelled
    );
    write_line(uart, &line).await;
    line.clear();
    let _ = write!(
        line,
        "METRICS I2C_TAIL_WORK kind={} id={} run_us={} sampled={} pipeline_frames={} pipeline_branches={} pipeline_poll_us={} touch_phase={} touch_phase_us={}\r\n",
        trace.tail_work_kind, trace.tail_work_id, trace.tail_work_us, trace.tail_work_valid,
        trace.tail_pipeline_frames, trace.tail_pipeline_branches, trace.tail_pipeline_poll_us,
        trace.tail_touch_phase, trace.tail_touch_phase_us
    );
    write_line(uart, &line).await;
    line.clear();
    let _ = write!(
        line,
        "METRICS I2C_TAIL_CPU task_us={} irq_us={} prep_us={} engine_us={} events_us={}\r\n",
        trace.tail_task_total_us,
        trace.tail_irq_total_us,
        trace.tail_pipeline_prep_us,
        trace.tail_pipeline_engine_us,
        trace.tail_pipeline_events_us
    );
    write_line(uart, &line).await;
    let timing = crate::firmware::imu::timing::snapshot();
    line.clear();
    let _ = write!(line,
        "METRICS IMU_TIMING gap_us={} prev_wake_us={} prev_service_us={} prev_publish_us={} wake_us={} prev_skipped={} skipped={}\r\n",
        timing.gap_us, timing.previous_wake_us, timing.previous_service_us,
        timing.previous_publish_us, timing.current_wake_us, timing.previous_skipped, timing.skipped);
    write_line(uart, &line).await;
    line.clear();
    let _ = write!(
        line,
        "METRICS IMU_DEADLINE gap_us={} rebase={} resumed={} mode_changed={}\r\n",
        timing.gap_us,
        timing.previous_rebase as u8,
        timing.previous_resumed as u8,
        timing.previous_mode_changed as u8
    );
    write_line(uart, &line).await;
    let reads = timing.previous_read_timing;
    let measured_us = reads
        .interrupt_port_us
        .saturating_add(reads.sensor_us)
        .saturating_add(timing.previous_aux_us);
    line.clear();
    let _ =
        write!(line,
        "METRICS IMU_READ gap_us={} at_ms={} int_port_us={} sensor_us={} aux_us={} other_us={}\r\n",
        timing.gap_us, timing.previous_start_ms, reads.interrupt_port_us,
        reads.sensor_us, timing.previous_aux_us,
        timing.previous_service_us.saturating_sub(measured_us));
    write_line(uart, &line).await;
    for (stage, bus) in timing.previous_bus_timing.stages.iter().enumerate() {
        line.clear();
        let _ = write!(line,
            "METRICS IMU_BUS stage={} gap_us={} queue_us={} config_us={} transfer_us={} attempts={} errors={}\r\n",
            stage, timing.gap_us, bus.queue_us, bus.config_us, bus.transfer_us, bus.attempts, bus.errors);
        write_line(uart, &line).await;
        line.clear();
        let _ = write!(
            line,
            "METRICS IMU_BUS_SPLIT stage={} admission_us={} mutex_us={}\r\n",
            stage, bus.admission_us, bus.mutex_us
        );
        write_line(uart, &line).await;
    }
    for (stage, wait) in timing.previous_bus_timing.waits.iter().enumerate() {
        line.clear();
        let _ = write!(line,
            "METRICS IMU_WAIT stage={} us={} busy_us={} tail_us={} admission_us={} mutex_us={} woke={} wake_to_admit_us={}\r\n",
            stage, wait.wait_us, wait.busy_us, wait.tail_us, wait.admission_us,
            wait.mutex_us, wait.woke, wait.wake_to_admit_us);
        write_line(uart, &line).await;
        line.clear();
        let _ = write!(line,
            "METRICS IMU_WAIT_CPU stage={} sampled={} task_us={} irq_us={} prep_us={} engine_us={} events_us={}\r\n",
            stage, wait.tail_sampled, wait.task_total_us, wait.irq_total_us,
            wait.pipeline_prep_us, wait.pipeline_engine_us, wait.pipeline_events_us);
        write_line(uart, &line).await;
        line.clear();
        let _ = write!(line,
            "METRICS IMU_WAIT_SOURCE stage={} transfers={} last_owner={:02x} touch_phase={} touch_phase_us={}\r\n",
            stage, wait.transfers, wait.last_owner, wait.touch_phase, wait.touch_phase_us);
        write_line(uart, &line).await;
    }
}

#[cfg(feature = "cpu-load")]
async fn write_cpu_load(uart: &mut SerialWriter) {
    let Some(state) = crate::firmware::cpu_observation::latest() else {
        let line = heapless::String::<40>::try_from("METRICS CPU unavailable=true\r\n").unwrap();
        write_line(uart, &line).await;
        return;
    };
    let mut metadata = heapless::String::<192>::new();
    let _ = write!(
        metadata,
        "METRICS CPU provider={} generation={} revision={} health={:?} attempt_ms={}\r\n",
        state.provider.0,
        state.generation.0,
        state.revision.0,
        state.health,
        state.last_attempt_at.map_or(0, |at| at.0)
    );
    write_line(uart, &metadata).await;
    let snapshot = state.snapshot;
    for (core, reading) in snapshot.cores.into_iter().enumerate() {
        let mut line = heapless::String::<192>::new();
        if let Some(reading) = reading {
            let _ = write!(&mut line,
                "METRICS CPU core={} percent={} peak_60s={} elapsed_us={} busy_us={} irq_us={} age_ms={}\r\n",
                core, reading.percent(), snapshot.peak_percent[core], reading.elapsed_us,
                reading.busy_us, reading.interrupt_us,
                (embassy_time::Instant::now().as_millis() as u32).wrapping_sub(snapshot.at_ms));
        } else {
            let _ = write!(&mut line, "METRICS CPU core={} unavailable=true\r\n", core);
        }
        write_line(uart, &line).await;
    }
}

async fn write_stack_line(uart: &mut SerialWriter) {
    let mut line = heapless::String::<96>::new();
    let _ = write!(
        &mut line,
        "stack_diag: tag=minimum headroom={}\r\n",
        observability::minimum_stack_headroom_bytes()
    );
    write_line(uart, &line).await;
}

async fn write_touch_stack_line(uart: &mut SerialWriter) {
    let mut line = heapless::String::<96>::new();
    let _ = write!(
        &mut line,
        "touch_core_stack_diag: tag=minimum headroom={}\r\n",
        observability::minimum_touch_core_stack_headroom_bytes()
    );
    write_line(uart, &line).await;
}

async fn write_touch_scheduling_line(uart: &mut SerialWriter) {
    let snapshot = crate::firmware::touch::scheduling::snapshot();
    let mut line = heapless::String::<256>::new();
    let _ = write!(
        &mut line,
        "METRICS TOUCH_SCHED active_n={} active_gap_max_ms={}\r\n",
        snapshot.active_sample_count, snapshot.active_sample_gap_max_ms,
    );
    write_line(uart, &line).await;
    let gap = crate::firmware::touch::scheduling::read_snapshot();
    for (phase, read) in [("prev", gap.previous), ("curr", gap.current)] {
        let at_ms = if phase == "prev" {
            gap.previous_at_ms
        } else {
            gap.previous_at_ms.wrapping_add(gap.gap_ms)
        };
        let accounted_us = read
            .bus
            .queue_us
            .saturating_add(read.bus.config_us)
            .saturating_add(read.bus.transfer_us)
            .saturating_add(read.retry_wait_us);
        line.clear();
        let _ = write!(line,
            "METRICS TOUCH_READ phase={} gap_ms={} at_ms={} read_us={} queue_us={} config_us={} transfer_us={} retry_wait_us={} retries={} other_us={} attempts={} errors={}\r\n",
            phase, gap.gap_ms, at_ms, read.read_us, read.bus.queue_us,
            read.bus.config_us, read.bus.transfer_us, read.retry_wait_us,
            read.retries, read.read_us.saturating_sub(accounted_us),
            read.bus.attempts, read.bus.errors);
        write_line(uart, &line).await;
        line.clear();
        let _ = write!(
            line,
            "METRICS TOUCH_READ_SPLIT phase={} admission_us={} mutex_us={}\r\n",
            phase, read.bus.admission_us, read.bus.mutex_us
        );
        write_line(uart, &line).await;
        for (index, wait) in read
            .waits
            .attempts
            .iter()
            .take(read.waits.count as usize)
            .enumerate()
        {
            line.clear();
            let _ = write!(line,
                "METRICS TOUCH_WAIT phase={} attempt={} queue_us={} busy_us={} tail_us={} holder={:02x} transfers={} last2={:04x}\r\n",
                phase, index + 1, wait.queue_us, wait.busy_us, wait.tail_us,
                wait.holder, wait.transfers, wait.last_two);
            write_line(uart, &line).await;
        }
        for (index, wake) in read
            .retry_wakes
            .iter()
            .take(read.retries.min(2) as usize)
            .enumerate()
        {
            line.clear();
            let _ = write!(
                line,
                "METRICS TOUCH_RETRY phase={} attempt={} armed_us={} irq_us={} resumed_us={} irq_events={} valid={}\r\n",
                phase,
                index + 1,
                wake.armed_us,
                wake.irq_us,
                wake.resumed_us,
                wake.irq_events,
                wake.valid
            );
            write_line(uart, &line).await;
        }
    }
}

async fn write_snapshot_line(
    uart: &mut SerialWriter,
    snapshot: &observability::Snapshot,
    line: SnapshotLine,
) {
    match line {
        SnapshotLine::Wifi => {
            let mut line = heapless::String::<256>::new();
            let _ = write!(
                &mut line,
                "METRICS WIFI attempt={} success={} failure={} no_ap={} scan_runs={} scan_empty={} scan_hits={}\r\n",
                snapshot.wifi_connect_attempts,
                snapshot.wifi_connect_successes,
                snapshot.wifi_connect_failures,
                snapshot.wifi_reason_no_ap_found,
                snapshot.wifi_scan_runs,
                snapshot.wifi_scan_empty,
                snapshot.wifi_scan_target_hits,
            );
            write_line(uart, &line).await;
        }
        SnapshotLine::WifiLink => {
            let mut line = heapless::String::<192>::new();
            let _ = write!(
                &mut line,
                "METRICS WIFI_LINK rssi_last_dbm={} rssi_min_dbm={} rssi_max_dbm={} rssi_samples={} rssi_low_samples={}\r\n",
                snapshot.wifi_link_rssi_last_dbm,
                snapshot.wifi_link_rssi_min_dbm,
                snapshot.wifi_link_rssi_max_dbm,
                snapshot.wifi_link_rssi_samples,
                snapshot.wifi_link_rssi_low_samples,
            );
            write_line(uart, &line).await;
        }
        SnapshotLine::WifiReassoc => {
            let mut line = heapless::String::<512>::new();
            let _ = write!(
                &mut line,
                "METRICS WIFI_REASSOC mode_pause={} mode_resume={} cred_rx={} cred_chg={} cfg_apply={} start_ok={} start_err={} conn_begin={} conn_ok={} conn_err={} disc_evt={} probe={} auth_rot={} hint_retry={} conn_ms={} conn_ms_max={}\r\n",
                snapshot.wifi_reassoc_mode_pauses,
                snapshot.wifi_reassoc_mode_resumes,
                snapshot.wifi_reassoc_credentials_received,
                snapshot.wifi_reassoc_credentials_changed,
                snapshot.wifi_reassoc_config_applied,
                snapshot.wifi_reassoc_start_ok,
                snapshot.wifi_reassoc_start_err,
                snapshot.wifi_reassoc_connect_begin,
                snapshot.wifi_reassoc_connect_success,
                snapshot.wifi_reassoc_connect_failure,
                snapshot.wifi_reassoc_disconnect_events,
                snapshot.wifi_reassoc_channel_probes,
                snapshot.wifi_reassoc_auth_rotations,
                snapshot.wifi_reassoc_hint_retries,
                snapshot.wifi_reassoc_connect_ms_total,
                snapshot.wifi_reassoc_connect_ms_max,
            );
            write_line(uart, &line).await;
        }
        SnapshotLine::WifiScanDiag => {
            let mut line = heapless::String::<512>::new();
            let _ = write!(
                &mut line,
                "METRICS WIFI_SCAN_DIAG active_n={} active_empty={} active_hit={} active_ms={} active_ms_max={} passive_n={} passive_empty={} passive_hit={} passive_ms={} passive_ms_max={} last_scan_ch={}\r\n",
                snapshot.wifi_reassoc_scan_active_runs,
                snapshot.wifi_reassoc_scan_active_empty,
                snapshot.wifi_reassoc_scan_active_hits,
                snapshot.wifi_reassoc_scan_active_ms_total,
                snapshot.wifi_reassoc_scan_active_ms_max,
                snapshot.wifi_reassoc_scan_passive_runs,
                snapshot.wifi_reassoc_scan_passive_empty,
                snapshot.wifi_reassoc_scan_passive_hits,
                snapshot.wifi_reassoc_scan_passive_ms_total,
                snapshot.wifi_reassoc_scan_passive_ms_max,
                snapshot.wifi_reassoc_last_scan_channel,
            );
            write_line(uart, &line).await;
        }
        SnapshotLine::WifiReasonDiag => {
            let mut line = heapless::String::<512>::new();
            let _ = write!(
                &mut line,
                "METRICS WIFI_REASON_DIAG r2={} r201={} r202={} r203={} r204={} r205={} r210={} r211={} r212={} rother={} last_reason={} last_auth={} last_ch={} last_probe={} last_stage={}\r\n",
                snapshot.wifi_reassoc_reason_2,
                snapshot.wifi_reassoc_reason_201,
                snapshot.wifi_reassoc_reason_202,
                snapshot.wifi_reassoc_reason_203,
                snapshot.wifi_reassoc_reason_204,
                snapshot.wifi_reassoc_reason_205,
                snapshot.wifi_reassoc_reason_210,
                snapshot.wifi_reassoc_reason_211,
                snapshot.wifi_reassoc_reason_212,
                snapshot.wifi_reassoc_reason_other,
                snapshot.wifi_reassoc_last_reason,
                snapshot.wifi_reassoc_last_auth_idx,
                snapshot.wifi_reassoc_last_channel_hint,
                snapshot.wifi_reassoc_last_probe_idx,
                snapshot.wifi_reassoc_last_stage,
            );
            write_line(uart, &line).await;
        }
        SnapshotLine::Upload => {
            let mut line = heapless::String::<384>::new();
            let _ = write!(
                &mut line,
                "METRICS UPLOAD accept_ok={} accept_err={} request_err={} req_hdr_to={} req_read_body={} req_read_body_reset={} req_sd_busy={} sd_errors={} sd_busy={} sd_timeouts={} sd_power_on_fail={} sd_init_fail={} sess_timeout_abort={} sess_mode_off_abort={}\r\n",
                snapshot.upload_http_accepts,
                snapshot.upload_http_accept_errors,
                snapshot.upload_http_request_errors,
                snapshot.upload_http_header_timeouts,
                snapshot.upload_http_read_body_errors,
                snapshot.upload_http_read_body_resets,
                snapshot.upload_http_sd_busy_errors,
                snapshot.sd_upload_errors,
                snapshot.sd_upload_busy,
                snapshot.sd_upload_timeouts,
                snapshot.sd_upload_power_on_failed,
                snapshot.sd_upload_init_failed,
                snapshot.sd_upload_session_timeout_aborts,
                snapshot.sd_upload_session_mode_off_aborts,
            );
            write_line(uart, &line).await;
        }
        SnapshotLine::UploadPhase => {
            let mut line = heapless::String::<320>::new();
            let _ = write!(
                &mut line,
                "METRICS UPLOAD_PHASE req={} bytes={} body_ms={} body_max={} sd_ms={} sd_max={} req_ms={} req_max={}\r\n",
                snapshot.upload_http_upload_requests,
                snapshot.upload_http_upload_bytes,
                snapshot.upload_http_upload_body_read_ms_total,
                snapshot.upload_http_upload_body_read_ms_max,
                snapshot.upload_http_upload_sd_wait_ms_total,
                snapshot.upload_http_upload_sd_wait_ms_max,
                snapshot.upload_http_upload_request_ms_total,
                snapshot.upload_http_upload_request_ms_max,
            );
            write_line(uart, &line).await;
        }
        SnapshotLine::UploadDecomp => {
            let mut line = heapless::String::<512>::new();
            let _ = write!(
                &mut line,
                "METRICS UPLOAD_DECOMP copy_ms={} copy_max={} sdq_ms={} sdq_max={} sdtask_ms={} sdtask_max={} commit_ms={} commit_max={} chunk_p50_max={} chunk_p95_max={} chunk_max={} chunk_samples={} chunk_drop={}\r\n",
                snapshot.upload_http_upload_payload_copy_ms_total,
                snapshot.upload_http_upload_payload_copy_ms_max,
                snapshot.upload_http_upload_sd_queue_ms_total,
                snapshot.upload_http_upload_sd_queue_ms_max,
                snapshot.upload_http_upload_sd_task_wait_ms_total,
                snapshot.upload_http_upload_sd_task_wait_ms_max,
                snapshot.upload_http_upload_commit_ms_total,
                snapshot.upload_http_upload_commit_ms_max,
                snapshot.upload_http_upload_chunk_p50_ms_max,
                snapshot.upload_http_upload_chunk_p95_ms_max,
                snapshot.upload_http_upload_chunk_max_ms_max,
                snapshot.upload_http_upload_chunk_samples_total,
                snapshot.upload_http_upload_chunk_samples_dropped,
            );
            write_line(uart, &line).await;
        }
        SnapshotLine::UploadRtt => {
            let mut line = heapless::String::<512>::new();
            let _ = write!(
                &mut line,
                "METRICS UPLOAD_RTT begin_n={} begin_ms={} begin_max={} chunk_n={} chunk_ms={} chunk_max={} commit_n={} commit_ms={} commit_max={} abort_n={} abort_ms={} abort_max={} mkdir_n={} mkdir_ms={} mkdir_max={} rm_n={} rm_ms={} rm_max={}\r\n",
                snapshot.sd_upload_rtt_begin_count,
                snapshot.sd_upload_rtt_begin_ms_total,
                snapshot.sd_upload_rtt_begin_ms_max,
                snapshot.sd_upload_rtt_chunk_count,
                snapshot.sd_upload_rtt_chunk_ms_total,
                snapshot.sd_upload_rtt_chunk_ms_max,
                snapshot.sd_upload_rtt_commit_count,
                snapshot.sd_upload_rtt_commit_ms_total,
                snapshot.sd_upload_rtt_commit_ms_max,
                snapshot.sd_upload_rtt_abort_count,
                snapshot.sd_upload_rtt_abort_ms_total,
                snapshot.sd_upload_rtt_abort_ms_max,
                snapshot.sd_upload_rtt_mkdir_count,
                snapshot.sd_upload_rtt_mkdir_ms_total,
                snapshot.sd_upload_rtt_mkdir_ms_max,
                snapshot.sd_upload_rtt_remove_count,
                snapshot.sd_upload_rtt_remove_ms_total,
                snapshot.sd_upload_rtt_remove_ms_max,
            );
            write_line(uart, &line).await;
        }
    }
}

#[cfg(feature = "cpu-load")]
async fn write_cpu_profile(uart: &mut SerialWriter) {
    let Ok(mut report) =
        crate::firmware::psram::ExternalValue::try_new_with(cpu_load::profile::Snapshot::new)
    else {
        crate::firmware::touch::debug_log::uart_write_all(
            uart,
            b"CPUPROFILE ERR reason=snapshot_alloc\r\n",
        )
        .await;
        return;
    };
    for core in 0..2 {
        cpu_load::profile_snapshot(core, &mut report);
        let snapshot = report.timings;
        let mut line = heapless::String::<192>::new();
        let _ = write!(
            line,
            "CPUPROFILE core={} at_us={} other_us={} errors={} poll_max_us={} poll_max_task={} wake_max_us={} wake_max_task={}\r\n",
            core, snapshot.0, snapshot.1, snapshot.2, snapshot.3, snapshot.4, snapshot.5, snapshot.6
        );
        write_line(uart, &line).await;
        for (kind, irq, count) in [
            ("task", false, cpu_load::profile::TASKS),
            ("irq", true, cpu_load::profile::IRQS),
        ] {
            for index in 0..count {
                let counter = report.counter(irq, index);
                if counter.calls == 0 {
                    continue;
                }
                line.clear();
                let _ = write!(
                    line,
                    "CPUPROFILE core={} kind={} id={} calls={} us={}\r\n",
                    core, kind, counter.id, counter.calls, counter.us
                );
                write_line(uart, &line).await;
            }
        }
    }
}

async fn write_acquisition_latency(uart: &mut SerialWriter) {
    use crate::firmware::acquisition_metrics::{delivery_snapshot, IMU, TOUCH};
    for (kind, values) in [
        ("touch", TOUCH.snapshot()),
        ("imu", IMU.snapshot()),
        ("touch_delivery", delivery_snapshot()),
    ] {
        let mut line = heapless::String::<160>::new();
        let _ = write!(
            line,
            "METRICS ACQUISITION kind={} samples={} max_ms={} excluded={}\r\n",
            kind, values[0], values[1], values[2]
        );
        write_line(uart, &line).await;
    }
}
