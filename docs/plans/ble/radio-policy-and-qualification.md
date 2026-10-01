# Radio policy and qualification

Status: Active

This backlog covers unfinished policy and target qualification. Completed
Inkplate handoff evidence and power criteria live in the
[qualification reference](../../references/radio-qualification.md).
Follow the [exclusive radio decision](../../architecture/wifi-ble-radio-exclusivity.md)
and [diagnostic protocol](../../references/ble-diagnostic-protocol.md).

## Configurable default owner

- Implement validated default-owner selection, policy generation and restoration
  of the latest desired policy after a lease, including BLE-default non-advertising idle.
- Define central-input availability, Wi-Fi interruption and reconnect/sleep
  restoration separately from diagnostic visibility deadlines.
- Verify boot policy, both handoff directions, failure recovery and policy changes
  during leases, including confirmed controller/callback shutdown and service restoration.
- Revise the governing contract or decision when default-owner or exclusive-radio
  policy changes.

## Remaining Inkplate acceptance

On an identified complete release artifact:

- Exercise interrupted uploads, input/display load, traffic saturation, reset
  and supported sleep/wake; complete the forty-boot reservation soak.
- Measure power against the
  [Inkplate criteria](../../references/radio-qualification.md#inkplate-power-qualification),
  confirming their applicability and actual radio configuration before the run.
- Run [memory validation](../../guides/memory/validation.md),
  [handoff acceptance](../../guides/network/validation.md#inkplate-wi-fible-handoff)
  and [Wi-Fi regression](../../guides/network/validation.md) on affected candidates.
  Retain artifact identity, workload and telemetry profile with every result.

## Independent Medinote acceptance

Waveshare has compile evidence only. On its complete release artifact:

- Physically qualify radio handoffs, controller/callback shutdown, failure recovery,
  discovery, uploads, restart and service restoration under supported workloads.
- Exercise repeated BLE sessions, interrupted uploads, input/display load, traffic
  saturation, reset and supported sleep/wake.
- Run target-specific memory and power acceptance; establish Waveshare limits
  rather than copying Inkplate figures.
