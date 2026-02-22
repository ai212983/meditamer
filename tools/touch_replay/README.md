# touch_replay

Deterministic touch-gesture replay harness for the statig touch engine.

## Usage

```bash
RUSTFLAGS='' cargo +stable-aarch64-apple-darwin run \
  --target aarch64-apple-darwin \
  --manifest-path tools/touch_replay/Cargo.toml -- \
  tools/touch_replay/fixtures/tap_trace.csv \
  --expect tools/touch_replay/fixtures/tap_expected.txt
```

The tool prints decoded events as CSV and exits non-zero if expected kinds do not match.

The `RUSTFLAGS=''` override is required because the firmware root config injects ESP linker flags that
are not valid for host binaries.

## Run bundled fixtures

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
