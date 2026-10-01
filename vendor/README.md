# Vendored crates

Local copies of upstream crates with small project patches. Cargo uses them
instead of the crates.io versions via `[patch.crates-io]`. The workspace root
and each firmware target (`targets/*/Cargo.toml`) select the patches they need.
Both firmware targets patch `esp-hal`; only Inkplate enables its temporary CPU
profiling feature and patches `esp-rtos` for context diagnostics. Each folder
keeps the upstream name and version and documents its exact changes in its own
`MEDITAMER_PATCH.md`.

| Crate | Folder | Why it's patched |
| --- | --- | --- |
| `proc-macro-error2` 2.0.1 | [proc-macro-error2-2.0.1-public-proc-macro](proc-macro-error2-2.0.1-public-proc-macro/MEDITAMER_PATCH.md) | Makes its `proc_macro` re-export compatible with the current Rust compiler. |
| `embassy-net` 0.9.1 | [embassy-net-0.9.1-restartable](embassy-net-0.9.1-restartable/MEDITAMER_PATCH.md) | Adds a reset for network buffers, so one static block can be reused across Wi-Fi restarts. |
| `trouble-host` 0.8.0 | [trouble-host-0.8.0-restartable](trouble-host-0.8.0-restartable/MEDITAMER_PATCH.md) | Frees held BLE packets when the host is dropped, so a new session can reuse the same slots. |
| `esp-alloc` 0.11.0 | [esp-alloc-0.11.0-provenance](esp-alloc-0.11.0-provenance/MEDITAMER_PATCH.md) | Fixes low-memory reports blaming the wrong allocation when both cores allocate at once. |
| `esp-backtrace` 0.20.0 | [esp-backtrace-0.20.0-custom-pre-backtrace-info](esp-backtrace-0.20.0-custom-pre-backtrace-info/MEDITAMER_PATCH.md) | Passes panic details to the crash-screen hook. A project-owned extension, not an upstream fix; only Medinote uses it. |
| `esp-radio` 1.0.0-beta.1 | [esp-radio-1.0.0-beta.1-bounded](esp-radio-1.0.0-beta.1-bounded/MEDITAMER_PATCH.md) | Fixed-size Bluetooth buffers and overflow counters instead of unbounded growth. |
| `esp-radio-rtos-driver` 0.4.2 | [esp-radio-rtos-driver-0.4.2-retained](esp-radio-rtos-driver-0.4.2-retained/MEDITAMER_PATCH.md) | Fixed queue slots instead of heap allocation that could be freed while still in use. |
| `esp-hal` 1.2.0 | [esp-hal-1.2.0-cpu-profile](esp-hal-1.2.0-cpu-profile/MEDITAMER_PATCH.md) | Adds temporary Inkplate CPU-load hooks and corrects ESP32-S3 flash-text section flags. Remove the hooks once the investigation closes. |
| `esp-rtos` 0.4.0 | [esp-rtos-0.4.0-context-probe](esp-rtos-0.4.0-context-probe/MEDITAMER_PATCH.md) | Temporary Xtensa context-copy diagnostic (first implausible `THREADPTR` per core in RTC slow RAM) plus an out-of-line cold timer error formatter. Only the workspace root and the Inkplate target patch it; Medinote keeps registry 0.4.0. Remove once the context writer is identified and the defect is repaired. |
