# Buzzer listening and A/B trials

This guide owns the standalone `buzzer-probe` workflow. It replaces the normal
application with a bounded diagnostic image, plays on boot, then shuts down
the rail and reads back its output latch. It has no product UI or serial command
service. General setup is in [build-and-flash.md](../development/build-and-flash.md);
hardware limits are in the [sound reference](../../references/hardware/inkplate/sound.md).
Remaining qualification and parked tuning are in the
[buzzer plan](../../plans/inkplate-buzzer-firmware.md).

## Choose a mode

Pitches below are nominal requests, not measured frequencies. A and B are
controls within each mode. Each A/B mode plays A, B, A, B; the first sound
starts about 1.5 seconds after initialization. Omitting `BUZZER_PROBE_MODE`
selects `chime`. Existing experimental modes support reproduction; they do
not imply that further tuning is planned.

| Build mode | Playback or A | B | Expected `Completed` runs |
| --- | --- | --- | --- |
| `notifications` | ack 313 ms, attention 313 ms, complete 500 ms | Single pass | 3 |
| `melody` | Twinkle opening two lines, 8000 ms | Single pass | 1 |
| `manufacturer-melody` | Vendor loop rhythm, octave x2, 11200 ms | Single pass | 1 |
| `steady` | Raw codes 0, 63, 127 held 650 ms each | Single pass | 3 |
| `chime` | Reference beep, strike/body and stepped-attack candidates | Three candidates, two rounds | 6 |
| `descending-chime` | 1974 Hz (code 17) 100 ms + 250 ms rest + 1974 Hz 50 ms + 1000 ms rest | Second note 1566 Hz (code 29) | 4 |
| `ring-down` | 2700 Hz (code 5) 100 ms + 1000 ms rest | 20 ms burst, same pitch/rest | 4 |
| `pitch-alternation` | 900/1450 Hz, 100 ms per pitch, 2 s total | 10 ms per pitch | 4 |
| `gradual-drop` | 75% to 25% duty drop at 1 s, 2 s total | Gradual mix of those pulse widths | 4 |
| `single-drop-long` | Constant 75% duty, 2 s total | 75% then 25% at 1 s | 4 |
| `single-drop` | Constant 75% duty, 4 ms slots | 75% then 25% at 300 ms | 4 |
| `constant-duty` | Constant 75% duty, 4 ms slots | Constant 25% duty | 4 |
| `fast-envelope` | Constant 75% duty, 4 ms slots | 75%, 50%, 25% duty blocks | 4 |
| `fast-cadence` | 75%, 50%, 25% duty blocks, 8 ms slots | Same duty blocks, 4 ms slots | 4 |
| `cadence` | 80% to 30% duty blocks, 10 ms slots | Same duty blocks, 20 ms slots | 4 |
| `envelope` | Constant 8 ms pulse width, 10 ms slots | Width falls from 8 to 3 ms | 4 |
| `density` | Every 10 ms slot sounds | Fewer slots sound over time | 4 |

Rail-gating modes use code 63 and 600 ms except the two long modes and
`gradual-drop` (2 seconds). Primed modes are probe-only; they do not qualify
rail discharge/reset behaviour. Experimental modes are retained for reproduction.

`pitch-alternation` keeps the rail on, alternating raw codes 73/34 without
rests. `ring-down` uses normal note programming without priming or gating;
its final rest supplies exactly 1 second of silence, with no extra score pause.

`manufacturer-melody` transposes the vendor rhythm up one octave. Baseline example:
`../Inkplate-Arduino-library/examples/Inkplate4TEMPERA/Advanced/Sensors/Inkplate4TEMPERA_Buzzer/Inkplate4TEMPERA_Buzzer.ino`.

## RTTTL notification import and audition

Author short notifications in `assets/sounds/notifications.rtttl`, one score
per line; blank and `#` lines are skipped. The host compiler generates borrowed
static scores; firmware does not parse RTTTL. The existing bank contains `ack`,
`attention` and `complete` in C6..G6. They remain listening candidates.

Preview a string without hardware:

```bash
cargo run --manifest-path tools/buzzer_preview/Cargo.toml --bin buzzer_preview \
  -- --out logs/ack.wav --rtttl "ack:d=16,o=6,b=120:c,32p,e"
cargo run --manifest-path tools/buzzer_preview/Cargo.toml --bin buzzer_preview \
  -- --out logs/ack-up.wav --rtttl "ack:d=16,o=6,b=120:c,32p,e" --transpose 2
```

The preview renders nominal square-wave timing; acoustic filtering and
ring-down are unmeasured. Use device recordings to assess timbre.

`--transpose N` shifts every note by N semitones; rests and durations stay
unchanged. An out-of-range note rejects the whole score with that note
identified. Notes are never individually clamped or transposed.

Regenerate the module after changing the catalogue:

```bash
cargo run --manifest-path tools/buzzer_preview/Cargo.toml --bin rtttl_catalogue \
  -- --in assets/sounds/notifications.rtttl \
     --out targets/meditamer-inkplate/src/buzzer_notifications.rs
```

Append `--transpose N` to apply one shift to the whole catalogue; zero is the
default. Generated data records the shift and oscillator parameters. Failed
regeneration leaves the previous module intact. Host regeneration tests check
that generated modules match their catalogues byte-for-byte.

For authoring, use only `d`/`o`/`b` header keys without duplicates, octaves 4..7,
tempo 1..1024 bpm, at most 256 notes/rests and at most 30 seconds per score.
Adjacent notes stay continuous: write `p` rests for separate beeps. Fractional
beats use cumulative millisecond rounding. Detailed grammar and software
contracts live in the [RTTTL compiler](../../../tools/buzzer_preview/src/rtttl.rs),
[board driver](../../../boards/inkplate-tempera/src/buzzer.rs) and
[score runner](../../../platform/audio/buzzer/src/runner.rs).

Build with `BUZZER_PROBE_MODE=notifications` using the workflow below. Expect
one run per entry in catalogue order, with 700 ms gaps. Ready output reports
candidate names/durations and transpose; the probe rejects generated data
whose oscillator parameters no longer match the board model.

## Melody catalogue

Regenerate `assets/sounds/melodies.rtttl` with the same compiler:

```bash
cargo run --manifest-path tools/buzzer_preview/Cargo.toml --bin rtttl_catalogue \
  -- --in assets/sounds/melodies.rtttl \
     --out targets/meditamer-inkplate/src/buzzer_melodies.rs
```

Select `BUZZER_PROBE_MODE=melody`; the mode table owns expected completion.

## Build, play and restore

Select a mode and build with the production linker/toolchain setup:

```bash
BUZZER_PROBE_MODE=notifications FIRMWARE_BIN=buzzer-probe CARGO_FEATURES=buzzer-probe \
  scripts/build/build.sh release minimal
```

Set `DEVICE_PORT` to the connected Inkplate UART port. Flash and capture through
the canonical workflow, using a fresh log directory for each run. A 20-second
boot window covers every mode above:

```bash
ESPFLASH_PORT="$DEVICE_PORT" FLASH_SET_TIME_AFTER_FLASH=0 \
  HOSTCTL_FLASH_CAPTURE_IMAGE=target/xtensa-esp32-none-elf/release/buzzer-probe \
  HOSTCTL_FLASH_CAPTURE_BOOT_WINDOW_MS=20000 \
  HOSTCTL_FLASH_CAPTURE_LOG_PATH=logs/buzzer-listening-probe \
  scripts/device/flash.sh release
```

Require the expected number of `Completed` outcomes, zero skipped actions, and
`BUZZER_PROBE complete rail_latch=off output=enabled readback=ok`. These establish
execution and latch readback, not measured pitch or acoustic quality. Firmware
applied-action counts exclude the terminal shutdown; host traces include it.

For comparisons, change one variable at a time. Capture board identity, build,
settings and raw recordings with fixed microphone geometry/gain and AGC off.
Assess recognition, clarity and pleasantness separately. No playback outcome
alone selects a product preset.

The probe replays on reset. Restore the normal application using the standard
production build/flash workflow in [build-and-flash.md](../development/build-and-flash.md),
with probe-specific overrides removed.
