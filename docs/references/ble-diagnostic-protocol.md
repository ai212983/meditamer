# BLE diagnostic protocol

Inkplate's diagnostic v1 service accepts one unauthenticated connection. It does
not pair, bond or expose application controls. See the
[handoff procedure](../guides/network/validation.md#inkplate-wi-fible-handoff) for opening a window.

## Discovery and lifetime

Advertising carries the service UUID; the scan response names `Meditamer`.
The advertising interval is 250 ms. Each controller run uses a new random-static
address, so clients must rediscover rather than treat a cached address as identity.
Select by service UUID and validate versions and characteristics. Multiple matching
devices are ambiguous; the name or build prefix does not authenticate a device.

| Deadline | Limit |
| --- | ---: |
| Advertising without a connection | 60 s |
| Connection | 60 s |
| Connected idle | 30 s |
| Complete window | 120 s |

GATT activity resets the idle timer, not the connection or complete-window timers.
Disconnect closes the window. An unauthenticated client can occupy the connection
until a deadline; the service does not guarantee availability against persistent
nearby interference.

## GATT values

The primary service UUID is `BE836D30-5EA2-4E16-B476-4D32E4F7CBCE`.

| Characteristic | UUID | Properties |
| --- | --- | --- |
| Build Info | `BE836D30-5EA2-4E16-B476-4D32E4F7CBCF` | Read |
| Echo | `BE836D30-5EA2-4E16-B476-4D32E4F7CBD0` | Read, Write Request, Notify |
| Lifecycle Status | `BE836D30-5EA2-4E16-B476-4D32E4F7CBD1` | Read, Notify |

### Build Info

Exactly 16 bytes. Schema and protocol versions are currently 1.

| Offset | Bytes | Field |
| --- | ---: | --- |
| 0 | 1 | Schema version |
| 1 | 1 | Protocol version |
| 2 | 1 | Capabilities: bit 0 Echo, bit 1 Lifecycle Status |
| 3 | 1 | Reserved, zero |
| 4 | 8 | Build digest prefix |
| 12 | 4 | Reserved, zero |

The current prefix is the first eight bytes of SHA-256 over `BUILD_ID`, **not a
build-manifest digest**. 

### Echo

Subscribe before writing to receive the echoed notification. Payloads are 1–32
bytes; accepted writes are limited to four per second and 16 per connection.
Invalid lengths and exhausted quotas reject the write. The current server ignores
notification errors; a successful write does not prove notification delivery.

### Lifecycle Status

Exactly eight bytes; multi-byte fields are little-endian.

| Offset | Bytes | Field |
| --- | ---: | --- |
| 0 | 1 | State: 0 advertising, 1 connected, 2 connected-idle, 3 closing |
| 1 | 2 | Remaining seconds |
| 3 | 2 | RX drops |
| 5 | 2 | TX timeouts |
| 7 | 1 | Reserved, zero |

Status is a snapshot. The current service leaves the drop and timeout fields at
zero; they do not establish error-free transport.
