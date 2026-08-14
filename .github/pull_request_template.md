## Summary

- 

## Validation

- [ ] Host checks passed (`scripts/host-test.sh test hostctl` and CI host regression).
- [ ] If touched files are `>= 420` lines, split plan included.

## Wi-Fi/Upload Regression Evidence (Required If Wi-Fi/Upload Paths Touched)

- [ ] I ran `scripts/tests/hw/test_wifi_regression_gate.sh`.
- [ ] I attached `report.json` and stage logs from the gate run.
- [ ] Panic detection outcome documented (`panic_detected=true|false`).
- [ ] If panic was detected, panic excerpt + troubleshoot log are attached.
- [ ] Discovery proof was completed before any throughput-only mode (`HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE=0`).

## Notes / Risk

- 
