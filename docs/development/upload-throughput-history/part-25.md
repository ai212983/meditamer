# Upload Throughput History, Part 25

## 2026-03-09: created `backend_legacy_port` staging module for the source-level migration

- Added a compile-clean staging module tree:
  - `src/firmware/storage/upload/wifi/backend_legacy_port/mod.rs`
  - `src/firmware/storage/upload/wifi/backend_legacy_port/bootstrap.rs`
  - `src/firmware/storage/upload/wifi/backend_legacy_port/contracts.rs`
  - `src/firmware/storage/upload/wifi/backend_legacy_port/availability.rs`
- Wired the module into:
  - `src/firmware/storage/upload/wifi.rs`
- The staging module does not switch runtime selection yet.
- It captures the proven working legacy port contract in code:
  - expected bootstrap sequence
  - scheduler bootstrap requirements
  - effective init-config invariants
  - Wi‑Fi task contract
  - scope of the first real port increment (`init/start/scan/stop` first, connect/device path deferred)

Validation:
- `cargo check` passes cleanly.
- LOC:
  - `backend_legacy_port/mod.rs`: `20`
  - `backend_legacy_port/bootstrap.rs`: `37`
  - `backend_legacy_port/contracts.rs`: `57`
  - `backend_legacy_port/availability.rs`: `57`

Why this matters:
- the legacy backend port now has an in-tree source target instead of only history notes and standalone tools
- the next step is to replace staging constants with real ported bootstrap/runtime code, not to invent the contract again

## 2026-03-09: encoded the first real blocker inside `backend_legacy_port`

- Added:
  - `src/firmware/storage/upload/wifi/backend_legacy_port/availability.rs`
- This now records which proven legacy bootstrap hooks are actually reachable from the current stack:
  - available:
    - `enable_wifi_power_domain`
    - `phy_mem_init`
    - `setup_radio_isr`
    - `wifi_set_log_verbose`
    - `init_radio_clocks`
    - `coex_initialize`
  - missing:
    - `preempt::enable`
    - `init_tasks`
    - explicit legacy `initial_yield`

Validation:
- `cargo check` passes cleanly.

Why this matters:
- the migration blocker is now explicit in code, not just inferred from history
- the next port step must move below the firmware layer and either:
  - vendor/recreate the missing scheduler bootstrap semantics, or
  - expose equivalent hooks from the current runtime generation
