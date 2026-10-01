## 2026-03-19: Debugger path is blocked by attached hardware exposing only USB-UART, not JTAG

### New artifacts

- `docs/development/2026-03-19-wifi-history-narrowing-followup-37.md`

### What changed

- Verified local availability of the Espressif debugger stack.
- Verified what the currently attached device exposes over USB on this host.

### What is now proven

1. The host has the debugger tools installed.

Available locally:
- OpenOCD:
  - `~/.espressif/tools/openocd-esp32/v0.12.0-esp32-20250707/openocd-esp32/bin/openocd`
- Xtensa GDB:
  - `~/.espressif/tools/xtensa-esp-elf-gdb/16.3_20250913/xtensa-esp-elf-gdb/bin/xtensa-esp32-elf-gdb`

So the blocker is not missing software.

2. The currently attached board exposes only a USB serial bridge.

Host-visible serial device:
- `/dev/cu.usbserial-410`

USB inspection shows:
- product string: `USB Serial`
- vendor id: `6790` (`0x1a86`)

That is a CH34x-style USB-UART bridge class, not a visible JTAG/debug adapter.

3. No debugger-capable USB interface is visible for the attached board.

The current USB inspection did not surface:
- Espressif USB JTAG
- FTDI/JTAG adapter
- CMSIS-DAP/J-Link/ST-Link style debug interface

So the previously recommended OpenOCD/JTAG step cannot be executed against the currently attached hardware as-is.

### Current boundary

The investigation is still narrowed to:
- runtime production/admission of the MAC interrupt event word before `hal_mac_interrupt_get_event()` sees it

But the preferred next method is blocked because:
- the board currently gives us UART only
- not a JTAG/debug transport

### What this closes

- “just attach OpenOCD now and continue”
- “the blocker is missing debugger binaries on the host”

### Best next step

To continue on the debugger path, one of these is required:
- a board/debug setup with actual JTAG access
- a supported external debug probe wired to the ESP32 JTAG pins
- or a new firmware-side instrumentation approach deeper than the current wrapper method

Without one of those, this branch is blocked at the hardware/debug-transport layer.
