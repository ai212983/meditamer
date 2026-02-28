# Cold Boot Validation (Deferred)

Status: deferred / skipped for now

- [ ] Re-run true cold-boot matrix after confirming full board power-off behavior.

Why deferred:
- Inkplate 4 TEMPERA can remain powered from internal battery when USB is disconnected.
- USB unplug/replug alone is not a valid cold boot and produced unreliable serial capture results.

Unblock conditions:
- Confirm repeatable true power-off method (board OFF state with power LED off, or battery physically disconnected).
- Re-run manual cold-boot matrix using that method.

Acceptance criteria:
- 5/5 manual true cold-boot cycles pass with required boot markers.
- No binary/noise-only capture logs during passing cycles.

References:
- docs.soldered.com Inkplate 4 TEMPERA quick start / FAQ / battery pages (power behavior)
- scripts/device/cold_boot_matrix.sh
