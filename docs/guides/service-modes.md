# Runtime Setup and Service Modes


## Allocator Diagnostics

Firmware accepts allocator status commands on `UART0` (`115200` baud):

```text
PSRAM
```

Aliases: `HEAP`, `ALLOCATOR`.

Response format:

```text
PSRAM feature_enabled=<bool> state=<state> total_bytes=<n> used_bytes=<n> free_bytes=<n> peak_used_bytes=<n> internal_free_bytes=<n> external_free_bytes=<n> min_free_bytes=<n> min_internal_free_bytes=<n> min_external_free_bytes=<n> large_alloc_external_ok=<n> large_alloc_internal_ok=<n> large_alloc_fail=<n>
```

- `internal_free_bytes` tracks capability-constrained internal RAM available for Wi-Fi/radio allocations.
- `min_*` values are boot-lifetime low-water marks to identify monotonic pressure during soak runs.
- `large_alloc_*` counters show where `alloc_large_byte_buffer` requests landed (external vs internal fallback).

Allocator probe command:

```text
PSRAMALLOC <bytes>
```

Alias: `HEAPALLOC <bytes>`.

Probe responses:

```text
PSRAMALLOC OK bytes=<n> placement=<placement> len=<n>
PSRAMALLOC ERR bytes=<n> reason=<reason>
```

## Runtime Service Modes

Runtime mode controls are available over `UART0` (`115200` baud):

```text
STATE GET
STATE SET upload=on
STATE SET upload=off
STATE DIAG kind=debug targets=SD|WIFI
DIAG GET
```

Response format:

```text
STATE phase=<...> upload=<on|off> diag_kind=<...> targets=<NONE|SD|WIFI|DISPLAY|TOUCH|IMU> ready=<true|false>
DIAG state=<idle|running|done|failed|canceled> targets=<...> step=<...> code=<...>
```

Notes:

- App state is persisted in flash and restored on boot.
- `STATE SET` returns `OK` only after the state update is applied by runtime tasks.
- `STATE SET upload=off` rejects upload operations and releases upload transfer buffers.
- Upload transfer buffers are allocated in PSRAM on demand and released when upload mode is
  disabled.

Quick RAM check sequence:

```text
PSRAM
STATE SET upload=on
PSRAM
STATE SET upload=off
PSRAM
```

Automated smoke run (mode toggles + PSRAM snapshots):

```bash
scripts/hostctl.sh test runtime-modes-smoke --suite full
```

Optional env var:

- `HOSTCTL_MODE_SMOKE_SETTLE_MS` (default `0`; can be raised if extra post-command delay is desired)
