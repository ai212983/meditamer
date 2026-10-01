use console_host_harness::{counters, deferred, drop_counts, dropped_write_count};
use std::sync::Mutex;

static SERIAL: Mutex<()> = Mutex::new(());

fn drain_all() {
    while deferred::RECORDS.try_receive().is_ok() {}
}

#[test]
fn oversize_reports_oversize_and_single_accounting() {
    let _guard = SERIAL.lock().unwrap();
    drain_all();
    let before = drop_counts();
    let total_before = dropped_write_count();

    let outcome = deferred::enqueue(format_args!("{:0260}", 1), true);
    assert_eq!(outcome, Err(deferred::DeferredDrop::Oversize));

    let after = drop_counts();
    assert_eq!(after.total.wrapping_sub(before.total), 1);
    assert_eq!(after.contention.wrapping_sub(before.contention), 0);
    assert_eq!(
        after
            .deferred_overflow
            .wrapping_sub(before.deferred_overflow),
        0
    );
    assert_eq!(
        after
            .deferred_oversize
            .wrapping_sub(before.deferred_oversize),
        1
    );
    assert_eq!(dropped_write_count().wrapping_sub(total_before), 1);
    assert!(after.stable);
    assert_eq!(
        after.total,
        after
            .contention
            .wrapping_add(after.deferred_overflow)
            .wrapping_add(after.deferred_oversize)
    );
    assert!(deferred::RECORDS.try_receive().is_err());
}

#[test]
fn boundary_256_byte_record_fits_and_257_fails() {
    let _guard = SERIAL.lock().unwrap();
    drain_all();

    assert!(deferred::enqueue(format_args!("{:0255}", 1), true).is_ok());
    let record = deferred::RECORDS.try_receive().expect("fits in 256");
    assert_eq!(record.len(), 256);

    let before = drop_counts();
    let outcome = deferred::enqueue(format_args!("{:0256}", 1), true);
    assert_eq!(outcome, Err(deferred::DeferredDrop::Oversize));
    let after = drop_counts();
    assert_eq!(after.total.wrapping_sub(before.total), 1);
    assert_eq!(
        after
            .deferred_oversize
            .wrapping_sub(before.deferred_oversize),
        1
    );
    drain_all();
}

#[test]
fn capacity_four_then_overflow_with_recovery() {
    let _guard = SERIAL.lock().unwrap();
    drain_all();
    let before = drop_counts();

    for i in 0..4 {
        assert!(deferred::enqueue(format_args!("record={i}"), true).is_ok());
    }
    let outcome = deferred::enqueue(format_args!("overflow"), true);
    assert_eq!(outcome, Err(deferred::DeferredDrop::Overflow));

    let after = drop_counts();
    assert_eq!(after.total.wrapping_sub(before.total), 1);
    assert_eq!(
        after
            .deferred_overflow
            .wrapping_sub(before.deferred_overflow),
        1
    );
    assert_eq!(
        after
            .deferred_oversize
            .wrapping_sub(before.deferred_oversize),
        0
    );
    assert_eq!(after.contention.wrapping_sub(before.contention), 0);

    for i in 0..4 {
        let record = deferred::RECORDS.try_receive().expect("queued record");
        assert_eq!(record.as_str(), &format!("record={i}\n"));
    }
    assert!(deferred::RECORDS.try_receive().is_err());
    assert!(deferred::enqueue(format_args!("recovered"), true).is_ok());
    assert_eq!(
        deferred::RECORDS.try_receive().unwrap().as_str(),
        "recovered\n"
    );
}

#[test]
fn contention_reports_contention_and_single_accounting() {
    let _guard = SERIAL.lock().unwrap();
    drain_all();
    let before = drop_counts();
    let total_before = dropped_write_count();

    counters::record_contention();

    let after = drop_counts();
    assert_eq!(after.total.wrapping_sub(before.total), 1);
    assert_eq!(after.contention.wrapping_sub(before.contention), 1);
    assert_eq!(
        after
            .deferred_overflow
            .wrapping_sub(before.deferred_overflow),
        0
    );
    assert_eq!(
        after
            .deferred_oversize
            .wrapping_sub(before.deferred_oversize),
        0
    );
    assert_eq!(dropped_write_count().wrapping_sub(total_before), 1);
}

#[test]
fn each_failure_accounts_exactly_one_cause_plus_total() {
    let _guard = SERIAL.lock().unwrap();
    drain_all();
    let before = drop_counts();

    assert_eq!(
        deferred::enqueue(format_args!("{:0300}", 1), true),
        Err(deferred::DeferredDrop::Oversize)
    );
    for i in 0..4 {
        assert!(deferred::enqueue(format_args!("fill={i}"), true).is_ok());
    }
    assert_eq!(
        deferred::enqueue(format_args!("extra"), true),
        Err(deferred::DeferredDrop::Overflow)
    );
    counters::record_contention();
    drain_all();

    let after = drop_counts();
    assert_eq!(after.total.wrapping_sub(before.total), 3);
    assert_eq!(after.contention.wrapping_sub(before.contention), 1);
    assert_eq!(
        after
            .deferred_overflow
            .wrapping_sub(before.deferred_overflow),
        1
    );
    assert_eq!(
        after
            .deferred_oversize
            .wrapping_sub(before.deferred_oversize),
        1
    );
    assert!(after.stable);
}

#[test]
fn snapshot_stable_and_coherent_when_quiescent() {
    let _guard = SERIAL.lock().unwrap();
    drain_all();
    deferred::enable_deferred_logs();
    assert!(deferred::enabled());
    let counts = drop_counts();
    assert!(counts.stable);
    assert_eq!(
        counts.total,
        counts
            .contention
            .wrapping_add(counts.deferred_overflow)
            .wrapping_add(counts.deferred_oversize)
    );
    assert_eq!(dropped_write_count(), counts.total);
}

#[test]
fn snapshot_never_hangs_under_concurrent_drops() {
    let _guard = SERIAL.lock().unwrap();
    drain_all();
    let handles: Vec<_> = (0..4)
        .map(|worker| {
            std::thread::spawn(move || {
                for _ in 0..500 {
                    match worker % 3 {
                        0 => counters::record_contention(),
                        1 => counters::record_deferred_overflow(),
                        _ => counters::record_deferred_oversize(),
                    }
                }
            })
        })
        .collect();
    for _ in 0..1000 {
        let counts = drop_counts();
        if counts.stable {
            assert_eq!(
                counts.total,
                counts
                    .contention
                    .wrapping_add(counts.deferred_overflow)
                    .wrapping_add(counts.deferred_oversize)
            );
        }
    }
    for handle in handles {
        handle.join().unwrap();
    }
    drain_all();
    let settled = drop_counts();
    assert!(settled.stable);
    assert_eq!(
        settled.total,
        settled
            .contention
            .wrapping_add(settled.deferred_overflow)
            .wrapping_add(settled.deferred_oversize)
    );
}
