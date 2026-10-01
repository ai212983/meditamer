# Memory validation

Read the affected [target budget](../../references/memory/README.md) first.
Validate each target's production composition (Medinote Wi-Fi remains opt-in):

```bash
scripts/build/build.sh release default
targets/medinote-waveshare/build.sh --locked --bin medinote-waveshare --features crash-screen,wifi-storage
```

Record target, binary, profile/features, commit and dirty-source identity, ELF hash,
section totals and relevant task/frame sizes in the build report. Use the matching
Xtensa `size -A`, `nm -C -S --size-sort` and `objdump` tools; task pools are persistent
data, not CPU call frames. Measure before/after on otherwise equivalent compositions.

Run the affected guards from the repository root:

```bash
scripts/ci/check_stack_risk.sh --inkplate-elf target/xtensa-esp32-none-elf/release/meditamer
scripts/ci/check_ble_image_budget.sh
scripts/ci/check_bt_dram_reservation.sh target/xtensa-esp32-none-elf/release/meditamer
scripts/ci/check_pinned_linker_scripts.sh
scripts/ci/check_panel_waveform_placement.sh target/xtensa-esp32-none-elf/release/meditamer
scripts/ci/check_iram_flash_refs.sh target/xtensa-esp32-none-elf/release/meditamer
scripts/ci/check_stack_risk.sh --medinote-elf targets/medinote-waveshare/target/xtensa-esp32s3-none-elf/release/medinote-waveshare
```

## Placement and runtime qualification

Requalify material placement, allocator, capacity, workload or BLE-composition
changes on the identified candidate. For Inkplate, inspect ordered ELF symbols
against the [ROM-stack placement rules](../../references/memory/meditamer-inkplate/rom-stack.md#required-placement).
After heap-region count/placement changes, run at least 40 boots. Exercise active
BLE epochs and Wi-Fi/BLE handoff, upload/SD/display overlap, allocation rejection
and cleanup; include supported sleep/wake paths.

Capture current internal free, allocatable contiguous capacity, heap minimum and
CPU-stack headroom. Use [runtime metrics](../../references/runtime/metrics.md),
[allocator commands](../../references/runtime/serial-control.md) and the [Wi-Fi gate](../network/validation.md)
for Inkplate; record target-specific S3 observations separately. Compare live
capacity and historical minima using the target budget's acceptance criteria.

Retain the method, artifact identity, result, limitations and governing constraint
in maintained documentation when qualification changes. Raw reports/logs may be
deleted; required rationale must survive without them or the archive.

## Asset-residency baseline capture

The bounded baseline workflow resets already-flashed firmware, normalizes
upload service off, captures Ambient and two clock activations, then toggles
upload service without transferring bytes. It writes the raw serial log and a
sibling JSON report containing labeled markers and parsed PSRAM snapshots.

```bash
HOSTCTL_PORT=$DEVICE_PORT scripts/hostctl.sh \
  test asset-residency-baseline \
  --feature-label default-release \
  --build-label app-sha256-<app-bin-sha256> \
  --marker-timeout-secs 360 \
  --output logs/asset_residency_baseline_<timestamp>.log
```

Use the exact flashed artifact hash as the build label. The longer marker
ceiling covers Ambient's five-minute parent composition cadence; it is not an
upload or allocator tuning knob. The workflow never flashes firmware or sends
upload bytes, and always attempts to restore upload service off before writing
its report. If the preflight upload state is unreadable, it retries the query
and requires an upload-off result before waiting for Mountain. It does not
issue `STATE SET` while a possible SD job owns the console. Mountain
composition is checked through the correlated `MOUNTAINSTATUS` response after
overlay adoption because the unsolicited composition line can drop.
If a `CLOCK_ASSETS` UART marker is dropped, a completed Clock frame can prove
that assets were available; the report marks the asset checkpoint `inferred`.
The same frame can prove screen entry if `CLOCK_SCREEN` is dropped, and that
entry checkpoint is also marked `inferred`. Do not derive screen-entry or
asset-load timing, or a load-only memory snapshot, from those inferred
checkpoints or the subsequent `clock_loaded` snapshot.

## Same-boot asset re-entry qualification

After the baseline leaves the device on Home, the re-entry workflow runs 2–10
Mountain Home → Clock → Home cycles without resetting or reflashing. Every
cycle requires a correlated `MOUNTAINSTATUS` reply showing that the active UI
adopted and composed the overlay, a resident pack release, Clock's first frame
and release, upload-mode on/off, PSRAM snapshots, and absence of a reset or
retained fault. A dropped cache-validation UART line is labeled `inferred`
only when the matching resident pack release is captured. The timing line is
validated when present and marked `missing` when dropped; a missing timing
line supplies no elapsed-time or SD-range measurement. It needs
firmware with `UISTEP AMBIENT_VIEW`; both named launch targets go through
Launcher. It records separate checkpoints for each cycle, fails on an
ambiguous UI acknowledgement, and restores upload mode off on exit.

```bash
HOSTCTL_PORT=$DEVICE_PORT scripts/hostctl.sh \
  test asset-residency-reentry \
  --feature-label segmented-resident-candidate \
  --build-label app-sha256-<app-bin-sha256> \
  --cycles 5 --marker-timeout-secs 90 \
  --output logs/asset_residency_reentry_<timestamp>.log
```

This is a repeatability and allocation-churn measurement, not a published
numeric admission budget. It does not transfer upload bytes or establish
visual/touch responsiveness; use the Wi-Fi gate and one observed physical
cycle for those separate checks.
