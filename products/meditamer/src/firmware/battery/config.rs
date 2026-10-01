//! Internal bounded transport: one provider receiver and two total requests.

use super::types::{BatteryFields, BatteryStateSnapshot, FIELDS};
use crate::firmware::bounded_control::Control;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::watch::Watch;
use observation::ingress::RequestIngress;
use observation::periodic::DemandControl;

pub(crate) const REQUEST_CAPACITY: usize = 2;
pub(crate) static BATTERY_DEMAND: DemandControl<CriticalSectionRawMutex, BatteryFields, FIELDS> =
    DemandControl::new();
pub(crate) static BATTERY_STATE: Watch<CriticalSectionRawMutex, BatteryStateSnapshot, 1> =
    Watch::new();
pub(crate) static BATTERY_REQUESTS: RequestIngress<
    CriticalSectionRawMutex,
    BatteryFields,
    REQUEST_CAPACITY,
> = RequestIngress::new();
pub(super) static CONTROL: Control<CriticalSectionRawMutex> = Control::new();
