# Repository commands

Start with the [development guide](../docs/guides/development/setup.md) for
setup and checks, or [build and flash](../docs/guides/development/build-and-flash.md)
for device work. Run commands from the repository root.

## Common entry points

| Task | Command |
| --- | --- |
| Build Inkplate firmware | `scripts/build/build.sh release default` |
| Build Medinote firmware | `targets/medinote-waveshare/build.sh --locked --bin medinote-waveshare --features crash-screen` |
| Run software checks | `scripts/ci/check_software_baseline.sh <lane>` |
| List host suites | `scripts/host-test.sh --list` |
| Test or lint a host suite | `scripts/host-test.sh <test\|lint> <suite\|all>` |
| Capture host coverage | `scripts/ci/coverage_host.sh` |
| Flash and capture boot | [device/flash.sh](device/flash.sh) |
| Operate or test a device | `scripts/hostctl.sh --help` |
| Capture a passive serial log | [device/monitor.sh](device/monitor.sh) |
| Repeated boot qualification | [device/soak_boot.sh](device/soak_boot.sh) |
| Manual reset-button matrix | [device/cold_boot_matrix.sh](device/cold_boot_matrix.sh) |
| Capture or import touch fixtures | [touch/](touch/) |
| Wi-Fi acceptance and regression | [network validation](../docs/guides/network/validation.md) |

Host test, lint and coverage membership has one owner:
[host-suites.tsv](host-suites.tsv). Test-only crates live in
[test-support/](../test-support/README.md); developer tools live in
[tools/](../tools/README.md).

## Check ownership

[check_software_baseline.sh](ci/check_software_baseline.sh) owns the software
lanes; use `--help` to select one. [CI workflows](../.github/workflows/) and
[lefthook.yml](../lefthook.yml) select lanes for automation.

- `source`: formatting, lock metadata, diff, secrets, BLE patch and network ownership.
- `host-tests` / `host-lint`: registered host behavior tests and strict Clippy.
- `firmware-builds` / `firmware-clippy`: explicit firmware composition matrix.
- `static-source`: stack, FAT-engine, panel-bus and UI ownership guards.
- `static-firmware`: linked memory placement, image capacity and stack budgets.
- `quality`: Markdown length advisory, generated-only includes, Rust source
  reachability and the SLOC/complexity ratchet.

`host`, `firmware` and `static` combine their respective lanes; `all` runs every
lane. These checks establish software properties. Device qualification is owned
by the [diagnostic guides](../docs/guides/README.md#diagnostics).

The panel baseline checks required IRAM/DRAM placement. Changes to panel scan
code or compiler output also require the manual qualification described by
[refresh-probe](../tools/rendering/refresh-probe/README.md).

## Adding a command

First check whether an existing hostctl command, suite or tool can own the job.
Add a script only for a recurring workflow with a current consumer. Keep helpers
beside their owning tool or in `lib/` when genuinely shared. Remove a closed
experiment's implementation after retaining its required conclusions in live
documentation; archived recipes do not make a tool a maintenance requirement.
