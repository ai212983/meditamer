# Platform crates

`platform/` owns reusable contracts and services shared by products, boards,
targets, and host tools. The ownership rules and dependency direction are
defined by the [platform, board, product and target boundaries](../docs/architecture/platform-board-product-target-boundaries.md).

Capability directories are a navigation aid, not dependency layers. A platform
crate may depend on a crate in another capability when that dependency follows
the crate's real responsibility. Platform crates never depend on a product,
board, or target crate.

| Capability | Crates | Responsibility |
| --- | --- | --- |
| `audio/` | `buzzer` | Single-oscillator buzzer scores, execution contract, and diagnostic operations shared by firmware and host tools |
| `connectivity/` | `arbitration`, `ble`, `http-upload`, `netstack` | Radio ownership, BLE and Wi-Fi sessions, networking, and network-facing transfer protocols |
| `diagnostics/` | `console`, `cpu-load`, `firmware-trace`, `runtime` | Diagnostic output, execution measurements, event capture, and diagnostic filtering |
| `memory/` | `asset-source`, `phase-arena`, `psram-store`, `residency-policy`, `transfer-cache-block-pool` | Generic checked read/borrow access core; single-activation checked offset arena; bounded chunked byte store; transactional residency policy foundation; fixed-block transfer/cache pool |
| `sensing/` | `observation` | Product-neutral observation subscriptions, scheduling, and delivery |
| `storage/` | `sdcard` | Storage transport, FAT operations, and shared storage service contracts |
| `time/` | `rtc`, `wall-clock` | Wall-clock hardware access and synchronization policy |
| `ui/` | `board`, `render`, `shell` | Display contracts, checked rendering, and application-shell lifecycle |
| `ui/visuals/` | `raster`, `enso`, `flipclock`, `hourglass` | Shared raster contracts and reusable visual models |
| `update/` | `bundle`, `otadata` | Firmware bundle validation and boot-selection metadata |

Each leaf listed above is an independent Cargo package. Add a new crate to the
capability that owns its primary contract; do not create a generic `common`,
`core`, or `utils` bucket. If ownership crosses the platform, board, product,
or target axes, resolve that boundary before choosing a directory.

Keep workspace membership explicit in the [root manifest](../Cargo.toml). The
list is the build inventory and prevents a new directory from silently becoming
part of the shared host-test workspace.
