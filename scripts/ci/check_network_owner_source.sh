#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
runtime="$repo_root/src/firmware/net/runtime.rs"
off_resources="$repo_root/src/firmware/net/runtime/off_resources.rs"
handoff="$repo_root/src/firmware/net/handoff.rs"
wifi="$repo_root/src/firmware/net/wifi.rs"
wifi_tree="$repo_root/src/firmware/net/wifi"
wifi_backend="$wifi_tree/backend_esp_radio.rs"
http="$repo_root/src/firmware/storage/upload/http.rs"
http_tree="$repo_root/src/firmware/storage/upload"
serial_dispatch="$repo_root/src/firmware/serial/command_dispatch.rs"
serial_family="$repo_root/src/firmware/serial/command_family.rs"
ble_runtime="$repo_root/src/firmware/ble/mod.rs"
serial_parser="$repo_root/src/firmware/serial/parser/basic/common.rs"
serial_task="$repo_root/src/firmware/serial.rs"
runtime_bootstrap="$repo_root/src/firmware/system.rs"
runtime_bootstrap_tasks="$repo_root/src/firmware/system/tasks.rs"
runtime_mod="$repo_root/src/firmware/mod.rs"
runtime_scheduling="$repo_root/src/firmware/scheduling.rs"
stack_recorder="$repo_root/src/firmware/observability/recorders/stack.rs"
phase1s_host="$repo_root/tools/hostctl/src/workflows/ble_phase1s/mod.rs"
phase1s_allocator_status="$repo_root/tools/hostctl/src/workflows/ble_phase1s/allocator_status.rs"
wifi_acceptance_start="$repo_root/tools/hostctl/src/workflows/wifi/acceptance/runtime_core/start.rs"
phase1d_host="$repo_root/tools/hostctl/src/workflows/ble_phase1d/mod.rs"
flash_artifacts="$repo_root/tools/hostctl/src/workflows/flash_capture/artifacts.rs"
firmware_build="$repo_root/scripts/build/build.sh"
phase1s_workflow="$repo_root/tools/hostctl/scenarios/ble-phase1s.sw.yaml"
serial_uart="$repo_root/src/firmware/serial_uart.rs"
firmware_tree="$repo_root/src/firmware"
uart_writer="$repo_root/src/firmware/touch/debug_log.rs"
uart_writer_stub="$repo_root/src/firmware/touch/debug_log_stub.rs"
uart_reservation="$repo_root/src/esp_println.rs"
psram_allocator="$repo_root/src/firmware/psram/mod.rs"
psram_provenance="$repo_root/src/firmware/psram/provenance.rs"
esp_alloc_root="$repo_root/vendor/esp-alloc-0.10.0-provenance"
esp_alloc_source="$esp_alloc_root/src/lib.rs"
sd_runtime="$repo_root/src/firmware/storage/sd_task/runtime_loop.rs"
sd_probe_io="$repo_root/packages/sdcard/src/probe/io.rs"
wifi_vendor="$repo_root/vendor/esp-radio-1.0.0-beta.0-bounded/src/wifi/mod.rs"
sd_task_tree="$repo_root/src/firmware/storage/sd_task"
sd_probe_mod="$repo_root/packages/sdcard/src/probe/mod.rs"
sd_probe_init="$repo_root/packages/sdcard/src/probe/init.rs"
sd_probe_wait="$repo_root/packages/sdcard/src/probe/wait.rs"
expected_embassy_net_digest="57d8329a2a17b4679b4c5d5b5d8b67a8d006b37ec4bc5ac31a7353ce0a90b3db"
expected_esp_hal_spi_dma_digest="16472f648beae62ae777deda985b49e3b1f6a8aacce6c7e69c59f808c3ae5652"
expected_esp_alloc_tree_digest="b24bdd8bc2f21a1de7786bbbe0ab33545ec3798c525be8cd76807c6c6f0bdc8e"

fail() {
  echo "network owner source check failed: $*" >&2
  exit 1
}

grep -Fq 'embassy-net = { version = "=0.9.1"' "$repo_root/Cargo.toml" \
  || fail "embassy-net is not exactly pinned to 0.9.1"
grep -Fq 'embassy-net = { path = "vendor/embassy-net-0.9.1-restartable" }' "$repo_root/Cargo.toml" \
  || fail "restartable embassy-net patch is not selected"
grep -Fq 'let rx_queue_size = if cfg!(debug_assertions) { 4 } else { 2 };' "$wifi_backend" \
  || fail "release/debug Wi-Fi RX queue calibration changed"
if grep -Fq '.with_ampdu_rx_enable(' "$wifi_backend"; then
  fail "rejected AMPDU RX disable experiment returned instead of the vendor baseline"
fi
if grep -Fq '.with_rx_ba_win(' "$wifi_backend"; then
  fail "rejected BA4 tuning returned instead of the accepted vendor receive-window baseline"
fi
grep -Fq 'static_rx_buf_num: 10,' "$wifi_vendor" \
  || fail "vendor static RX buffer default changed"
grep -Fq 'dynamic_rx_buf_num: 32,' "$wifi_vendor" \
  || fail "vendor dynamic RX buffer default changed"
grep -Fq 'ampdu_rx_enable: true,' "$wifi_vendor" \
  || fail "vendor AMPDU RX baseline changed"
grep -Fq 'esp-alloc = { version = "=0.10.0"' "$repo_root/Cargo.toml" \
  || fail "esp-alloc is not exactly pinned to 0.10.0"
grep -Fq 'esp-alloc = { path = "vendor/esp-alloc-0.10.0-provenance" }' "$repo_root/Cargo.toml" \
  || fail "exact allocation-provenance patch is not selected"
esp_alloc_manifest="$(cargo metadata --locked --format-version 1 2>/dev/null | python3 -c '
import json, sys
packages = [p for p in json.load(sys.stdin)["packages"] if p["name"] == "esp-alloc" and p["version"] == "0.10.0"]
if len(packages) != 1:
    raise SystemExit(2)
print(packages[0]["manifest_path"])
')" || fail "could not resolve exactly one esp-alloc 0.10.0 source"
[[ "$(cd "$(dirname "$esp_alloc_manifest")" && pwd -P)/$(basename "$esp_alloc_manifest")" == \
   "$(cd "$esp_alloc_root" && pwd -P)/Cargo.toml" ]] \
  || fail "Cargo resolves esp-alloc outside the reviewed vendor patch: $esp_alloc_manifest"
actual_esp_alloc_tree_digest="$({
  find "$esp_alloc_root" -type f ! -name MEDITAMER_PATCH.md -print0 \
    | LC_ALL=C sort -z \
    | while IFS= read -r -d '' source; do
        digest="$(shasum -a 256 "$source" | awk '{print $1}')"
        printf '%s  %s\n' "$digest" "${source#"$esp_alloc_root/"}"
      done
} | shasum -a 256 | awk '{print $1}')"
grep -Fq "\`$expected_esp_alloc_tree_digest\`" "$esp_alloc_root/MEDITAMER_PATCH.md" \
  || fail "esp-alloc patch manifest digest does not match the guarded tree"
[[ "$actual_esp_alloc_tree_digest" == "$expected_esp_alloc_tree_digest" ]] \
  || fail "esp-alloc provenance tree digest changed: $actual_esp_alloc_tree_digest"
grep -Fq 'let internal_free_before = heap.free_caps(MemoryCapability::Internal.into());' "$esp_alloc_source" \
  || fail "esp-alloc does not sample internal free before allocation under its mutex"
grep -Fq 'let post_internal_free = heap.free_caps(MemoryCapability::Internal.into());' "$esp_alloc_source" \
  || fail "esp-alloc does not sample internal free after allocation under its mutex"
grep -Fq 'internal_free_before,' "$esp_alloc_source" \
  || fail "esp-alloc hook omits the exact pre-allocation free count"
grep -Fq 'post_internal_free,' "$esp_alloc_source" \
  || fail "esp-alloc hook omits the exact post-allocation free count"
grep -Fq '_esp_alloc_dealloc_completed(self, ptr.addr().get(), layout.size());' "$esp_alloc_source" \
  || fail "esp-alloc does not publish exact deallocation completion under its mutex"
lock_block="$(awk 'BEGIN { found=0 } /^name = "embassy-net"$/ { found=1 } found { print } found && /^$/ { exit }' "$repo_root/Cargo.lock")"
grep -Fq 'version = "0.9.1"' <<<"$lock_block" || fail "Cargo.lock does not resolve embassy-net 0.9.1"

manifest="$(cargo metadata --locked --format-version 1 2>/dev/null | python3 -c '
import json, sys
packages = [p for p in json.load(sys.stdin)["packages"] if p["name"] == "embassy-net" and p["version"] == "0.9.1"]
if len(packages) != 1:
    raise SystemExit(2)
print(packages[0]["manifest_path"])
')" || fail "could not resolve exactly one embassy-net 0.9.1 source"
embassy_source="$(dirname "$manifest")/src/lib.rs"
[[ "$manifest" == "$repo_root/vendor/embassy-net-0.9.1-restartable/Cargo.toml" ]] \
  || fail "Cargo resolves embassy-net outside the guarded vendor patch: $manifest"
[[ -f "$embassy_source" ]] || fail "resolved embassy-net source is missing"
actual_digest="$(shasum -a 256 "$embassy_source" | awk '{print $1}')"
[[ "$actual_digest" == "$expected_embassy_net_digest" ]] \
  || fail "embassy-net source digest changed: $actual_digest"

runner_body="$(sed -n '/    pub async fn run(&mut self) -> ! {/,/^    }/p' "$embassy_source")"
grep -Fq 'poll_fn(|cx|' <<<"$runner_body" || fail "Runner::run is no longer a poll-only future"
grep -Fq 'Poll::<()>::Pending' <<<"$runner_body" || fail "Runner::run no longer remains cancellation-safe pending"
[[ "$(grep -Fc '.await;' <<<"$runner_body")" -eq 1 ]] \
  || fail "Runner::run no longer awaits exactly its cancellation-safe poll future"
grep -Fq 'pub fn reset(&mut self)' "$embassy_source" \
  || fail "StackResources restart reset is missing"
grep -Fq 'resources.reset();' "$embassy_source" \
  || fail "new network epochs do not destroy prior StackResources state"
grep -Fq 'self.inner.assume_init_drop();' "$embassy_source" \
  || fail "StackResources reset does not destroy prior inner state"

grep -Fq '#[embassy_executor::task]' "$runtime" || fail "network owner task is missing"
[[ "$(grep -Fc '#[embassy_executor::task]' "$runtime")" -eq 1 ]] \
  || fail "network runtime contains more than one detached task"
if grep -R -Fq '#[embassy_executor::task]' "$wifi" "$wifi_tree" "$http" "$http_tree"; then
  fail "Wi-Fi connection or HTTP service returned to detached task ownership"
fi
grep -Fq 'wifi_peripheral.reborrow()' "$runtime" || fail "Wi-Fi token is not reborrowed per epoch"
grep -Fq 'static STACK_RESOURCES: StaticCell<StackResources<' "$runtime" \
  || fail "StackResources is not statically retained"
grep -Fq 'let network = run_network_runner(&mut runner);' "$runtime" \
  || fail "Runner future is not locally borrowed"
grep -Fq 'let mut services = pin!(join3(connection, network, http));' "$runtime" \
  || fail "network epoch futures are not locally supervised"
grep -Fq 'wifi::request_control_quiescence();' "$runtime" \
  || fail "Wi-Fi connection future lacks cooperative quiescence admission"
grep -Fq 'wifi::control_quiesced()' "$runtime" \
  || fail "Wi-Fi connection future lacks a cooperative safe-point acknowledgement"
control_quiescent_body="$(sed -n '/^fn network_control_quiescent()/,/^}/p' "$runtime")"
grep -Fq 'control_quiescence_complete(wifi::control_quiesced())' <<<"$control_quiescent_body" \
  || fail "network control quiescence no longer requires the held Wi-Fi safe point"
if grep -Fq 'wifi_service_ready' <<<"$control_quiescent_body"; then
  fail "handoff quiescence incorrectly depends on serving telemetry after admission closes"
fi
grep -Fq 'static NEXT_HANDOFF_REQUEST_ID: AtomicU32' "$runtime" \
  || fail "handoff transport lacks monotonic request identity"
grep -Fq 'request_matches_ack(ticket.0, &ack)' "$runtime" \
  || fail "handoff acknowledgements are not matched by exact request identity"
grep -Fq 'if ack.request_id == 0 {' "$runtime" \
  || fail "unsolicited lifecycle state can occupy the bounded acknowledgement channel"
grep -Fq 'storage::upload::abort_sd_upload()' "$runtime" \
  || fail "network teardown lacks the mandatory SD FIFO barrier"
grep -Fq 'BLEPROBE ERR reason=phase1s_selected' "$serial_dispatch" \
  || fail "the historical resident BLE probe can still start during Phase 1S"
grep -Fq 'lto = "fat"' "$repo_root/Cargo.toml" \
  || fail "BLE release no longer retains the stack/image-recovering LTO policy"
grep -Fq 'codegen-units = 1' "$repo_root/Cargo.toml" \
  || fail "BLE release no longer uses the measured single codegen unit"
grep -Fq 'resources: psram::ExternalValue<BoardRuntimeResources>' "$runtime_bootstrap_tasks" \
  || fail "one-shot board startup resources returned to the permanent task pool"
grep -Fq 'external_value(run_board_runtime(spawner, resources))' "$runtime_bootstrap_tasks" \
  || fail "one-shot board startup future is no longer transient external storage"
grep -Fq 'observability::record_stack_headroom();' "$runtime_bootstrap_tasks" \
  || fail "transient external startup allocation no longer samples CPU0 stack headroom"
grep -Fq 'if installed && headroom_bytes <= 4 * 1024 {' "$stack_recorder" \
  || fail "deep stack telemetry can repeat unchanged low-water lines"
grep -Fq 'if installed && headroom <= 512 {' "$stack_recorder" \
  || fail "touch-core stack telemetry can repeat unchanged low-water lines"
grep -Fq 'const REQUIRED_CPU0_STACK_HEADROOM: u32 = 8 * 1_024;' "$phase1s_host" \
  || fail "Phase 1S host gate no longer enforces the CPU0 stack floor"
grep -Fq 'const REQUIRED_TOUCH_STACK_HEADROOM: u32 = 1_024;' "$phase1s_host" \
  || fail "Phase 1S host gate no longer enforces the touch-core stack floor"
grep -Fq '.command_wait_regex("STACKSTATUS", &status_re, Duration::from_secs(5))?' "$phase1s_host" \
  || fail "Phase 1S no longer uses the compact correlated stack-status command"
grep -Fq 'let (main, touch, uart_log_drops) = runtime.query_stack_status()?;' "$phase1s_allocator_status" \
  || fail "Phase 1S no longer parses both stack floors from one matched response"
grep -Fq 'self.uart_log_drops_baseline = Some(uart_log_drops);' "$phase1s_host" \
  || fail "Phase 1S no longer captures a pre-evidence UART drop baseline"
grep -Fq 'runtime.uart_log_drops_final = Some(uart_log_drops);' "$phase1s_allocator_status" \
  || fail "Phase 1S no longer captures a final UART drop count"
grep -Fq 'Ok(during_gate) => violations' "$phase1s_host" \
  || fail "Phase 1S can pass after losing UART diagnostics during the evidence window"
grep -Fq 'validate_uart_drop_counter(uart_log_drops_baseline, uart_log_drops_final).ok();' "$phase1s_host" \
  || fail "Phase 1S report no longer records a checked UART drop delta"
grep -Fq 'gate_passed,' "$phase1s_host" \
  || fail "retained Phase 1S JSON no longer carries an explicit final verdict"
grep -Fq 'violations,' "$phase1s_host" \
  || fail "retained Phase 1S JSON no longer explains a failed final verdict"
grep -Fq 'if free_drift > MAX_POST_WARMUP_DRIFT || block_drift > MAX_POST_WARMUP_DRIFT {' "$phase1s_host" \
  || fail "Phase 1S drift can bypass the persisted final verdict"
collect_line="$(grep -n -m1 'call: "collect_stack_metrics"' "$phase1s_workflow" | cut -d: -f1)"
report_line="$(grep -n -m1 'call: "write_report"' "$phase1s_workflow" | cut -d: -f1)"
assert_line="$(grep -n -m1 'call: "assert_stack_floors"' "$phase1s_workflow" | cut -d: -f1)"
[[ "$collect_line" -lt "$report_line" && "$report_line" -lt "$assert_line" ]] \
  || fail "Phase 1S must collect stack floors, persist evidence, then enforce the floors"
grep -Fq '"STACK_STATUS cpu0={} touch={} tx_drop={}\r\n"' "$serial_dispatch" \
  || fail "firmware stack status is no longer one compact observable response"
[[ "$(grep -Fc 'SerialCommand::StackStatus' "$serial_task")" -ge 2 ]] \
  || fail "stack status returned to the wide boxed dispatcher"
[[ "$(grep -Fc 'SerialCommand::AllocatorStatus' "$serial_task")" -ge 2 ]] \
  || fail "allocator status returned to the wide boxed dispatcher"
grep -Fq 'cmd == b"PSRAM" || cmd == b"ALLOCSTATUS" || cmd == b"ALLOCATOR" || cmd == b"HEAP"' "$serial_parser" \
  || fail "Phase 1S allocator-status command is no longer accepted by the firmware parser"
grep -Fq 'return Some(SerialCommand::AllocatorStatus);' "$repo_root/src/firmware/serial/parser/mod.rs" \
  || fail "allocator-status aliases no longer route to AllocatorStatus"
grep -Fq 'internal_probe_performed=false' "$repo_root/src/firmware/serial/io.rs" \
  || fail "serving allocator status no longer identifies its allocation-free snapshot"
if grep -Fq 'probe_internal_block_above_reserve(' "$repo_root/src/firmware/serial/io.rs"; then
  fail "serving allocator status can again race a deliberate probe allocation with radio RX"
fi
grep -Fq 'probe_internal_block_above_reserve(INTERNAL_RESERVE_BYTES)' "$runtime" \
  || fail "OffConfirmed no longer performs the binding source-quiescent contiguous probe"
epoch_body="$(sed -n '/^async fn run_network_epoch(/,/^async fn await_restoration/p' "$runtime")"
if grep -Fq 'resource_snapshot(true)' <<<"$epoch_body"; then
  fail "binding off-state probe runs before the network epoch future has returned"
fi
grep -Fq 'let resources = settled_off_resource_snapshot().await;' "$runtime" \
  || fail "network owner no longer waits for coherent post-epoch allocator settlement"
grep -Fq 'mod off_resources;' "$runtime" \
  || fail "post-epoch allocator settlement module is no longer reachable"
grep -Fq 'const OFF_RESOURCE_SETTLEMENT_SAMPLES: u8 = 2;' "$off_resources" \
  || fail "post-epoch allocator settlement no longer requires consecutive samples"
grep -Fq 'const OFF_RESOURCE_POST_PROBE_CONFIRMATIONS: u8 = 2;' "$off_resources" \
  || fail "post-epoch allocator settlement no longer confirms the complete probe signature"
grep -Fq 'const OFF_RESOURCE_RTOS_BARRIER_US: u32 = 1_000;' "$off_resources" \
  || fail "post-epoch allocator settlement lost its preemptive scheduler barrier"
grep -Fq 'esp_radio_rtos_driver::usleep(OFF_RESOURCE_RTOS_BARRIER_US);' "$off_resources" \
  || fail "post-epoch allocator evidence can again run before deferred RTOS task deletion"
grep -Fq 'same_probe_signature(&probed, &confirmed)' "$off_resources" \
  || fail "post-epoch allocator settlement can accept a stale pre-reclamation probe layout"
grep -Fq 'return reject_unsettled(probed);' "$off_resources" \
  || fail "post-epoch allocator settlement can publish an unconfirmed signature at timeout"
off_wait_body="$(sed -n '/^async fn wait_while_off(/,/^async fn service_restore_wait/p' "$runtime")"
if grep -Fq 'resource_snapshot(true)' <<<"$off_wait_body"; then
  fail "OffConfirmed STATUS can re-open the binding probe race"
fi
grep -Fq '"handoff_outcome": "known_serving_failed"' "$phase1s_host" \
  || fail "correlated Rejected/Serving ownership is no longer retained as known"
grep -Fq 'when: ".handoff_outcome == \"known_serving_failed\""' "$phase1s_workflow" \
  || fail "workflow no longer records a known serving rejection without duplicate restore"
grep -Fq '!minimum.probe_performed,' "$phase1s_allocator_status" \
  || fail "Phase 1S no longer requires the allocation-free serving snapshot"
grep -Fq 'validate_serving_allocator_snapshot(' "$wifi_acceptance_start" \
  || fail "Wi-Fi acceptance no longer validates the allocation-free serving snapshot"
grep -Fq 'command_family::pre_box_command_class(&cmd)' "$serial_task" \
  || fail "firmware stream state can again inflate every ordinary serial command"
grep -Fq 'InternalValue::try_new_bounded::<' "$serial_task" \
  || fail "firmware commands no longer retain internal cache-safe future ownership"
if grep -Fq 'Box::pin(command_dispatch::handle_serial_command' "$serial_task"; then
  fail "ordinary serial commands can again fall back to external PSRAM"
fi
grep -Fq 'InternalValue::try_new_bounded::<1_500, _>' "$serial_task" \
  || fail "ordinary serial-command future lost its internal target-size ceiling"
[[ "$(grep -Fc '| SerialCommand::NetRecover' "$serial_task")" -ge 2 ]] \
  || fail "network recovery can again depend on heap allocation"
grep -Fq 'unreachable!("network recovery uses the allocation-free path")' "$serial_dispatch" \
  || fail "network recovery can again enter the ordinary dispatcher"
grep -Fq 'unreachable!("firmware command reached ordinary boxed dispatcher")' "$serial_dispatch" \
  || fail "firmware commands can again reach the ordinary boxed dispatcher"
grep -Fq 'fail_firmware_dispatch_allocation(' "$serial_task" \
  || fail "firmware allocation failure can again strand the prepared hardware lease"
grep -Fq 'firmware_update::end_transport();' "$serial_family" \
  || fail "firmware allocation failure lost its allocation-free transport cleanup"
grep -Fq 'command_dispatch::release_firmware_update_hardware(state).await;' "$serial_family" \
  || fail "firmware allocation failure lost its hardware lease cleanup"
grep -Fq 'const fn pre_box_command_class' "$serial_family" \
  || fail "command family routing lost its production compile-time contract"
grep -Fq 'pre_box_command_class(&SerialCommand::FirmwareFactoryBoot' "$serial_family" \
  || fail "firmware factory-boot request is no longer compile-time guarded before allocation"
grep -Fq 'PreBoxCommandClass::Metrics' "$serial_task" \
  || fail "multi-line metrics state can again inflate every ordinary serial command"
grep -Fq 'PreBoxCommandClass::MetricsNet' "$serial_task" \
  || fail "network metrics state can again inflate every ordinary serial command"
grep -Fq 'unreachable!("metrics command reached ordinary boxed dispatcher")' "$serial_dispatch" \
  || fail "metrics commands can again reach the ordinary boxed dispatcher"
grep -Fq 'esp-println-upstream = { package = "esp-println", version = "0.17.0", default-features = false, features = ["esp32", "log-04", "uart"] }' "$repo_root/Cargo.toml" \
  || fail "upstream ROM writer identity/features changed"
if cargo tree --locked --features ble-foundation -e features -i esp-println 2>/dev/null \
  | grep -Fq 'esp-println feature "critical-section"'; then
  fail "product UART0 writer again disables interrupts through esp-println critical-section"
fi
grep -Fq 'metadata_value(&metadata, "image_source") != Some("build")' "$phase1d_host" \
  || fail "Phase 1S can accept an explicit image relabeled as a workflow build"
grep -Fq 'metadata_value(&metadata, "requested_features") != Some("ble-foundation")' "$phase1d_host" \
  || fail "Phase 1S artifact validation no longer pins the requested BLE feature"
grep -Fq 'metadata_value(&metadata, "no_default_features") != Some("false")' "$phase1d_host" \
  || fail "Phase 1S artifact validation can omit default product features"
grep -Fq 'if built_in_workflow && profile == "ble-release"' "$flash_artifacts" \
  || fail "flash capture no longer records canonical BLE build provenance"
grep -Fq 'PROFILE_ARGS=(--features ble-foundation)' "$firmware_build" \
  || fail "ble-release no longer builds the canonical default-plus-BLE feature set"
grep -Fq 'ble-release evidence requires the default product features' "$firmware_build" \
  || fail "ble-release can silently omit default product features"
grep -Fq 'static RESPONSE_PENDING_CORE: AtomicU8' "$uart_reservation" \
  || fail "UART0 responses lack priority reservation over diagnostics"
grep -Fq 'DROPPED_WRITES.fetch_add(1' "$uart_reservation" \
  || fail "lossy UART0 diagnostics no longer account for contention"
grep -Fq 'if pending == current || owner == current {' "$uart_reservation" \
  || fail "same-core UART0 reentry can deadlock the active writer"
grep -Fq 'spin_loop();' "$uart_reservation" \
  || fail "cross-core UART0 events no longer wait with interrupts enabled"
grep -Fq 'let _pending = PendingResponse;' "$uart_reservation" \
  || fail "cancelled UART0 response can leave diagnostic admission blocked"
if grep -R -n 'esp_println_upstream' "$firmware_tree" >/dev/null; then
  fail "first-party firmware bypasses the UART0 priority reservation"
fi
if grep -Fq 'esp-println' "$repo_root/packages/sdcard/Cargo.toml"; then
  fail "SD package can bypass the firmware UART0 reservation"
fi
grep -Fq 'extern crate self as esp_println;' "$repo_root/packages/sdcard/src/lib.rs" \
  || fail "SD package is no longer explicitly UART-neutral"
if rg -n 'esp-println' "$repo_root/packages" --glob Cargo.toml >/dev/null; then
  fail "a workspace package can bypass the firmware UART0 reservation"
fi
if grep -R -n -E 'uart\.(write|write_async|flush_async)\(' "$firmware_tree" >/dev/null; then
  fail "serial responses can race esp-println through HAL async TX"
fi
grep -Fq 'crate::write_uart_response(bytes).await;' "$uart_writer" \
  || fail "serial responses bypass the interrupt-enabled priority reservation"
grep -Fq 'crate::write_uart_response(bytes).await;' "$uart_writer_stub" \
  || fail "slim serial responses bypass the interrupt-enabled priority reservation"
grep -Fq '(Self::Interactive | Self::Upload, TaskClass::Serial) => 3,' "$runtime_scheduling" \
  || fail "serial control-plane task is not preemptive under interactive/upload load"
grep -Fq '| TaskClass::Display,' "$runtime_scheduling" \
  || fail "upload-mode display/app-event work can again be starved by the network epoch"
grep -Fq 'profile.priority(TaskClass::Display),' "$runtime_scheduling" \
  || fail "scheduler tests no longer bind upload-mode display/network fairness"
grep -Fq 'const INTERNAL_HEAP_DRAM2_BYTES: usize = 64 * 1024 + 3_200;' "$psram_allocator" \
  || fail "Phase 1S internal heap no longer retains the RX replacement budget"
grep -Fq 'features = ["alloc-hooks", "compat", "esp32", "global-allocator"]' "$repo_root/Cargo.toml" \
  || fail "allocator-wide internal low-water hook is no longer enabled"
grep -Fq 'unsafe extern "Rust" fn _esp_alloc_alloc(' "$psram_provenance" \
  || fail "successful allocations no longer update the internal low-water"
grep -Fq 'static INTERNAL_LOW_WATER: AtomicU32' "$psram_provenance" \
  || fail "internal low-water and provenance are not one native atomic record"
grep -Fq 'internal_free_before.saturating_sub(post_internal_free)' "$psram_provenance" \
  || fail "allocation hook does not record the exact under-lock internal charge"
hook_body="$(sed -n '/unsafe extern "Rust" fn _esp_alloc_alloc(/,/^}/p' "$psram_provenance")"
if grep -Eq 'free_caps|println|write!|alloc_caps' <<<"$hook_body"; then
  fail "allocation hook performs allocator, formatting, or UART work"
fi
grep -Fq 'min_internal_alloc_charge_bytes={}' "$repo_root/src/firmware/serial/io.rs" \
  || fail "allocator status omits exact low-water allocation charge"
grep -Fq 'min_internal_alloc_post_free_bytes={}' "$repo_root/src/firmware/serial/io.rs" \
  || fail "allocator status omits exact low-water post-allocation free bytes"
grep -Fq 'min_internal_alloc_internal_required={}' "$repo_root/src/firmware/serial/io.rs" \
  || fail "allocator status omits exact low-water requested capability class"
grep -Fq 'min_internal_alloc_charge_overflow={}' "$repo_root/src/firmware/serial/io.rs" \
  || fail "allocator status omits low-water charge overflow disposition"
grep -Fq 'min_internal_alloc_correlation_stable={}' "$repo_root/src/firmware/serial/io.rs" \
  || fail "allocator status omits low-water correlation stability"
grep -Fq 'min_internal_alloc_wifi_rx_matched={}' "$repo_root/src/firmware/serial/io.rs" \
  || fail "allocator status omits station-RX low-water correlation"
grep -Fq 'min_internal_alloc_released={}' "$repo_root/src/firmware/serial/io.rs" \
  || fail "allocator status omits exact low-water allocation release"
grep -Fq 'const ALLOCATOR_STATUS_CAPACITY: usize = 960;' "$repo_root/src/firmware/serial/io.rs" \
  || fail "allocator status response capacity changed without max-width review"
grep -Fq 'const ALLOCATOR_STATUS_TARGET_MAX_BYTES: usize = 916;' "$repo_root/src/firmware/serial/io.rs" \
  || fail "allocator status target max-width proof changed"
python3 - "$repo_root/src/firmware/serial/io.rs" <<'PY' \
  || fail "allocator status production format exceeds or disagrees with its target max-width proof"
import re
import sys

source = open(sys.argv[1], encoding="utf-8").read()
match = re.search(r'"(PSRAM feature_enabled=.*?\\r\\n)",\n', source, re.S)
if not match:
    raise SystemExit(2)
wire = bytes(match.group(1), "utf-8").decode("unicode_escape")
if wire.count("{}") != 25 or wire.count("{:?}") != 1:
    raise SystemExit(3)
# ESP32 usize/u32 are at most ten decimal bytes. The production line has 18
# numeric values, seven bool values (longest `false`), and one AllocatorState
# debug value (longest `NotInitialized`). Placeholder bytes are removed.
rendered = len(wire) - 25 * 2 - 4 + 18 * 10 + 7 * 5 + len("NotInitialized")
if rendered != 916 or rendered >= 960:
    raise SystemExit(4)
PY
rx_sta_body="$(sed -n '/unsafe extern "C" fn recv_cb_sta(/,/^}/p' "$wifi_vendor")"
rx_correlation_line="$(grep -n -m1 '_meditamer_match_internal_low_water_wifi_rx' <<<"$rx_sta_body" | cut -d: -f1)"
rx_callback_guard_line="$(grep -n -m1 'let callback = WifiCallbackGuard::enter();' <<<"$rx_sta_body" | cut -d: -f1)"
rx_owner_cas_line="$(grep -n -m1 'WIFI_RX_STA_CALLBACK_OWNED' <<<"$rx_sta_body" | cut -d: -f1)"
[[ -n "$rx_correlation_line" && -n "$rx_callback_guard_line" && -n "$rx_owner_cas_line" \
  && "$rx_callback_guard_line" -lt "$rx_correlation_line" \
  && "$rx_correlation_line" -lt "$rx_owner_cas_line" ]] \
  || fail "station-RX correlation escaped the callback fence or retained-owner boundary"
grep -Fq 'range_in_winning_allocation(eb, 1, ptr, size)' "$psram_provenance" \
  || fail "station-RX correlation no longer requires the opaque handle in the winning allocation"
grep -Fq 'range_in_winning_allocation(buffer, len, ptr, size)' "$psram_provenance" \
  || fail "station-RX correlation no longer requires the full payload in the winning allocation"
grep -Fq 'minimum_alloc_correlation_stable' "$wifi_acceptance_start" \
  || fail "Wi-Fi acceptance can credit ambiguous low-water correlation"
grep -Fq 'min_internal_alloc_correlation_stable' "$phase1s_allocator_status" \
  || fail "Phase 1S can credit ambiguous low-water correlation"
grep -Fq 'minimum_serving_internal_alloc_charge_bytes' "$phase1s_host" \
  || fail "Phase 1S report does not retain low-water allocation provenance"
grep -Fq 'minimum_alloc_post_free != status.minimum_internal' "$wifi_acceptance_start" \
  || fail "Wi-Fi acceptance does not bind allocation provenance to the reported minimum"
grep -Fq 'minimum_alloc_charge_overflow' "$wifi_acceptance_start" \
  || fail "Wi-Fi acceptance can pass a saturated low-water charge"
grep -Fq 'charge_overflow' "$phase1s_allocator_status" \
  || fail "Phase 1S can pass a saturated low-water charge"
grep -Fq 'const INTERNAL_PROBE_ALIGNMENT: usize = 2 * core::mem::size_of::<usize>();' "$repo_root/src/firmware/psram/status.rs" \
  || fail "reserve probe can again cross the floor through allocator size rounding"
grep -Fq 'fat_engine: crate::firmware::psram::ExternalValue<FatEngine>' "$sd_runtime" \
  || fail "persistent FAT interpreter state returned to internal heap"
grep -Fq 'ExternalValue::try_new(fat_engine)' "$sd_runtime" \
  || fail "FAT interpreter is not explicitly allocated in external PSRAM"
grep -Fq 'self.read_data_sector_512_into_cached(lba, high_capacity)' "$sd_probe_io" \
  || fail "FAT read path can again pass a PSRAM caller buffer to SPI DMA"
cache_invalidate_line="$(grep -n -m1 'self.cached_sector_lba = None;' "$sd_probe_io" | cut -d: -f1)"
cached_read_line="$(grep -n -m1 'self.read_data_sector_512_into_cached(lba, high_capacity)' "$sd_probe_io" | cut -d: -f1)"
[[ "$cache_invalidate_line" -lt "$cached_read_line" ]] \
  || fail "SD read miss no longer invalidates the old cache tag before DMA"
grep -Fq '.transfer_in_place_to_completion(&mut self.cached_sector[..SD_SECTOR_SIZE])' "$sd_probe_io" \
  || fail "SD read DMA no longer targets the probe-owned internal bounce sector"
grep -Fq 'static WIFI_RX_STA_CALLBACK_OWNED: AtomicBool = AtomicBool::new(false);' "$wifi_vendor" \
  || fail "station RX callback retained ownership is no longer single-flight bounded"
grep -Fq '.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)' "$wifi_vendor" \
  || fail "station RX callback can again retain concurrent not-yet-queued packets"
grep -Fq 'WIFI_RX_STA_CALLBACK_OWNED.store(false, Ordering::Release);' "$wifi_vendor" \
  || fail "station RX callback ownership token is not released"
grep -Fq '&& !callbacks.rx_sta_callback_owned' "$wifi_vendor" \
  || fail "Wi-Fi queue reclamation no longer verifies station RX callback ownership"
grep -Fq '&& !callbacks.rx_sta_callback_owned' "$runtime" \
  || fail "network owner no longer fences station RX callback ownership"
if grep -R -Fq 'with_timeout(Duration::from_secs(2), sd_probe.init())' "$sd_task_tree"; then
  fail "SD initialization again permits coarse cancellation across DMA transfers"
fi
[[ "$(grep -R -F 'sd_probe.init().await' "$sd_task_tree" | wc -l | tr -d ' ')" -eq 1 ]] \
  || fail "firmware SD initialization escaped the single package-owned default deadline"
[[ "$(grep -R -F 'init_until(init_deadline).await' "$sd_task_tree" | wc -l | tr -d ' ')" -eq 3 ]] \
  || fail "SD request initialization no longer shares one cooperative deadline"
[[ "$(grep -R -F 'probe_until(init_deadline).await' "$sd_task_tree" | wc -l | tr -d ' ')" -eq 1 ]] \
  || fail "SD probe command no longer shares the request initialization deadline"
grep -Fq 'if !wait_before_deadline(SD_RETRY_DELAY_MS, init_deadline).await {' "$sd_task_tree/dispatch.rs" \
  || fail "SD retries can outlive the shared request initialization deadline"
if grep -R -Eq 'request_sd_power_until|retry_after_power_cycle' "$sd_task_tree"; then
  fail "deadline-bounded SD init again permits a late queued power mutation"
fi
grep -Fq 'const SD_INIT_DEADLINE_MS: u64 = 2_000;' "$sd_probe_mod" \
  || fail "SD default initialization deadline changed without source-gate review"
grep -Fq 'self.spi.transfer_in_place_to_completion(words).await?;' "$sd_probe_mod" \
  || fail "SD cooperative deadline no longer waits for an atomic DMA transfer"
[[ "$(grep -Fc 'self.check_init_deadline()' "$sd_probe_mod")" -eq 2 ]] \
  || fail "SD cooperative deadline is not checked before and after each init transfer"
grep -Fq 'self.init_deadline = None;' "$sd_probe_init" \
  || fail "SD initialization can leave a stale cooperative deadline installed"
[[ "$(grep -Fc 'self.transfer_init_bounded' "$sd_probe_io")" -eq 6 ]] \
  || fail "an init-reachable SD transfer no longer uses the cooperative deadline wrapper"
if grep -Fq 'self.spi.transfer_in_place_to_completion' "$sd_probe_init" "$sd_probe_io"; then
  fail "SD initialization can bypass the cooperative transfer deadline"
fi
wait_data_token_body="$(sed -n '/    pub(super) async fn wait_data_token/,/^    }/p' "$sd_probe_wait")"
grep -Fq 'self.transfer_byte(0xFF).await?' <<<"$wait_data_token_body" \
  || fail "SD initialization token polling bypasses the cooperative transfer wrapper"
if grep -Fq 'transfer_in_place_to_completion' <<<"$wait_data_token_body"; then
  fail "SD initialization token polling directly awaits DMA"
fi
[[ "$(grep -Fhc 'esp-hal = { version = "=1.1.1"' "$repo_root/Cargo.toml" "$repo_root/packages/sdcard/Cargo.toml" | awk '{ total += $1 } END { print total + 0 }')" -eq 2 ]] \
  || fail "esp-hal is not exactly pinned for SD DMA cancellation semantics"
esp_hal_lock="$(awk 'BEGIN { found=0 } /^name = "esp-hal"$/ { found=1 } found { print } found && /^$/ { exit }' "$repo_root/Cargo.lock")"
grep -Fq 'version = "1.1.1"' <<<"$esp_hal_lock" \
  || fail "Cargo.lock no longer resolves esp-hal 1.1.1"
grep -Fq 'checksum = "2bfcf2a0842903717f4663f6a08512c32b0f6b2d7fb7db3c8a6895d2e6d49f72"' <<<"$esp_hal_lock" \
  || fail "Cargo.lock esp-hal checksum changed"
esp_hal_manifest="$(cargo metadata --locked --format-version 1 2>/dev/null | python3 -c '
import json, sys
packages = [p for p in json.load(sys.stdin)["packages"] if p["name"] == "esp-hal" and p["version"] == "1.1.1"]
if len(packages) != 1:
    raise SystemExit(2)
print(packages[0]["manifest_path"])
')" || fail "could not resolve exactly one esp-hal 1.1.1 source"
esp_hal_spi_dma="$(dirname "$esp_hal_manifest")/src/spi/master/dma.rs"
[[ -f "$esp_hal_spi_dma" ]] || fail "resolved esp-hal SPI DMA source is missing"
actual_esp_hal_spi_dma_digest="$(shasum -a 256 "$esp_hal_spi_dma" | awk '{print $1}')"
[[ "$actual_esp_hal_spi_dma_digest" == "$expected_esp_hal_spi_dma_digest" ]] \
  || fail "esp-hal SPI DMA cancellation source changed: $actual_esp_hal_spi_dma_digest"
grep -Fq 'pub(crate) const RX_FIFO_FULL_THRESHOLD: u16 = 64;' "$serial_uart" \
  || fail "UART RX FIFO threshold no longer preserves half-FIFO scheduling headroom"
grep -Fq 'super::serial_uart::config(UART_BAUD)' "$runtime_bootstrap" \
  || fail "bootstrap no longer uses the guarded UART configuration"
grep -Fq 'line_reader.discard_until_line_end();' "$serial_task" \
  || fail "UART RX errors no longer retire a damaged text command"
grep -Fq 'SERIAL_RX status=error' "$serial_task" \
  || fail "UART RX errors are no longer observable"
grep -Fq 'super::super::reset_pending_update_or_halt();' "$runtime_bootstrap_tasks" \
  || fail "external startup allocation failure no longer preserves pending-update rollback"
grep -Fq 'crate::firmware::reset_pending_update_or_halt();' "$sd_runtime" \
  || fail "external FAT allocation failure no longer preserves pending-update rollback"
pending_reset_body="$(sed -n '/^pub(crate) fn reset_pending_update_or_halt()/,/^}/p' "$runtime_mod")"
grep -Fq 'OtaImageState::PendingVerify' <<<"$pending_reset_body" \
  || fail "startup allocation failure no longer distinguishes pending verification"
grep -Fq 'esp_hal::system::software_reset();' <<<"$pending_reset_body" \
  || fail "pending startup allocation failure no longer resets into rollback"
grep -Fq 'core::hint::spin_loop();' <<<"$pending_reset_body" \
  || fail "confirmed-image startup allocation failure no longer fails closed"
if grep -R -n -E "Stack[[:space:]]*<['\"]static>" \
  "$runtime" "$wifi" "$wifi_tree" "$http" "$http_tree" >/dev/null; then
  fail "a static Stack escaped the supervised network epoch"
fi

runner_drop="$(grep -n -m1 'drop(runner);' "$runtime" | cut -d: -f1)"
stop_call="$(grep -n -m1 'with_timeout(WIFI_STOP_TIMEOUT' "$runtime" | cut -d: -f1)"
controller_drop="$(grep -n -m1 'drop(controller);' "$runtime" | cut -d: -f1)"
[[ "$runner_drop" -lt "$stop_call" && "$stop_call" -lt "$controller_drop" ]] \
  || fail "runner/stop/controller teardown order changed"
grep -Fq 'wifi::wifi_shutdown_source(&mut controller)' "$runtime" \
  || fail "controller callback source is not explicitly disabled"
grep -Fq 'wifi::wifi_finalize_shutdown(&mut controller)' "$runtime" \
  || fail "controller queue reclamation is not explicitly finalized"
grep -Fq 'resources.reset();' "$runtime" \
  || fail "network stack resources are not explicitly destroyed after each epoch"
grep -Fq 'parse_ble_phase1s_start_command' "$serial_parser" \
  || fail "Phase 1S BLE start is not boot/epoch parsed"
grep -Fq 'SerialCommand::BlePhase1sStart { .. }' "$serial_task" \
  || fail "Phase 1S BLE evidence returned to the wide heap-backed dispatcher"
grep -Fq 'exclusive_lease_matches(boot_generation, epoch)' "$ble_runtime" \
  || fail "Phase 1S BLE admission is not bound to the exact off lease"
grep -Fq 'phase1s_exclusive_ownership_confirmed(' "$ble_runtime" \
  || fail "Phase 1S BLE evidence bypasses the shared exclusive-ownership predicate"
grep -Fq '&& !http_listener' "$handoff" \
  || fail "Phase 1S exclusive-ownership predicate lost listener absence"
if grep -A8 -F 'let exclusive_ok =' "$ble_runtime" | grep -Fq 'net.radio_quiesced'; then
  fail "Phase 1S BLE ownership regressed to connection-policy radio telemetry"
fi
grep -Fq 'if update_reserved() {' "$ble_runtime" \
  || fail "Phase 1S BLE admission no longer checks update priority"
grep -Fq 'const PHASE1S_CYCLES: u8 = 1;' "$ble_runtime" \
  || fail "Phase 1S BLE request no longer initializes exactly one window"
if grep -Eq '#\[gatt_(server|service)|\.advertise\(' "$ble_runtime"; then
  fail "Phase 1S A5 unexpectedly initializes GATT or advertising"
fi
grep -Fq 'match crate::firmware::ble::phase1s_ownership()' "$runtime" \
  || fail "Wi-Fi restoration no longer uses one atomic BLE ownership disposition"
grep -Fq 'ProbeRequestError::Consumed' "$serial_dispatch" \
  || fail "an off lease can initialize BLE more than once"
grep -Fq 'stack.command(ReadBdAddr::new())' "$ble_runtime" \
  || fail "Phase 1S can close before the Trouble host completes initialization"
grep -Fq 'PHASE1S_RX_QUEUE_OVERFLOW.store' "$ble_runtime" \
  || fail "Phase 1S does not preserve pre-deinit transport fault counters"
grep -Eq '(transport|active)\.rx_queue_overflow != 0' "$ble_runtime" \
  || fail "Phase 1S can pass after a bounded transport overflow"
grep -Fq 'quiescent.rejected != 0' "$ble_runtime" \
  || fail "Phase 1S can pass after callback rejection during shutdown"
grep -Fq 'call: "prepare_off_window"' "$phase1s_workflow" \
  || fail "Phase 1S workflow has no explicit Wi-Fi-off boundary"
grep -Fq 'call: "run_ble_window"' "$phase1s_workflow" \
  || fail "Phase 1S workflow does not execute a BLE window per epoch"
grep -Fq 'call: "restore_and_complete_cycle"' "$phase1s_workflow" \
  || fail "Phase 1S workflow does not restore Wi-Fi after known BLE close"
grep -Fq 'when: ".ble_outcome == \"known_failed\""' "$phase1s_workflow" \
  || fail "known BLE failure restoration is hidden outside workflow orchestration"
grep -Fq 'call: "write_failure_report"' "$phase1s_workflow" \
  || fail "Phase 1S does not retain structured first-init failure evidence"
grep -Fq 'when: ".handoff_outcome == \"known_off_failed\""' "$phase1s_workflow" \
  || fail "off-settlement failure no longer uses the YAML-owned rollback path"
grep -Fq 'when: ".restore_outcome == \"pass\""' "$phase1s_workflow" \
  || fail "Wi-Fi restoration failures can bypass structured evidence"
grep -Fq 'when: ".metrics_outcome == \"pass\""' "$phase1s_workflow" \
  || fail "final metrics failures can bypass structured evidence"
grep -Fq 'call: "fail_final_metrics"' "$phase1s_workflow" \
  || fail "final metrics failure does not stop after evidence persistence"
if grep -Fq 'BLEPROBE START' "$phase1s_host"; then
  fail "Phase 1S host can invoke the historical resident BLE probe"
fi

echo "network owner source check passed"
