//! A real central/scanner role over the reviewed BLE controller (shared BLE
//! runtime and roles plan, Phase 3: "Scan in fixed windows, keep a bounded
//! set of results, and recognize one chosen advertising device... Exercise
//! a full result set and repeated runtime restarts").
//!
//! All the bookkeeping (bounded result set, dedupe, restart, stale-event
//! rejection) is [`crate::scan::ScanWindow`], already host-tested in Phase
//! 2; this module only wires it to a real `trouble-host` central/scanner
//! and decodes real advertising reports into
//! [`crate::scan::Advertisement`]s. No GATT, no connections, no HID -- those
//! are later phases (see this module's own scope note next to the
//! `trouble-host` dependency in `Cargo.toml`).
//!
//! `trouble-host`'s `EventHandler::on_adv_reports` delivers reports through
//! a shared `&self`, so the in-flight [`crate::scan::ScanWindow`] lives
//! behind a `critical_section::Mutex<RefCell<_>>` for the duration of one
//! [`run`] call -- the same interior-mutability shape `platform/connectivity/netstack`
//! already uses for callback-delivered state.

use core::cell::RefCell;
use core::pin::pin;

use critical_section::Mutex;
use embassy_futures::select::{select, Either};
use embassy_time::{Duration, Timer};
use esp_hal::peripherals::BT;
use esp_radio::ble::controller::BleConnector;
use heapless::Vec;
use static_cell::StaticCell;
use trouble_host::prelude::*;

use crate::capacity::SCAN_RESULTS_MAX;
use crate::scan::{Advertisement, ScanResult, ScanWindow};

const CONNECTIONS_MAX: usize = 1;
const L2CAP_CHANNELS_MAX: usize = 1;
const AD_TYPE_SHORTENED_LOCAL_NAME: u8 = 0x08;
const AD_TYPE_COMPLETE_LOCAL_NAME: u8 = 0x09;

type Controller = ExternalController<BleConnector<'static>, 1>;
type Resources = HostResources<DefaultPacketPool, CONNECTIONS_MAX, L2CAP_CHANNELS_MAX>;

/// Why [`run`] stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveScanOutcome {
    /// Every requested window ran to completion.
    Completed,
    /// `BleConnector::new` returned an initialization error.
    ControllerInit,
    /// The host runner exited before scanning finished -- a transport
    /// fault, not a scan-specific failure.
    HostExited,
    /// The `LeSetScanEnable`/`LeSetScanParams` command sequence failed.
    ScanCommandFailed,
}

/// One completed scan window's results, taken out of the live
/// [`crate::scan::ScanWindow`] after it closed.
pub struct LiveScanWindowResult {
    pub generation: u32,
    pub results: Vec<ScanResult, SCAN_RESULTS_MAX>,
    pub overflow_count: u32,
}

/// The outcome of [`run`]: one entry per window that ran before
/// `outcome` stopped further scanning.
pub struct LiveScanReport {
    pub outcome: LiveScanOutcome,
    pub windows: Vec<LiveScanWindowResult, 4>,
}

struct AdvertisementHandler<'a> {
    window: &'a Mutex<RefCell<ScanWindow>>,
    now_ticks: fn() -> u64,
}

impl EventHandler for AdvertisementHandler<'_> {
    fn on_adv_reports(&self, reports: bt_hci::param::LeAdvReportsIter) {
        for report in reports.into_iter().flatten() {
            let name = parse_local_name(report.data);
            let address: [u8; 6] = report.addr.raw().try_into().unwrap_or([0; 6]);
            let now = (self.now_ticks)();
            critical_section::with(|token| {
                let mut window = self.window.borrow_ref_mut(token);
                let generation = window.generation();
                // A real controller only ever delivers reports for the
                // window that is currently open, but `observe_if_current`
                // is the same stale-event guard `crate::scan`'s own host
                // tests already prove -- reused here rather than
                // duplicated.
                let _ = window.observe_if_current(
                    generation,
                    now,
                    &Advertisement {
                        address,
                        name,
                        rssi: report.rssi,
                    },
                );
            });
        }
    }
}

/// Walk the raw AD-structure bytes of one advertisement for a Complete or
/// Shortened Local Name. Malformed length-prefixed data (a length byte
/// that would run past the end) stops the walk rather than panicking or
/// reading out of bounds; the name defaults to empty either way.
pub(crate) fn parse_local_name(data: &[u8]) -> &str {
    let mut remaining = data;
    while let [len, rest @ ..] = remaining {
        let len = *len as usize;
        if len == 0 || len > rest.len() {
            break;
        }
        let (structure, next) = rest.split_at(len);
        remaining = next;
        let Some((&ad_type, payload)) = structure.split_first() else {
            continue;
        };
        if ad_type == AD_TYPE_COMPLETE_LOCAL_NAME || ad_type == AD_TYPE_SHORTENED_LOCAL_NAME {
            return core::str::from_utf8(payload).unwrap_or("");
        }
    }
    ""
}

/// Run `window_count` sequential fixed scan windows, each `window_duration`
/// long, restarting [`crate::scan::ScanWindow`] between them (plan: "repeated
/// runtime restarts"). `now_ticks` is the caller's monotonic clock in
/// microseconds (unlike `crate::scan`'s unit-agnostic tests, this module
/// computes real deadlines against real `embassy_time::Duration`s, so it
/// fixes a concrete unit), sampled per advertisement and per window
/// boundary; this module reads it but never advances it itself.
///
/// Takes the chip's `BT` peripheral singleton by value, same as
/// [`crate::start_stop_probe`]. Tears the controller down cleanly (the
/// `BleConnector`'s own `Drop` impl) when every window has run or the host
/// exits.
pub async fn run(
    device: BT<'static>,
    window_count: u32,
    window_duration: Duration,
    now_ticks: fn() -> u64,
) -> LiveScanReport {
    let connector = match BleConnector::new(device, Default::default()) {
        Ok(connector) => connector,
        Err(_) => {
            return LiveScanReport {
                outcome: LiveScanOutcome::ControllerInit,
                windows: Vec::new(),
            };
        }
    };
    let controller: Controller = ExternalController::new(connector);

    static RESOURCES: StaticCell<Resources> = StaticCell::new();
    let resources = RESOURCES.init(Resources::new());
    let stack = trouble_host::new(controller, resources)
        .set_random_address(Address::random([0xfe, 0x53, 0x43, 0x41, 0x4e, 0x01]))
        .build();
    let mut runner = stack.runner();
    let mut central = stack.central();
    let mut scanner = Scanner::new(&mut central);

    let window = Mutex::new(RefCell::new(ScanWindow::new(
        now_ticks().saturating_add(window_duration.as_micros()),
    )));
    let handler = AdvertisementHandler {
        window: &window,
        now_ticks,
    };
    let config = ScanConfig {
        active: true,
        ..Default::default()
    };

    let mut windows: Vec<LiveScanWindowResult, 4> = Vec::new();
    let mut outcome = LiveScanOutcome::Completed;

    let mut runner_future = pin!(async { runner.run_with_handler(&handler).await });
    for window_index in 0..window_count {
        if window_index > 0 {
            critical_section::with(|token| {
                window
                    .borrow_ref_mut(token)
                    .restart(now_ticks().saturating_add(window_duration.as_micros()));
            });
        }

        let scan_body = async {
            match scanner.scan(&config).await {
                Ok(session) => {
                    Timer::after(window_duration).await;
                    drop(session);
                    Ok(())
                }
                Err(_) => Err(()),
            }
        };
        match select(runner_future.as_mut(), scan_body).await {
            Either::First(_) => {
                outcome = LiveScanOutcome::HostExited;
                break;
            }
            Either::Second(Err(())) => {
                outcome = LiveScanOutcome::ScanCommandFailed;
                break;
            }
            Either::Second(Ok(())) => {}
        }

        let (generation, results, overflow_count) = critical_section::with(|token| {
            let window = window.borrow_ref(token);
            (
                window.generation(),
                Vec::from_slice(window.results()).unwrap_or_default(),
                window.overflow_count(),
            )
        });
        if windows
            .push(LiveScanWindowResult {
                generation,
                results,
                overflow_count,
            })
            .is_err()
        {
            break;
        }
    }

    LiveScanReport { outcome, windows }
}
