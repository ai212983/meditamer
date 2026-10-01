#![allow(dead_code)]
#[path = "../../../products/meditamer/src/firmware/types/i2c/admission.rs"]
mod admission;
#[path = "../../../products/meditamer/src/firmware/types/i2c/client_timing.rs"]
mod client_timing;
#[path = "../../../products/meditamer/src/firmware/types/i2c/wait_trace.rs"]
mod wait_trace;

#[test]
fn imu_stage_keeps_largest_wait_but_sums_retry_splits() {
    client_timing::reset_imu();
    let first = wait_trace::Snapshot {
        address: 0x20,
        wait_us: 3_000,
        admission_us: 2_800,
        mutex_us: 200,
        ..Default::default()
    };
    client_timing::record_touch_wait(1, first);
    client_timing::record(1, 0x20, Some(0x01), 3_000, 100, 700, true);
    let retry = wait_trace::Snapshot {
        address: 0x20,
        wait_us: 2_000,
        admission_us: 1_500,
        mutex_us: 500,
        ..Default::default()
    };
    client_timing::record_touch_wait(1, retry);
    client_timing::record(1, 0x20, Some(0x01), 2_000, 100, 700, false);
    let timing = client_timing::imu_snapshot();
    assert_eq!(timing.stages[0].queue_us, 5_000);
    assert_eq!(timing.stages[0].admission_us, 4_300);
    assert_eq!(timing.stages[0].mutex_us, 700);
    assert_eq!((timing.stages[0].attempts, timing.stages[0].errors), (2, 1));
    assert_eq!(timing.waits[0].wait_us, 3_000);
}

#[test]
fn sensor_pair_charges_shared_admission_once() {
    client_timing::reset_imu();
    client_timing::record_touch_wait(
        1,
        wait_trace::Snapshot {
            address: 0x6b,
            wait_us: 1_200,
            admission_us: 1_100,
            mutex_us: 100,
            ..Default::default()
        },
    );
    client_timing::record(1, 0x6b, Some(0x1c), 1_200, 50, 300, false);
    client_timing::record(1, 0x6b, Some(0x22), 0, 0, 400, false);
    let timing = client_timing::imu_snapshot();
    assert_eq!(timing.stages[1].admission_us, 1_100);
    assert_eq!(timing.stages[1].mutex_us, 100);
    assert_eq!(timing.stages[2].admission_us, 0);
    assert_eq!(timing.stages[2].mutex_us, 0);
    assert_eq!(timing.waits[1].wait_us, 1_200);
}
