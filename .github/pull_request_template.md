## Summary

- 

## Validation

- [ ] Local software baseline passed (`scripts/ci/check_software_baseline.sh all`).
- [ ] Production Rust files respect the 600/1000 SLOC ratchet; any real module-boundary split follows `AGENTS.md`.

## Wi-Fi/Upload Regression Evidence (Required If Wi-Fi/Upload Paths Touched)

- [ ] I ran `scripts/tests/hw/test_wifi_regression_gate.sh`.
- [ ] I attached `report.json` and stage logs from the gate run.
- [ ] Panic detection outcome documented (`panic_detected=true|false`).
- [ ] If panic was detected, panic excerpt + troubleshoot log are attached.
- [ ] Discovery proof was completed before any throughput-only mode (`HOSTCTL_NET_REQUIRE_BOOT_DISCOVERY_GATE=0`).

## Notes / Risk

- 
