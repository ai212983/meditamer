# Cold Boot Validation

- Status: Done (reset-cycle evidence accepted; true power-rail cold boot remains a tracked limitation)
- Last-reviewed: 2026-08-13

- [x] Run the reset-button boot-path matrix and record the result as REL-001's accepted evidence.

Why the original plan (true power-off) was replaced:
- Inkplate 4 TEMPERA has a built-in, non-user-removable battery and no on/off power switch —
  confirmed against hardware in hand: the only physical control besides USB is a reset button.
- USB unplug/replug alone is not a valid cold boot and produced unreliable serial capture results
  (the board stays powered from battery).
- A reset-button press exercises the ESP32 boot sequence but does not necessarily cut power to
  peripherals whose rails aren't gated by that button (e.g. the e-ink PMIC, SD rail) — so it does
  not establish the same guarantee a true power-rail cold boot would. This gap is accepted as a
  residual, tracked limitation rather than blocking REL-001 indefinitely.

Result (2026-08-13):
- `ESPFLASH_PORT=/dev/cu.usbserial-2110 COLD_BOOT_LOG_DIR=logs/source-tree-cleanup/rel-001
  scripts/device/cold_boot_matrix.sh 5` → `reset-cycle summary: pass=5 fail=0 cycles=5`.
- All 4 required markers (`BOOT_RESET reason=`, `touch: ready phase=`, `LVGL init=ready`,
  `RUNTIME_READY app_state=ready display=ready`) present in every cycle log; each log 99.99-100%
  printable ASCII, no binary/noise-only captures.
- Evidence identity: source HEAD `803077a0816b191342f80c7ee1d2edaba6eafb1c`, snapshot commit
  `dc7178eda5f088963f14809d0255ebf1c6cdb59d`, device `/dev/cu.usbserial-2110`, logs under
  `logs/source-tree-cleanup/rel-001/`.

| Cycle | Log SHA-256 |
| ---: | --- |
| 1 | `ec9bb855622588ca64f3e246bd9f749760b2cc2206e5a0f1fd55d9eb542f6689` |
| 2 | `c2ab69e0554fc93b23385108e40bf481603f387493ee55047dfae9cf26444a4e` |
| 3 | `5a5d40b234d9068b87a0bdd46f556348dbffce473cd9027ec3f4aa0c72968d69` |
| 4 | `ce7023af7ac35270035ba95d30453e8f789b3f1bde9c67f80132015d2eb3fbb0` |
| 5 | `72e698f46302f3a8d6235889b2bcc6821df1efb47b927423849cc44f665c5ebc` |

Two real script bugs were found and fixed while producing this evidence (not device/firmware
issues): the serial port is now held open while its baud is configured, and capture waits for a real
reset marker before evaluating the current boot-marker set.

Unblock conditions for the residual true-power-rail gap (either would supersede this evidence):
- Identify a true power-cut method this board actually supports (e.g. disassembly to the internal
  battery JST connector); or
- A bug is suspected that only manifests after genuine power loss, motivating renewed investment in
  a true power-cut method.

References:
- [Inkplate 4TEMPERA FAQ & Troubleshooting](https://docs.soldered.com/inkplate/4tempera/faq-troubleshooting/)
- [Inkplate 4TEMPERA Quick Start Guide](https://docs.soldered.com/inkplate/4tempera/quick-start-guide/)
- scripts/device/cold_boot_matrix.sh
