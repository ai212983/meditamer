# touch_replay

Deterministic touch-gesture replay harness for the statig touch engine.

## Usage

```bash
cargo run \
  --target "$(rustc -vV | awk '/^host:/ {print $2}')" \
  --manifest-path tools/touch_replay/Cargo.toml -- \
  tools/touch_replay/fixtures/tap_trace.csv \
  --expect tools/touch_replay/fixtures/tap_expected.txt
```

The tool prints decoded events as CSV and exits non-zero if expected kinds do not match.

## Run bundled fixtures

```bash
tools/touch_replay/run_fixtures.sh
```

Bundled fixtures cover tap, long-press, short-drag-no-swipe, all swipe directions, multitouch cancel, and a diagonal drag that must not classify as swipe.

## Capture real traces from device

1. Capture serial monitor output while interacting with the touch panel:

```bash
ESPFLASH_PORT=/dev/cu.usbserial-XXXX scripts/touch/touch_capture.sh
```

2. Build replay fixtures from that capture:

```bash
scripts/touch/make_touch_fixture.sh logs/touch_trace_YYYYMMDD_HHMMSS.log my_real_case
```

This produces:

- `tools/touch_replay/fixtures/my_real_case_trace.csv`
- `tools/touch_replay/fixtures/my_real_case_expected.txt`

When capture logs contain `touch_event,...` lines, expected kinds are sourced directly from device-decoded events. Otherwise they are derived from replay output.

3. Replay and verify:

```bash
tools/touch_replay/run_fixtures.sh
```

## Expected file format

One event kind per line:

- `down`
- `move`
- `up`
- `tap`
- `long_press`
- `swipe_left`
- `swipe_right`
- `swipe_up`
- `swipe_down`
- `cancel`
