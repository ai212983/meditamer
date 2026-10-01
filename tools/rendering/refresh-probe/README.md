# Inkplate refresh performance

This probe compares the official Soldered Inkplate Arduino InkplateLibrary 11.1.4
with Meditamer's Rust `InkplateHal` driver on the same 600 by 600 test images.
Durations cover software transactions, not electrical signal timing.

## Current timings

| Refresh mode | Power state | Samples each | Reference | Our driver | Difference | Status |
|---|---|---:|---:|---:|---:|---|
| 1-bit partial | warm; panel left on | 100 | 182.167 ms | 142.081 ms | -40.086 ms (-22.01%) | Production |
| 1-bit full | cold; power cycle per update | 100 | 1,049.178 ms | 919.022 ms | -130.156 ms (-12.41%) | Production |
| Native 3-bit full | cold; power cycle per update | 100 | 1,028.997 ms | 2,489.325 ms | +1,460.328 ms (+141.92%) | Production |

The performance rows use one unmeasured mode-matched warm-up, identical fixtures,
and a 250 ms refresh-end-to-start cadence. Native 3-bit full is the current
conservative production implementation.

## Where the time goes

Profile builds are separate instrumented binaries. Use them for attribution,
not as headline transaction timings.

### 1-bit partial

Both columns are matched 20-sample warm phase profiles.

| Transaction phase | Reference | Our driver |
|---|---:|---:|
| Preparation | 19.439 ms | 15.526 ms |
| Power on / already-on check | 0.001 ms | 0.033 ms |
| Waveform / scan | 158.489 ms | 124.552 ms |
| Finalization | 0.000 ms | 0.027 ms |
| Previous-frame copy | 4.259 ms | 2.071 ms |
| Timing residual | 0.002 ms | 0.203 ms |
| **Profiled transaction total** | **182.194 ms** | **142.413 ms** |

Preparation breakdown:

| Preparation phase | Reference | Our driver |
|---|---:|---:|
| Changed-row discovery | not separated | 0.058 ms |
| Transition preparation | not separated | 15.468 ms |

Waveform / scan breakdown:

| Waveform phase | Reference | Our driver |
|---|---:|---:|
| Nine transition scans | 125.677 ms | 123.496 ms |
| Cleanup / gate drain | 32.813 ms | 1.057 ms |
| **Waveform / scan total** | **158.489 ms** | **124.552 ms** |

The Rust gate-drain waveform omits the reference library's three cleanup source
passes and performs only the final gate operation.

### 1-bit full

Both columns are matched 20-sample cold phase profiles.

| Transaction phase | Reference | Our driver |
|---|---:|---:|
| Preparation / previous-frame copy | 5.114 ms | 2.176 ms |
| Power on | 45.592 ms | 40.518 ms |
| Waveform / scan | 861.690 ms | 744.076 ms |
| Power off / finalization | 135.588 ms | 132.962 ms |
| Timing residual | 0.010 ms | 0.038 ms |
| **Profiled transaction total** | **1,047.997 ms** | **919.909 ms** |

Waveform / scan breakdown:

| Waveform phase | Reference | Our driver |
|---|---:|---:|
| Initial 65 clean passes | 693.844 ms | 619.415 ms |
| Ten framebuffer passes | 131.941 ms | 95.673 ms |
| Settle pass | 13.748 ms | 9.609 ms |
| Final two clean passes | 21.347 ms | 19.087 ms |
| Terminal vertical scan | 0.809 ms | 0.291 ms |
| **Waveform / scan total** | **861.690 ms** | **744.076 ms** |

### Native 3-bit full

Both columns are matched 20-sample cold phase profiles using the shared numbered
packed-grayscale fixture.

| Phase | Reference | Our driver |
|---|---:|---:|
| Preparation / previous-frame copy | 0.000 ms | 0.000 ms |
| Power on | 45.723 ms | 43.836 ms |
| Waveform / scan | 847.719 ms | 2,309.941 ms |
| Power off / finalization | 135.542 ms | 135.238 ms |
| Timing residual | 0.010 ms | 0.027 ms |
| **Profiled transaction total** | **1,028.997 ms** | **2,489.140 ms** |

Waveform / scan breakdown:

| Waveform phase | Reference | Our driver |
|---|---:|---:|
| Initial 65 clean passes | 692.933 ms | 1,917.581 ms |
| Eight framebuffer passes | 143.335 ms | 361.755 ms |
| Final clean pass | 10.656 ms | 29.876 ms |
| Terminal vertical scan | 0.794 ms | 0.728 ms |
| **Waveform / scan total** | **847.719 ms** | **2,309.941 ms** |

For build and measurement instructions, see [DEVELOPMENT.md](DEVELOPMENT.md).

## Qualification

The automated [panel guard](../../../scripts/ci/check_panel_waveform_placement.sh)
checks required IRAM/DRAM placement. After scan-source or compiler changes, run
[check_scan_contract.sh](check_scan_contract.sh) against the affected release
ELF, then qualify the panel physically using the workflow above. Compiled
instruction checks do not establish electrical timing or visual correctness.
