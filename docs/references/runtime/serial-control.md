# Runtime serial control

Inkplate commands use UART0 at 115200 baud, CRLF terminated. Medinote exposes
its supported subset through native USB. [Hostctl](../hostctl.md) owns transport
and paths; [device validation](../../guides/diagnostics/device-validation.md) owns recipes.

## Service state

```text
STATE GET
STATE SET upload=on|off
STATE DIAG kind=debug targets=SD|WIFI
DIAG GET
STATE phase=<...> upload=<on|off> diag_kind=<...> targets=<NONE|SD|WIFI|DISPLAY|TOUCH|IMU> ready=<true|false>
DIAG state=<idle|running|done|failed|canceled> targets=<...> step=<...> code=<...>
```

App state persists in flash. `STATE SET` returns OK after runtime application.
Upload enable controls service/network admission; disabling it releases transfer
buffers. It does not prohibit normal UI, full refresh, clean recovery, clock
navigation, IMU actions or frontlight. Panel safety and diagnostic admission still
apply. Scheduling uses the upload profile while network service is enabled,
independently of transferred bytes. Hardware coexistence requires qualification.

## Navigation

`UISTEP` cycles Home/Ambient → Launcher → Diagnostics → Home; from Analog Clock
it returns Home. `UISTEP ANALOG_CLOCK` and `UISTEP AMBIENT_VIEW` launch through
Launcher without changing the persisted home binding. Direct launch from Home
is rejected. `UISTEP OK|BUSY|ERR reason=...` follows the navigation panel frame;
clock numerical preparation continues cooperatively afterward.

`MOUNTAINSTATUS` returns `MOUNTAIN_STATUS adopted=0|1 composed=0|1 tx_drop=<n>`.
Ambient clears adoption/composition on release or rearm. Use correlated status
rather than droppable unsolicited lines for re-entry gates.

## Time synchronization

`TIMEGET` reads a fresh RTC snapshot without changing time. `TIMESYNC` opens a
bounded manual session on Inkplate; each firmware runtime also opens one boot
session. A valid RTC remains usable when the offered boot synchronization expires.

The shared [codec](../../../platform/time/wall-clock/src/codec.rs) defines:

```text
TIME_REQUEST version=<u8> session=<u32> nonce=<u32> reason=<label> deadline_ms=<u32>
TIME_REPLY version=<u8> session=<u32> nonce=<u32> utc=<u32> offset_min=<i16>
TIME_SYNC OK version=<u8> session=<u32> utc=<u32> offset_min=<i16>
TIME_SYNC ERR version=<u8> session=<u32> reason=<label>
```

The host echoes the request's version, session and nonce. Only the first matching
reply from an allowed source before the deadline can apply time. Late, duplicate,
mismatched and wrong-state replies are ignored; malformed lines do not write the
RTC. The [coordinator](../../../platform/time/wall-clock/src/session.rs) owns
admission, and the [RTC backend](../../../platform/time/wall-clock/src/rtc_backend.rs)
validates the value and verifies a delayed readback before reporting success.

Flash capture retains the boot `TIME_REQUEST`, which the device emits once.
Hostctl answers a still-live captured request; if its deadline has elapsed, it
requests a fresh Inkplate session with `TIMESYNC`. See
[flash capture outcomes](../hostctl.md#flash-capture) for artifact and failure
semantics. There is no unconditional serial time-set command.

## Allocator

`PSRAM` (aliases `HEAP`, `ALLOCATOR`) returns:

```text
PSRAM feature_enabled=<bool> state=<state> total_bytes=<n> used_bytes=<n> free_bytes=<n> peak_used_bytes=<n> internal_free_bytes=<n> external_free_bytes=<n> min_free_bytes=<n> min_internal_free_bytes=<n> min_external_free_bytes=<n> large_alloc_external_ok=<n> large_alloc_internal_ok=<n> large_alloc_fail=<n>
```

Internal free is capability-constrained radio allocation capacity. `min_*` are
boot-lifetime low-water marks; `large_alloc_*` distinguish external/internal
placement and failure. `PSRAMALLOC <bytes>` (alias `HEAPALLOC`) probes allocation:

```text
PSRAMALLOC OK bytes=<n> placement=<placement> len=<n>
PSRAMALLOC ERR bytes=<n> reason=<reason>
```

## Scheduling

`SCHEDPROFILE` queries the active/automatic profile, volatile override and
readiness. `SCHEDPROFILE AUTO|INTERACTIVE|UPLOAD|DIAGNOSTICS` sets the override;
AUTO removes it. Overrides do not persist. Automatic diagnostics takes precedence
over upload. [Product scheduling](../../../products/meditamer/src/firmware/scheduling.rs)
owns task priorities; locks provide no priority inheritance.

`TOUCHSCHEDRESET` resets acquisition/delivery, bus-service, and poll/wake windows,
without resetting cumulative CPU accounting. `ACQWINDOW` requests 30 seconds
at the existing active IMU cadence; panel/touch suppression remains active.
`CPUPROFILE` snapshots use external temporary storage, with live IRQ counters
internal. IRQ executor work is not also charged as task CPU; poll duration
includes preemption.

`METRICS`/`METRICSNET` are available regardless of telemetry. One dump is admitted
at a time; concurrent requests receive BUSY. Commands can progress between whole
output records. See [metric semantics](metrics.md).

## Telemetry

```text
TELEM
TELEM STATUS
TELEMSET DEFAULT|NONE|ALL
TELEMSET ALL ON|OFF
TELEMSET WIFI|REASSOC|NET|HTTP|SD ON|OFF
TELEM mask=0x<hex> wifi=<on|off> reassoc=<on|off> net=<on|off> http=<on|off> sd=<on|off>
TELEMSET OK mask=0x<hex> wifi=<on|off> reassoc=<on|off> net=<on|off> http=<on|off> sd=<on|off>
```

Invalid syntax returns `CMD ERR`. Aliases: REASSOC=`SCAN`/`WIFI_SCAN`,
NET=`NETWORK`, SD=`STORAGE`. The mask is volatile and never clears metrics,
`CONSOLESTATS`, or `STACKSTATUS`.

| Domain | Logs |
| --- | --- |
| WIFI | Station lifecycle, `NET_EVENT`, backend, successful handoff diagnostic copies |
| REASSOC | Scans, authentication/channel rotation, disconnect reasons |
| NET | DHCP, listener, TCP acceptance |
| HTTP | Requests, errors, accepted connections |
| SD | SD operations emitted by HTTP handlers |

Handoff errors and correlated replies remain enabled. DEFAULT chooses the boot
mask, not the prior state; [device validation](../../guides/diagnostics/device-validation.md#service-and-telemetry-smoke)
owns restoration. Continuous tap/GPIO36 traces, verbose
successful environment delivery (`BME688_DELIVER`), and successful display
refresh timing (`LVGL_REFRESH status=ok`) require `firmware-trace`. Sensor
delivery, refresh policy and health/error reporting remain active without it.
