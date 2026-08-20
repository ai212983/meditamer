# Vendored crates

This folder holds local copies of a few upstream Rust crates, each with a small patch applied.
They're wired in via `[patch.crates-io]` in the root [`Cargo.toml`](../Cargo.toml), so the build
uses these instead of the plain crates.io versions.

Each subfolder has its own `MEDITAMER_PATCH.md` with the full technical details. This file is just
the map: what's here, and why.

| Crate | Folder | Why it's patched |
| --- | --- | --- |
| `embassy-net` 0.9.1 | [`embassy-net-0.9.1-restartable`](embassy-net-0.9.1-restartable/MEDITAMER_PATCH.md) | Adds a way to reset and reuse network buffers across Wi-Fi restarts. Upstream only supports setting them up once. |
| `esp-alloc` 0.10.0 | [`esp-alloc-0.10.0-provenance`](esp-alloc-0.10.0-provenance/MEDITAMER_PATCH.md) | Fixes a race in the memory-allocator diagnostics hook that could blame the wrong allocation for a low-memory event. |
| `esp-radio` 1.0.0-beta.0 | [`esp-radio-1.0.0-beta.0-bounded`](esp-radio-1.0.0-beta.0-bounded/MEDITAMER_PATCH.md) | Replaces unbounded heap-growing Bluetooth buffers with fixed-size ones, so a stuck peer can't grow memory use without limit. |
| `esp-radio-rtos-driver` 0.3.0 | [`esp-radio-rtos-driver-0.3.0-retained`](esp-radio-rtos-driver-0.3.0-retained/MEDITAMER_PATCH.md) | Replaces a heap-allocated internal queue with fixed static slots, so it can't be freed while something still references it. |
| `esp-backtrace` 0.19.0 | [`esp-backtrace-0.19.0-window-spill-fix`](esp-backtrace-0.19.0-window-spill-fix/MEDITAMER_PATCH.md) | Upstream bug fix backport (part of the boot-crash fix below): a stack-walking helper could corrupt a CPU register. |
| `esp-rtos` 0.3.0 | [`esp-rtos-0.3.0-idle-context-fix`](esp-rtos-0.3.0-idle-context-fix/MEDITAMER_PATCH.md) | Upstream bug fix backport (part of the boot-crash fix below): fixes stack-overflow checking while idle and a stack-size miscalculation. |
| `xtensa-lx-rt` 0.22.0 | [`xtensa-lx-rt-0.22.0-user-mode-vector-fix`](xtensa-lx-rt-0.22.0-user-mode-vector-fix/MEDITAMER_PATCH.md) | Upstream bug fix backport, the main fix for the device boot crash: interrupt handlers weren't setting a CPU mode bit correctly, which could send a nested exception to the wrong handler and crash randomly-looking but fixed code paths. |

## About the three boot-crash backports

`esp-backtrace`, `esp-rtos`, and `xtensa-lx-rt` are patched together because they're one fix:
[esp-rs/esp-hal#6027](https://github.com/esp-rs/esp-hal/pull/6027), which fixed the device panic
(`LoadStoreError`, ~1.1-1.5s after boot) documented in
[ADR-0014](../docs/architecture/0014-single-production-sd-recovery-updater.md)'s addendum. No
released version of those three crates contains that fix yet, so it's applied here by hand onto
the exact versions this project already uses.

**These three are meant to be temporary.** Once `esp-backtrace`, `esp-rtos`, and `xtensa-lx-rt`
all publish a release containing commit `998e4fa`, delete these three folders, remove their
`[patch.crates-io]` lines, and go back to plain crates.io versions.

## How this works, in short

`[patch.crates-io]` tells Cargo "when something asks for crate X from crates.io, use this local
folder instead." The folder still declares the same crate name and version, so nothing else in
the dependency tree needs to change — it's a drop-in swap, not a fork under a new name.
