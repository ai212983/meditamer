#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
patch_root="$repo_root/vendor/esp-radio-1.0.0-beta.1-bounded"
driver_root="$repo_root/vendor/esp-radio-rtos-driver-0.4.2-retained"
ble_mod="$patch_root/src/ble/mod.rs"
btdm="$patch_root/src/ble/btdm.rs"
esp32_ble_adapter="$patch_root/src/ble/os_adapter_esp32.rs"
controller="$patch_root/src/ble/controller/mod.rs"
tx_cancellation="$patch_root/src/ble/tx_cancellation.rs"
compat_queue="$patch_root/src/compat/queue.rs"
queue_lifecycle="$patch_root/src/compat/queue_lifecycle.rs"
wifi_adapter="$patch_root/src/wifi/os_adapter/mod.rs"
wifi_controller="$patch_root/src/wifi/mod.rs"
root_manifest="$repo_root/targets/meditamer-inkplate/Cargo.toml"
serial_task="$repo_root/products/meditamer/src/firmware/serial.rs"
byte_dispatch="$repo_root/products/meditamer/src/firmware/serial/byte_dispatch.rs"
command_dispatch="$repo_root/products/meditamer/src/firmware/serial/command_dispatch.rs"
firmware_ble_runtime="$repo_root/products/meditamer/src/firmware/ble/runtime.rs"
psram_provenance="$repo_root/products/meditamer/src/firmware/psram/provenance.rs"
psram_init="$repo_root/products/meditamer/src/firmware/psram/init.rs"
shared_ble_root="$repo_root/platform/connectivity/ble"
waveshare_manifest="$repo_root/targets/medinote-waveshare/Cargo.toml"
product_manifest="$repo_root/products/meditamer/Cargo.toml"
expected_tree_digest="951842dea4666da29a2e0ec137d8585c24827d5bc75518665fee8b9b1499858a"
expected_driver_digest="98ff7d137198ccf6d3147fc965ba78c136fb989d5a9d34507ccc3c6bcb877088"
expected_esp_rtos_scheduler_digest="8535628f76bb6fab1bfbd5e2d4fe2cea8460ca44dc170b438388d981b5fe6100"
expected_esp_rtos_task_digest="4b98f87e6aa4323b1394c1bd3d89a87bd1257773a18b8402e5b24349d827ff18"
expected_esp_rtos_timer_digest="e88aeeb862c8be0ce299f54648c843236852b2db51ebbcd4c727c4af6089233b"
expected_esp_rtos_xtensa_digest="d010862b5c7403d376a6903c4a0c60409ee744add04165379c387ca0f15fc725"
expected_esp_rtos_context_probe_digest="e45e1d32af71b3416c131ee7c747b2975d724804900451caed1442ff78335400"
expected_esp_rtos_lib_digest="88eb87dcf4ddbc53153792dbf0cfbc69f74d55e578c8694a4e5e3f3a013a5dff"
expected_esp_rtos_tree_digest="0272570ae7d42083c96d58bea6c46de092266f6adcd31604b54bf060a72f6db9"
esp_rtos_probe_root="$repo_root/vendor/esp-rtos-0.4.0-context-probe"
esp_rtos_probe_manifest="$esp_rtos_probe_root/Cargo.toml"
patch_manifest="$patch_root/MEDITAMER_PATCH.md"

fail() {
  echo "BLE controller patch check failed: $*" >&2
  exit 1
}

for source in "$ble_mod" "$btdm" "$esp32_ble_adapter" "$controller" "$tx_cancellation" "$compat_queue" "$queue_lifecycle"; do
  [[ -f "$source" ]] || fail "missing ${source#"$repo_root/"}"
done
[[ -f "$driver_root/src/queue.rs" ]] || fail "missing fixed RTOS queue implementation"
[[ -f "$serial_task" ]] || fail "missing serial command memory-boundary implementation"
[[ -f "$byte_dispatch" ]] || fail "missing serial command route classification"
[[ -f "$command_dispatch" ]] || fail "missing BLE status formatter"
[[ -f "$firmware_ble_runtime" ]] || fail "missing BLE lifecycle implementation"
grep -Fq "\`$expected_tree_digest\`" "$patch_manifest" \
  || fail "patch manifest tree digest does not match the guarded source digest"
grep -Fq 'const COMPAT_QUEUE_SLOT_COUNT: usize = 8;' "$driver_root/src/queue.rs" \
  || fail "fixed RTOS queue slot capacity changed"
grep -Fq 'const COMPAT_QUEUE_MAX_ITEM_BYTES: usize = 512;' "$driver_root/src/queue.rs" \
  || fail "critical-section item copy ceiling changed"
grep -Fq 'const COMPAT_QUEUE_MAX_PAYLOAD_BYTES: usize = 2 * 1024;' "$driver_root/src/queue.rs" \
  || fail "per-queue payload ceiling changed"
grep -Fq 'const COMPAT_QUEUE_TOTAL_PAYLOAD_BYTES: usize = 2 * 1024;' "$driver_root/src/queue.rs" \
  || fail "aggregate queue payload ceiling changed"
grep -Fq 'const COMPAT_QUEUE_WAIT_POLL_US: u64 = 1_000;' "$driver_root/src/queue.rs" \
  || fail "task queue wait poll interval changed"
grep -Fq 'static COMPAT_QUEUE_SLOTS:' "$driver_root/src/queue.rs" \
  || fail "fixed RTOS queue owner is missing"
if grep -Eq 'Box::new\(CompatQueue|Box::leak\(q\)' "$driver_root/src/queue.rs"; then
  fail "heap-owned compat queue control returned"
fi
grep -Fq 'fn cleanup_before_driver_init(&mut self, original: WifiError) -> WifiError' "$wifi_controller" \
  || fail "pre-driver queue-allocation cleanup is missing"
if grep -Fq 'change_capacity(config.rx_queue_size))?;' "$wifi_controller"; then
  fail "pre-driver queue allocation can bypass epoch cleanup"
fi
grep -Fq 'let callback = super::WifiCallbackGuard::enter_event();' "$wifi_adapter" \
  || fail "Wi-Fi event callback is outside the callback fence"
grep -Fq 'if !callback.admitted {' "$wifi_adapter" \
  || fail "late Wi-Fi events are not suppressed after source shutdown"
grep -Fq 'let packet = PacketBuffer::new(buffer, len, eb);' "$wifi_controller" \
  || fail "Wi-Fi RX ownership telemetry does not cover callback packet construction"
grep -Fq '_meditamer_match_internal_low_water_wifi_rx(buffer.addr(), len as usize, eb.addr());' "$wifi_controller" \
  || fail "station receive callback no longer exposes the allocator correlation boundary"
grep -Fq 'record_wifi_rx_buffer_dropped(self.len);' "$wifi_controller" \
  || fail "Wi-Fi RX ownership telemetry does not cover packet release"
rx_drop_body="$(sed -n '/impl Drop for PacketBuffer/,/^    }/p' "$wifi_controller")"
rx_vendor_free_line="$(grep -n -m1 'esp_wifi_internal_free_rx_buffer' <<<"$rx_drop_body" | cut -d: -f1)"
rx_account_drop_line="$(grep -n -m1 'record_wifi_rx_buffer_dropped' <<<"$rx_drop_body" | cut -d: -f1)"
[[ -n "$rx_vendor_free_line" && -n "$rx_account_drop_line" \
  && "$rx_vendor_free_line" -lt "$rx_account_drop_line" ]] \
  || fail "Wi-Fi RX telemetry releases ownership before vendor free completes"
grep -Fq 'pub fn wifi_rx_buffer_stats() -> WifiRxBufferStats' "$wifi_controller" \
  || fail "Wi-Fi RX ownership telemetry is not exported"
grep -Fq 'command_dispatch::run_low_overhead_diagnostic_command(uart, state, cmd).await;' "$serial_task" \
  || fail "allocator/handoff diagnostics returned to the heap-backed wide dispatcher"
if grep -Fq 'esp_alloc::ExternalMemory' "$serial_task"; then
  fail "serial dispatcher returned to external PSRAM"
fi
grep -Fq 'struct GuardedStorage' "$driver_root/src/queue.rs" \
  || fail "queue payload canaries are missing"
grep -Fq 'static PAYLOAD_ARENA: PayloadArena' "$driver_root/src/queue.rs" \
  || fail "radio queue payload returned to allocator ownership"
grep -Fq 'lock: RawMutex' "$driver_root/src/queue.rs" \
  || fail "queue bookkeeping lacks its per-queue raw lock"
grep -Fq 'inner: RefCell<QueueInner>' "$driver_root/src/queue.rs" \
  || fail "nested same-core queue access is not borrow-checked"
grep -Fq 'TASK_CONTENTION_REJECTED.fetch_add' "$driver_root/src/queue.rs" \
  || fail "task-side nested queue rejection is not observable"
grep -Fq 'ISR_CONTENTION_REJECTED.fetch_add' "$driver_root/src/queue.rs" \
  || fail "ISR-side nested queue rejection is not observable"
grep -Fq 'NONBLOCKING_CONTEXT_REDIRECTED.fetch_add' "$driver_root/src/queue.rs" \
  || fail "nonblocking-context redirection is not observable"
grep -Fq 'xtensa_lx::interrupt::get_level() != 0' "$driver_root/src/queue.rs" \
  || fail "nominally blocking calls are not guarded against ISR context"
grep -Fq 'waiting.wait_until(Some(' "$driver_root/src/queue.rs" \
  || fail "blocking queue waits do not use bounded task-context deadlines"
queue_implementation="$(sed -n '/mod implementation {/,/^}/p' "$driver_root/src/queue.rs")"
if grep -Fq '.notify_from_isr(' <<<"$queue_implementation"; then
  fail "compat queue enters the scheduler from ISR context"
fi
grep -Fq 'version = "=0.13.0"' "$driver_root/Cargo.toml" \
  || fail "Xtensa interrupt-context detector dependency is not exactly pinned"
if grep -Eq 'SemaphoreHandle|NonReentrantMutex|critical_section::|yield_task|allocator_api2|InternalMemory|Box<\[u8' <<<"$queue_implementation"; then
  fail "compat queue returned to reentrant semaphore/mutex control"
fi
grep -Fq 'pub unsafe fn compat_queue_reclaim' "$driver_root/src/queue.rs" \
  || fail "source-quiescent queue reclamation is missing"
driver_delete_body="$(awk '
  /unsafe fn delete\(queue: QueuePtr\)/ { capture = 1 }
  capture && count++ < 8 { print }
  capture && count == 8 { exit }
' "$driver_root/src/queue.rs")"
if grep -Eq 'Box::from_raw|drop\(' <<<"$driver_delete_body"; then
  fail "lower RTOS deletion frees callback-reachable queue storage"
fi
driver_reclaim_body="$(sed -n '/pub unsafe fn compat_queue_reclaim/,/^    }/p' "$driver_root/src/queue.rs")"
grep -Fq 'SLOT_RETIRED' <<<"$driver_reclaim_body" \
  || fail "lower reclamation does not require retirement"
grep -Fq 'drop_in_place()' <<<"$driver_reclaim_body" \
  || fail "lower reclamation does not release retired storage"

grep -Fq 'const SLOT_COUNT: usize = 8;' "$queue_lifecycle" \
  || fail "queue lifecycle registry capacity changed"
grep -Fq 'const OPERATION_SLOT_COUNT: usize = 16;' "$queue_lifecycle" \
  || fail "bounded queue operation registry capacity changed"
grep -Fq 'const TASK_SLOT_COUNT: usize = 4;' "$queue_lifecycle" \
  || fail "bounded BTDM task registry capacity changed"
grep -Fq 'compare_exchange(ACTIVE, RETIRED' "$queue_lifecycle" \
  || fail "queue retirement is not atomic"
grep -Fq 'LATE_USE_REJECTED.fetch_add' "$queue_lifecycle" \
  || fail "late queue operations are not observable"
grep -Fq 'begin_reclaimable_epoch' "$queue_lifecycle" \
  || fail "source-scoped queue epoch is missing"
grep -Fq 'reclaim_current_epoch_after_source_quiescent' "$queue_lifecycle" \
  || fail "source-quiescent lifecycle reclamation is missing"
grep -Fq 'slot.in_flight.load(Ordering::Acquire) != 0' "$queue_lifecycle" \
  || fail "lifecycle reclamation does not reject in-flight operations"
grep -Fq 'complete_btdm_task_delete' "$queue_lifecycle" \
  || fail "BTDM task deletion cannot retire task-owned queue operations"
grep -Fq 'const OPERATION_STATE_BITS: usize = 3;' "$queue_lifecycle" \
  || fail "operation generation and state are not represented by one tagged token"
grep -Fq 'token: AtomicUsize' "$queue_lifecycle" \
  || fail "operation generation and state returned to independently published fields"
grep -Fq '.compare_exchange(' "$queue_lifecycle" \
  || fail "tagged operation claims no longer use compare-exchange"
grep -Fq 'operation_token(generation, COMPLETING_OPERATION)' "$queue_lifecycle" \
  || fail "operation completion publishes a reusable slot before accounting"
completion_body="$(sed -n '/^fn complete_operation_locked(/,/^}/p' "$queue_lifecycle")"
account_line="$(grep -n -m1 'OPERATION_COMPLETED.fetch_add' <<<"$completion_body" | cut -d: -f1)"
in_flight_line="$(grep -n -m1 'in_flight' <<<"$completion_body" | cut -d: -f1)"
empty_line="$(grep -n -m1 'operation_token(generation, EMPTY)' <<<"$completion_body" | cut -d: -f1)"
[[ -n "$account_line" && -n "$in_flight_line" && -n "$empty_line" \
   && "$account_line" -lt "$in_flight_line" && "$in_flight_line" -lt "$empty_line" ]] \
  || fail "operation accounting/quiescence/reuse publication order changed"
grep -Fq 'operation_balance_error' "$queue_lifecycle" \
  || fail "queue operation completion/cancellation balance is not observable"
grep -Fq 'task.state.store(TASK_DELETED, Ordering::Release);' "$queue_lifecycle" \
  || fail "BTDM task completion is not published before reclamation"
if grep -Eq 'queue_header_is_usable|read_volatile|initial_header|observe_heap_dealloc' \
  "$compat_queue" "$queue_lifecycle"; then
  fail "private-layout queue-header diagnostic returned"
fi
grep -Fq 'features = ["alloc-hooks", "compat", "esp32", "global-allocator"]' "$root_manifest" \
  || fail "run-wide internal low-water allocation hook is not enabled"

# ESP-IDF-style, one-shot release of the ESP32 controller's Classic-only EM
# range. The product must take ownership before the first BLE epoch; every
# later BTDM memory initialization must preserve that exact region.
grep -Fq 'const _: () = assert!(CONFIG_BTDM_CTRL_MODE_BLE_ONLY == 1);' "$esp32_ble_adapter" \
  || fail "Classic memory release is not compile-time guarded by BLE-only controller mode"
grep -Fq 'const _: () = assert!(CLASSIC_BT_MEMORY_SIZE == 15_448);' "$esp32_ble_adapter" \
  || fail "Classic Bluetooth memory size is no longer pinned to 15,448 bytes"
grep -Fq 'pub fn release_classic_bt_memory()' "$esp32_ble_adapter" \
  || fail "one-shot Classic Bluetooth memory release API is missing"
grep -Fq 'CLASSIC_BT_MEMORY_STATE.compare_exchange(' "$esp32_ble_adapter" \
  || fail "Classic Bluetooth release is not atomically exclusive with controller startup"
grep -Fq 'region.mode == esp_bt_mode_t_ESP_BT_MODE_CLASSIC_BT' "$esp32_ble_adapter" \
  || fail "BTDM initialization does not identify the released Classic-only region"
grep -Fq '&& !released_classic_region' "$esp32_ble_adapter" \
  || fail "BTDM initialization can still zero the released Classic-only region"
grep -Fq 'classic-bt-memory-reclaim = ["dep:esp-radio", "esp-radio/ble"]' "$product_manifest" \
  || fail "product Classic-memory feature no longer requires the BLE controller"
grep -Fq '"meditamer-product/classic-bt-memory-reclaim"' "$root_manifest" \
  || fail "shared ESP32 BLE runtime no longer enables Classic-memory reclaim"
grep -Fq 'esp_radio::ble::release_classic_bt_memory()' "$psram_init" \
  || fail "allocator initialization does not take Classic-memory ownership"
grep -Fq 'esp_alloc::MemoryCapability::Internal.into(),' "$psram_init" \
  || fail "released Classic memory is not registered as internal-capability RAM"
classic_release_line="$(grep -n -m1 'release_classic_bt_memory()' "$psram_init" | cut -d: -f1)"
classic_add_line="$(grep -n -m1 'esp_alloc::HEAP.add_region' "$psram_init" | cut -d: -f1)"
psram_add_line="$(grep -n -m1 'esp_alloc::psram_allocator!' "$psram_init" | cut -d: -f1)"
[[ -n "$classic_release_line" && -n "$classic_add_line" && -n "$psram_add_line" \
   && "$classic_release_line" -lt "$classic_add_line" \
   && "$classic_add_line" -lt "$psram_add_line" ]] \
  || fail "Classic-memory ownership/addition does not precede PSRAM's final heap-region slot"

grep -Fq 'esp-radio-rtos-driver = "=0.4.2"' "$root_manifest" \
  || fail "direct RTOS settlement driver dependency is not exactly pinned"
grep -A2 -F '[dependencies.bt-hci-transport]' "$patch_root/Cargo.toml" \
  | grep -Fq 'version = "0.1.0"' \
  || fail "reviewed esp-radio source no longer pins the Trouble transport bridge"
grep -Fq '"dep:bt-hci-transport"' "$patch_root/Cargo.toml" \
  || fail "esp-radio BLE feature no longer enables the Trouble transport bridge"
grep -Fq "impl bt_hci_transport::Transport for BleConnector<'_>" "$controller" \
  || fail "bounded BLE connector no longer implements Trouble's transport contract"
grep -Fq 'send_hci(buf).await?;' "$controller" \
  || fail "Trouble transport writer bypasses the bounded async HCI transmit path"
grep -Fq 'unsafe extern "Rust" fn _esp_alloc_alloc(' "$psram_provenance" \
  || fail "allocation hook is not limited to the reviewed low-water recorder"
[[ "$(grep -Fc 'queue_lifecycle::begin_task_use' "$compat_queue")" -eq 5 ]] \
  || fail "not every task queue operation is task-owned and lifecycle-fenced"
[[ "$(grep -Fc 'queue_lifecycle::begin_isr_use' "$compat_queue")" -eq 2 ]] \
  || fail "not every ISR queue operation is separately lifecycle-fenced"
queue_delete_body="$(sed -n '/^pub(crate) fn queue_delete/,/^}/p' "$compat_queue")"
grep -Fq 'queue_lifecycle::retire' <<<"$queue_delete_body" \
  || fail "queue deletion bypasses retirement"
if grep -Eq 'QueueHandle::from_ptr|drop\(' <<<"$queue_delete_body"; then
  fail "outer retirement directly frees callback-reachable queue storage"
fi
grep -Fq 'struct WifiStaticQueue' "$wifi_adapter" \
  || fail "Wi-Fi static queue ABI wrapper is missing"
grep -B1 -F 'struct WifiStaticQueue' "$wifi_adapter" | grep -Fq '#[repr(C)]' \
  || fail "Wi-Fi static queue wrapper is not repr(C)"
grep -Fq 'storage: *mut c_void' "$wifi_adapter" \
  || fail "Wi-Fi static queue ABI wrapper lacks its storage field"
grep -Fq 'reclaim_current_epoch_after_source_quiescent()' "$btdm" \
  || fail "BTDM teardown does not reclaim its source-scoped queues"
grep -Fq 'extern "C" fn btdm_task_entry' "$btdm" \
  || fail "BTDM task entry no longer registers before vendor code"
grep -Fq 'register_btdm_task(current.as_ptr() as usize)' "$btdm" \
  || fail "BTDM task trampoline does not self-register current_task"
grep -Fq 'while bootstrap.state.load(Ordering::Acquire) != TASK_BOOTSTRAP_EMPTY' "$btdm" \
  || fail "BTDM task can run vendor code before handle publication"
entry_body="$(sed -n '/^extern "C" fn btdm_task_entry/,/^}/p' "$btdm")"
grep -Fq 'const TASK_BOOTSTRAP_POLL_US: u32 = 1_000;' "$btdm" \
  || fail "BTDM bootstrap blocking interval is not pinned"
grep -Fq 'crate::preempt::usleep(TASK_BOOTSTRAP_POLL_US);' <<<"$entry_body" \
  || fail "higher-priority BTDM bootstrap wait does not block for creator handle publication"
if grep -Fq 'crate::preempt::yield_task();' <<<"$entry_body"; then
  fail "higher-priority BTDM bootstrap can yield-deadlock its creator"
fi
entry_register_line="$(grep -n -m1 'register_btdm_task(current.as_ptr() as usize)' <<<"$entry_body" | cut -d: -f1)"
entry_release_line="$(grep -n -m1 'while bootstrap.state.load' <<<"$entry_body" | cut -d: -f1)"
entry_vendor_line="$(grep -n -m1 'function(parameter);' <<<"$entry_body" | cut -d: -f1)"
[[ -n "$entry_register_line" && -n "$entry_release_line" && -n "$entry_vendor_line" \
   && "$entry_register_line" -lt "$entry_release_line" && "$entry_release_line" -lt "$entry_vendor_line" ]] \
  || fail "BTDM task entry can execute vendor code before registration and creator release"
grep -Fq 'const BTDM_LIFECYCLE_CORE: u32 = 0;' "$btdm" \
  || fail "BTDM task/deinit core affinity is not pinned"
grep -Fq 'if core_id != BTDM_LIFECYCLE_CORE {' "$btdm" \
  || fail "BTDM task creation can accept an unsafe core affinity"
grep -Fq 'prepare_btdm_task_delete' "$btdm" \
  || fail "BTDM task deletion is not correlated to task-owned operations"
grep -Fq 'if target == current {' "$btdm" \
  || fail "explicit-current BTDM task deletion can bypass pre-delete operation cancellation"
[[ "$(grep -Fc 'complete_btdm_task_delete(delete);' "$btdm")" -eq 3 ]] \
  || fail "returning, current, and non-current BTDM task deletion paths must retire operations"
if grep -Fq 'BLE queue reclamation failed after BTDM source shutdown");' "$btdm"; then
  fail "BTDM teardown returned to panic-on-reclamation-failure"
fi
grep -Fq 'let teardown_transport = hci_transport_stats();' "$firmware_ble_runtime" \
  || fail "firmware does not resample transport faults after BLE teardown"
stack_drop_line="$(grep -n -m1 'drop(stack);' "$firmware_ble_runtime" | cut -d: -f1)"
resources_clear_line="$(grep -n -m1 'HOST_RESOURCES.clear();' "$firmware_ble_runtime" | cut -d: -f1)"
teardown_transport_line="$(grep -n -m1 'let teardown_transport = hci_transport_stats();' "$firmware_ble_runtime" | cut -d: -f1)"
[[ -n "$stack_drop_line" && -n "$resources_clear_line" && -n "$teardown_transport_line" \
   && "$stack_drop_line" -lt "$resources_clear_line" \
   && "$resources_clear_line" -lt "$teardown_transport_line" ]] \
  || fail "transport fault snapshot occurs before connector teardown"
grep -Fq 'heapless::String::<768>::new()' "$command_dispatch" \
  || fail "BLE terminal status envelope is too small for bounded fault telemetry"
grep -Fq 'queue_task_cancelled={} queue_balance={} queue_task_live={} queue_task_faults={} queue_op_full={}' \
  "$command_dispatch" \
  || fail "BLE terminal queue/task field order changed without host protocol review"
[[ "$(grep -Fc 'SerialCommand::BlePhase1sStatus => write_ble_phase1s_status(uart).await' "$command_dispatch")" -eq 1 ]] \
  || fail "BLE status formatter returned to the heap-backed wide dispatcher"
wide_dispatch_body="$(sed -n '/^pub(super) async fn handle_serial_command/,/^}/p' "$command_dispatch")"
grep -Fq 'unreachable!("low-overhead BLE lifecycle command reached boxed dispatcher")' \
  <<<"$wide_dispatch_body" \
  || fail "BLE lifecycle command can allocate the heap-backed wide dispatcher"
local_dispatch_body="$(sed -n '/^async fn handle_local_command/,/^}/p' "$command_dispatch")"
if grep -Eq 'StackStatus|AllocatorStatus|BlePhase1s(Start|Status)|RadioHandoff(Acquire|Release|Status)' \
  <<<"$local_dispatch_body"; then
  fail "wide local-command future retains low-overhead lifecycle or memory branches"
fi
serial_route_body="$(sed -n '/^async fn handle_uart_byte/,/^}/p' "$serial_task")"
route_classification_body="$(sed -n '/^pub(super) fn is_low_overhead_diagnostic/,/^}/p' "$byte_dispatch")"
low_overhead_body="$(sed -n '/^pub(super) async fn run_low_overhead_diagnostic_command/,/^}/p' "$command_dispatch")"
grep -Fq 'match byte_dispatch::route_for_command(&cmd)' <<<"$serial_route_body" \
  || fail "serial command no longer uses the bounded route classification"
grep -Fq 'byte_dispatch::DispatchRoute::LowOverhead =>' <<<"$serial_route_body" \
  || fail "serial command no longer has a low-overhead route"
grep -Fq 'command_dispatch::run_low_overhead_diagnostic_command(uart, state, cmd).await' <<<"$serial_route_body" \
  || fail "low-overhead route no longer bypasses the boxed dispatcher"
grep -Fq 'if is_low_overhead_diagnostic(cmd)' "$byte_dispatch" \
  || fail "low-overhead diagnostic classification is disconnected"
grep -Fq 'return DispatchRoute::LowOverhead;' "$byte_dispatch" \
  || fail "low-overhead diagnostic classification no longer selects its route"
for variant in \
  StackStatus \
  AllocatorStatus \
  NetStatus \
  StateSet \
  StateDiag \
  NetStart \
  NetStop \
  BlePhase1sStart \
  BlePhase1sStatus \
  RadioHandoffAcquire \
  RadioHandoffRelease \
  RadioHandoffStatus; do
  grep -Fq "SerialCommand::$variant" <<<"$route_classification_body" \
    || fail "$variant no longer bypasses the heap-backed wide dispatcher"
  grep -Fq "SerialCommand::$variant" <<<"$low_overhead_body" \
    || fail "$variant is routed low-overhead but has no bounded implementation"
done
grep -Fq 'struct WifiQueueEpochGuard' "$wifi_controller" \
  || fail "Wi-Fi queue epoch guard is missing"
grep -Fq 'let mut queue_epoch = WifiQueueEpochGuard::begin();' "$wifi_controller" \
  || fail "Wi-Fi controller does not begin a source-scoped queue epoch"
grep -Fq 'reclaim_current_epoch_after_source_quiescent()' "$wifi_controller" \
  || fail "Wi-Fi controller teardown does not reclaim its queue epoch"
grep -Fq 'pub fn shutdown_source(&mut self)' "$wifi_controller" \
  || fail "Wi-Fi controller has no fallible source shutdown"
grep -Fq 'pub fn finalize_shutdown(&mut self)' "$wifi_controller" \
  || fail "Wi-Fi controller has no explicit queue finalization"
grep -Fq 'drop(self.guard.take());' "$wifi_controller" \
  || fail "radio guard is not released before queue reclamation"
grep -Fq 'WIFI_CALLBACK_IN_FLIGHT.load(Ordering::Acquire)' "$wifi_controller" \
  || fail "Wi-Fi callback in-flight fence is missing"

grep -Fq 'version = "1.0.0-beta.1"' "$patch_root/Cargo.toml" || fail "unexpected base version"
grep -A2 -F '[dependencies.embassy-time]' "$patch_root/Cargo.toml" \
  | grep -Fq 'version = "=0.5.1"' || fail "async deadline dependency is not exactly pinned"
grep -Fq 'Deque<ReceivedPacket, RX_QUEUE_CAPACITY>' "$ble_mod" || fail "receive queue is not fixed"
grep -Fq 'Vec<u8, HCI_PACKET_CAPACITY>' "$ble_mod" || fail "packet storage is not fixed"
grep -Fq 'record_rx_queue_overflow' "$btdm" || fail "receive overflow is not observable"
grep -Fq 'HCI_TX_TIMEOUT' "$btdm" || fail "transmit deadline is missing"
grep -Fq 'static HCI_TX_WAKER: AtomicWaker' "$btdm" || fail "transmit callback waker is missing"
grep -Fq 'HCI_TX_WAKER.register(cx.waker())' "$btdm" || fail "transmit wait does not register its waker"
grep -Fq 'HCI_TX_WAKER.wake()' "$btdm" || fail "controller callback does not wake transmit"
grep -Fq 'with_timeout(HCI_TX_TIMEOUT' "$btdm" || fail "async transmit wait has no timer-backed deadline"
grep -Fq 'pub async fn send_hci' "$btdm" || fail "HCI transmit path is not async"
grep -Fq 'struct TxCancellationGuard' "$tx_cancellation" || fail "transmit cancellation guard is missing"
grep -Fq 'impl<L: TxCancellationLatch> Drop for TxCancellationGuard' "$tx_cancellation" \
  || fail "transmit cancellation is not drop-guarded"
grep -Fq 'latch_transport_fault();' "$btdm" || fail "transmit cancellation does not latch a fault"
grep -Fq 'if transport_faulted() {' "$btdm" || fail "queued transmit does not recheck the fault after locking"
grep -Fq 'cancellation_before_controller_availability_latches_fault' "$tx_cancellation" \
  || fail "pre-submission cancellation test is missing"
grep -Fq 'cancellation_after_packet_submission_latches_fault' "$tx_cancellation" \
  || fail "post-submission cancellation test is missing"
grep -Fq 'send_hci(&buf[..len]).await?' "$controller" || fail "controller transport does not await HCI transmit"
grep -Fq 'Result<(), HciTransportError>' "$btdm" || fail "transport errors are not returned"
grep -Fq 'transport_faulted()' "$btdm" || fail "timeout fault is not latched"
grep -Fq 'TRANSPORT_FAULTED.store(true' "$ble_mod" || fail "timeout cannot latch a fault"
grep -Fq 'Transport(HciTransportError)' "$controller" || fail "connector drops transport errors"
grep -Fq 'static CALLBACK_ADMISSION_OPEN: AtomicBool' "$btdm" \
  || fail "callback admission fence is missing"
grep -Fq 'CALLBACK_IN_FLIGHT.fetch_add(1, Ordering::AcqRel)' "$btdm" \
  || fail "callback entry is not counted"
grep -Fq 'CALLBACK_IN_FLIGHT.fetch_sub(1, Ordering::AcqRel)' "$btdm" \
  || fail "callback exit is not counted"
grep -Fq 'CONTROLLER_CALLBACK_SOURCE_ACTIVE.swap(false, Ordering::AcqRel)' "$btdm" \
  || fail "shutdown does not atomically claim controller disable"
grep -Fq 'btdm_controller_disable();' "$btdm" \
  || fail "shutdown does not disable the controller callback source"
grep -Fq 'with_timeout(timeout, quiescent).await.is_ok()' "$btdm" \
  || fail "callback quiescence wait has no bounded deadline"
[[ "$(grep -Fc 'let callback = HciCallbackGuard::enter();' "$btdm")" -eq 2 ]] \
  || fail "every VHCI callback must enter the admission/in-flight fence"

send_hci_body="$(sed -n '/^pub async fn send_hci/,/^}/p' "$btdm")"
if grep -Eq 'crate::preempt::yield_task|Instant::now|(^|[[:space:]])(loop|while)[[:space:]]' \
  <<<"$send_hci_body"; then
  fail "HCI transmit path contains a synchronous polling/yield loop"
fi

lock_line="$(grep -n -m1 'HCI_OUT_COLLECTOR.lock()' <<<"$send_hci_body" | cut -d: -f1)"
fault_recheck_line="$(grep -n 'if transport_faulted() {' <<<"$send_hci_body" | tail -1 | cut -d: -f1)"
push_line="$(grep -n -m1 'hci_out.push(data)' <<<"$send_hci_body" | cut -d: -f1)"
guard_line="$(grep -n -m1 'let mut cancellation_guard' <<<"$send_hci_body" | cut -d: -f1)"
first_wait_line="$(grep -n -m1 'if wait_for_tx_signal' <<<"$send_hci_body" | cut -d: -f1)"
[[ "$lock_line" -lt "$fault_recheck_line" && "$fault_recheck_line" -lt "$push_line" ]] \
  || fail "queued transmit fault recheck is not between collector lock and append"
[[ "$guard_line" -lt "$first_wait_line" ]] \
  || fail "transmit cancellation guard is not armed before the first cancellable wait"
[[ "$(grep -Fc 'cancellation_guard.disarm();' <<<"$send_hci_body")" -eq 3 ]] \
  || fail "transmit cancellation guard is not disarmed on all normal timeout/success exits"

if grep -Eq 'Box::from\(data\)|VecDeque<ReceivedPacket>|while !PACKET_SENT[^\{]*\{\}' "$ble_mod" "$btdm"; then
  fail "an unbounded or busy-wait transport pattern returned"
fi

# Temporary Xtensa context-copy probe: the workspace root and the Inkplate
# target both patch esp-rtos 0.4.0 at the local probe tree (qualified in
# vendor/esp-rtos-0.4.0-context-probe/MEDITAMER_PATCH.md). A path-patched
# crate never carries a registry source/checksum, so each lock stanza must
# pin version 0.4.0 with no source/checksum lines -- a checksum here would
# mean the probe patch is bypassed, not that it is intact.
esp_rtos_lock="$(awk '/^name = "esp-rtos"$/ { found=1 } found { print } found && /^$/ { exit }' "$repo_root/Cargo.lock")"
grep -Fq 'version = "0.4.0"' <<<"$esp_rtos_lock" \
  || fail "workspace Cargo.lock no longer resolves esp-rtos 0.4.0"
if grep -Eq '^(source|checksum) =' <<<"$esp_rtos_lock"; then
  fail "workspace Cargo.lock resolved esp-rtos from the registry instead of the local probe"
fi
esp_rtos_target_lock="$(awk '/^name = "esp-rtos"$/ { found=1 } found { print } found && /^$/ { exit }' "$repo_root/targets/meditamer-inkplate/Cargo.lock")"
grep -Fq 'version = "0.4.0"' <<<"$esp_rtos_target_lock" \
  || fail "Inkplate Cargo.lock no longer resolves esp-rtos 0.4.0"
if grep -Eq '^(source|checksum) =' <<<"$esp_rtos_target_lock"; then
  fail "Inkplate Cargo.lock resolved esp-rtos from the registry instead of the local probe"
fi
# These digests still pin the specific source this patch's
# `queue_lifecycle` bounded registry relies on: task deletion frees the
# task's stack allocation without unwinding whatever the task's own stack
# was still running.
grep -Fq 'esp-rtos = { path = "vendor/esp-rtos-0.4.0-context-probe" }' "$repo_root/Cargo.toml" \
  || fail "workspace no longer patches esp-rtos to the reviewed probe tree"
grep -Fq 'esp-rtos = { path = "../../vendor/esp-rtos-0.4.0-context-probe" }' "$root_manifest" \
  || fail "Inkplate target no longer patches esp-rtos to the reviewed probe tree"
grep -Fq 'esp-rtos = { version = "0.4.0"' "$root_manifest" \
  || fail "Inkplate target esp-rtos dependency is not pinned to 0.4.0"
grep -Fq 'name = "esp-rtos"' "$esp_rtos_probe_manifest" \
  || fail "probe manifest is not the esp-rtos package"
grep -Fq 'version = "0.4.0"' "$esp_rtos_probe_manifest" \
  || fail "probe manifest is not version 0.4.0"
grep -Fq 'context-probe = []' "$esp_rtos_probe_manifest" \
  || fail "probe manifest lost the context-probe feature"
resolve_esp_rtos_manifest() {
  cargo metadata --locked --format-version 1 --manifest-path "$1" 2>/dev/null | python3 -c '
import json, sys
packages = [p for p in json.load(sys.stdin)["packages"] if p["name"] == "esp-rtos" and p["version"] == "0.4.0"]
if len(packages) != 1:
    raise SystemExit(2)
print(packages[0]["manifest_path"])
'
}
esp_rtos_manifest="$(resolve_esp_rtos_manifest "$repo_root/Cargo.toml")" \
  || fail "could not resolve exactly one esp-rtos 0.4.0 source in the workspace graph"
[[ "$esp_rtos_manifest" == "$(realpath "$esp_rtos_probe_manifest")" ]] \
  || fail "workspace graph did not resolve esp-rtos to the reviewed probe tree: $esp_rtos_manifest"
esp_rtos_target_manifest="$(resolve_esp_rtos_manifest "$root_manifest")" \
  || fail "could not resolve exactly one esp-rtos 0.4.0 source in the Inkplate graph"
[[ "$esp_rtos_target_manifest" == "$(realpath "$esp_rtos_probe_manifest")" ]] \
  || fail "Inkplate graph did not resolve esp-rtos to the reviewed probe tree: $esp_rtos_target_manifest"
esp_rtos_root="$(dirname "$esp_rtos_manifest")"
actual_scheduler_digest="$(shasum -a 256 "$esp_rtos_root/src/scheduler.rs" | awk '{print $1}')"
actual_task_digest="$(shasum -a 256 "$esp_rtos_root/src/task/mod.rs" | awk '{print $1}')"
actual_timer_digest="$(shasum -a 256 "$esp_rtos_root/src/timer/mod.rs" | awk '{print $1}')"
actual_xtensa_digest="$(shasum -a 256 "$esp_rtos_root/src/task/xtensa.rs" | awk '{print $1}')"
actual_context_probe_digest="$(shasum -a 256 "$esp_rtos_root/src/context_probe.rs" | awk '{print $1}')"
actual_rtos_lib_digest="$(shasum -a 256 "$esp_rtos_root/src/lib.rs" | awk '{print $1}')"
[[ "$actual_scheduler_digest" == "$expected_esp_rtos_scheduler_digest" ]] \
  || fail "esp-rtos scheduler deletion source changed: $actual_scheduler_digest"
[[ "$actual_task_digest" == "$expected_esp_rtos_task_digest" ]] \
  || fail "esp-rtos task deletion source changed: $actual_task_digest"
[[ "$actual_timer_digest" == "$expected_esp_rtos_timer_digest" ]] \
  || fail "esp-rtos timer cold-error source changed: $actual_timer_digest"
[[ "$actual_xtensa_digest" == "$expected_esp_rtos_xtensa_digest" ]] \
  || fail "esp-rtos Xtensa context-switch probe changed: $actual_xtensa_digest"
[[ "$actual_context_probe_digest" == "$expected_esp_rtos_context_probe_digest" ]] \
  || fail "esp-rtos context-probe record changed: $actual_context_probe_digest"
[[ "$actual_rtos_lib_digest" == "$expected_esp_rtos_lib_digest" ]] \
  || fail "esp-rtos probe module wiring changed: $actual_rtos_lib_digest"
actual_esp_rtos_tree_digest="$({
  find "$esp_rtos_root" -type f ! -name MEDITAMER_PATCH.md -print0 \
    | LC_ALL=C sort -z \
    | while IFS= read -r -d '' source; do
        digest="$(shasum -a 256 "$source" | awk '{print $1}')"
        printf '%s  %s\n' "$digest" "${source#"$esp_rtos_root/"}"
      done
} | shasum -a 256 | awk '{print $1}')"
[[ "$actual_esp_rtos_tree_digest" == "$expected_esp_rtos_tree_digest" ]] \
  || fail "esp-rtos probe tree changed outside the reviewed files: $actual_esp_rtos_tree_digest"
grep -Fq 'context-probe = ["esp-rtos/context-probe", "esp-radio?/meditamer-connect-phase-probe"]' "$root_manifest" \
  || fail "Inkplate target no longer wires context-probe to esp-rtos"
grep -Fq 'esp_rtos::context_probe::take()' "$repo_root/targets/meditamer-inkplate/src/system.rs" \
  || fail "Inkplate boot no longer drains the probe record"

expected_driver_manifest="$driver_root/Cargo.toml"
resolved_driver_manifest="$(cargo metadata --locked --format-version 1 2>/dev/null | python3 -c '
import json, os, sys
packages = [p for p in json.load(sys.stdin)["packages"] if p["name"] == "esp-radio-rtos-driver" and p["version"] == "0.4.2"]
if len(packages) != 1:
    raise SystemExit(2)
print(os.path.realpath(packages[0]["manifest_path"]))
')" || fail "could not resolve exactly one esp-radio-rtos-driver 0.4.2 source"
[[ "$resolved_driver_manifest" == "$(realpath "$expected_driver_manifest")" ]] \
  || fail "RTOS settlement driver did not resolve to the reviewed vendor tree: $resolved_driver_manifest"

actual_tree_digest="$({
  find "$patch_root" -type f ! -name MEDITAMER_PATCH.md -print0 \
    | LC_ALL=C sort -z \
    | while IFS= read -r -d '' source; do
        digest="$(shasum -a 256 "$source" | awk '{print $1}')"
        printf '%s  %s\n' "$digest" "${source#"$patch_root/"}"
      done
} | shasum -a 256 | awk '{print $1}')"
[[ "$actual_tree_digest" == "$expected_tree_digest" ]] \
  || fail "patched source tree digest changed: $actual_tree_digest"

actual_driver_digest="$({
  find "$driver_root" -type f ! -name MEDITAMER_PATCH.md -print0 \
    | LC_ALL=C sort -z \
    | while IFS= read -r -d '' source; do
        digest="$(shasum -a 256 "$source" | awk '{print $1}')"
        printf '%s  %s\n' "$digest" "${source#"$driver_root/"}"
      done
} | shasum -a 256 | awk '{print $1}')"
[[ "$actual_driver_digest" == "$expected_driver_digest" ]] \
  || fail "patched RTOS driver tree digest changed: $actual_driver_digest"

# Shared BLE runtime and roles plan, Phase 1
# (docs/plans/shared-ble-host-and-role-capabilities.md): the shared runtime crate and its
# ESP32-S3 selection wiring are as load-bearing to the reviewed BLE source actually building as
# the vendor patch itself, so this guard covers them too.
shared_ble_manifest="$shared_ble_root/Cargo.toml"
[[ -f "$shared_ble_manifest" ]] || fail "missing shared BLE runtime crate"
grep -Fq 'esp32 = [' "$shared_ble_manifest" \
  || fail "shared BLE runtime crate no longer forwards ESP32 chip selection"
grep -Fq 'esp32s3 = [' "$shared_ble_manifest" \
  || fail "shared BLE runtime crate no longer forwards ESP32-S3 chip selection"
grep -Fq '"esp-radio/esp32s3"' "$shared_ble_manifest" \
  || fail "shared BLE runtime crate's esp32s3 feature no longer forwards to esp-radio"
grep -Fq 'features = ["ble"' "$shared_ble_manifest" \
  || fail "shared BLE runtime crate no longer requests esp-radio's ble feature"
grep -Fq 'bt-hci = "=0.10.1"' "$shared_ble_manifest" \
  || fail "shared BLE runtime crate's bt-hci dependency is no longer exactly pinned"
grep -Fq 'trouble-host = { version = "=0.8.0"' "$shared_ble_manifest" \
  || fail "shared BLE runtime crate's Trouble dependency is no longer exactly pinned"

grep -Fq 'esp-radio = { path = "../../vendor/esp-radio-1.0.0-beta.1-bounded" }' "$waveshare_manifest" \
  || fail "ESP32-S3 target does not patch esp-radio to the reviewed vendor source"
grep -Fq 'esp-radio-rtos-driver = { path = "../../vendor/esp-radio-rtos-driver-0.4.2-retained" }' \
  "$waveshare_manifest" \
  || fail "ESP32-S3 target does not patch esp-radio-rtos-driver to the reviewed vendor source"
grep -Fq 'esp-alloc = { path = "../../vendor/esp-alloc-0.11.0-provenance" }' "$waveshare_manifest" \
  || fail "ESP32-S3 target does not patch esp-alloc to the reviewed vendor source"
grep -Fq 'ble = { path = "../../platform/connectivity/ble"' "$waveshare_manifest" \
  || fail "ESP32-S3 target no longer depends on the shared BLE runtime crate"
grep -Fq '"ble/esp32s3"' "$waveshare_manifest" \
  || fail "ESP32-S3 target's shared-ble-runtime feature no longer selects esp32s3 on the shared crate"

grep -Fq 'ble = { path = "../../platform/connectivity/ble"' "$root_manifest" \
  || fail "ESP32 target no longer depends on the shared BLE runtime crate"
grep -Fq '"ble/esp32"' "$root_manifest" \
  || fail "ESP32 target's shared-ble-runtime feature no longer selects esp32 on the shared crate"

# Optional allocator correlation hook (see "Optional allocator correlation hook" in
# the vendor patch manifest): `meditamer-allocation-provenance` is Inkplate-only.
# The vendor tree gates exactly the extern declaration and its invocation in
# `recv_cb_sta`; both Inkplate manifests opt in while Medinote and the shared BLE
# crate stay on the hook-free build, and the provider symbol stays product-owned.
grep -Fq 'meditamer-allocation-provenance = []' "$patch_root/Cargo.toml" \
  || fail "vendor meditamer-allocation-provenance feature declaration is missing"
[[ "$(grep -Fc 'cfg(feature = "meditamer-allocation-provenance")' "$wifi_controller")" -eq 2 ]] \
  || fail "station receive hook must gate exactly the extern declaration and its invocation"
grep -F 'esp-radio = {' "$product_manifest" | grep -Fq 'meditamer-allocation-provenance' \
  || fail "Inkplate product esp-radio dependency does not opt into meditamer-allocation-provenance"
grep -F 'esp-radio = {' "$root_manifest" | grep -Fq 'meditamer-allocation-provenance' \
  || fail "Inkplate target esp-radio dependency does not opt into meditamer-allocation-provenance"
if grep -F 'esp-radio = {' "$waveshare_manifest" | grep -Fq 'meditamer-allocation-provenance'; then
  fail "Medinote esp-radio dependency must not opt into meditamer-allocation-provenance"
fi
if grep -F 'esp-radio = {' "$shared_ble_manifest" | grep -Fq 'meditamer-allocation-provenance'; then
  fail "shared BLE esp-radio dependency must not opt into meditamer-allocation-provenance"
fi
grep -Fq 'unsafe extern "C" fn _meditamer_match_internal_low_water_wifi_rx(' "$psram_provenance" \
  || fail "allocation provenance provider symbol is missing from psram provenance"

# Check the resolved target graph: manifest text alone misses transitive feature
# unification, which previously enabled central/pairing in the peripheral image.
# Exclude dev dependencies, and use each board's target rather than the host graph.
check_chip_only_graph() {
  local chip="$1" target="$2" tree
  tree="$(cargo tree --locked --manifest-path "$shared_ble_manifest" --target "$target" \
    --no-default-features --features "$chip" -e normal,build --prefix none --format '{p}')" \
    || fail "$chip controller-only: could not resolve shared BLE dependencies"
  [[ "$tree" == 'ble v0.1.0 '* ]] \
    || fail "$chip controller-only: expected the shared BLE dependency graph"
  if grep -Eq '^trouble-host v' <<< "$tree"; then
    fail "$chip controller-only: unexpectedly includes the Trouble host"
  fi
  echo "BLE role feature check passed: $chip controller-only"
}

check_trouble_roles() {
  local label="$1" manifest="$2" target="$3" roles="$4"
  shift 4
  local tree features feature
  tree="$(cargo tree --locked --manifest-path "$manifest" --target "$target" \
    -e normal,build -i trouble-host --depth 0 --prefix none --format '{p}|{f}' "$@")" \
    || fail "$label: could not resolve Trouble features"
  # `{p}` renders a parenthesized source annotation for patched sources
  # (e.g. `trouble-host v0.8.0 (path)`), so accept it while still pinning
  # exactly version 0.8.0 on exactly one output line.
  local trouble_package_re='^trouble-host v0\.8\.0( \([^)]*\))?\|'
  [[ "$tree" =~ $trouble_package_re && "$tree" != *$'\n'* ]] \
    || fail "$label: expected exactly one Trouble 0.8.0 dependency"
  features=",${tree#*|},"
  [[ "$features" == *,gatt,* ]] || fail "$label: GATT support is missing"

  local central_features=(central scan security legacy-pairing default-packet-pool \
    gatt-client-notification-queue-size-8)
  for feature in "${central_features[@]}"; do
    if [[ "$roles" == peripheral ]]; then
      [[ "$features" != *",$feature,"* ]] \
        || fail "$label: peripheral-only image unexpectedly enables $feature"
    else
      [[ "$features" == *",$feature,"* ]] \
        || fail "$label: central role requires $feature"
    fi
  done

  if [[ "$roles" == central ]]; then
    [[ "$features" != *,peripheral,* ]] \
      || fail "$label: central-only image unexpectedly enables peripheral"
  else
    for feature in peripheral derive connection-event-queue-size-2 \
      l2cap-rx-queue-size-2 l2cap-tx-queue-size-2; do
      [[ "$features" == *",$feature,"* ]] \
        || fail "$label: diagnostic peripheral requires $feature"
    done
  fi

  # Multiple numeric features can silently select a different bound. Preserve
  # both the diagnostic peripheral's queues and the proven CheerTok burst queue.
  local enabled_features
  IFS=',' read -r -a enabled_features <<< "${tree#*|}"
  for feature in "${enabled_features[@]}"; do
    case "$feature" in
      gatt-client-notification-queue-size-*)
        [[ "$roles" != peripheral && "$feature" == gatt-client-notification-queue-size-8 ]] \
          || fail "$label: unexpected notification queue bound $feature"
        ;;
      connection-event-queue-size-*|l2cap-rx-queue-size-*|l2cap-tx-queue-size-*)
        [[ "$roles" == central || "$feature" == *-size-2 ]] \
          || fail "$label: unexpected diagnostic queue bound $feature"
        ;;
    esac
  done
  echo "BLE role feature check passed: $label"
}

check_chip_only_graph esp32 xtensa-esp32-none-elf
check_chip_only_graph esp32s3 xtensa-esp32s3-none-elf
check_trouble_roles "Inkplate diagnostic peripheral" "$root_manifest" \
  xtensa-esp32-none-elf peripheral --features ble-foundation
check_trouble_roles "Inkplate explicit central" "$root_manifest" \
  xtensa-esp32-none-elf central --no-default-features --features shared-ble-runtime
check_trouble_roles "Inkplate combined roles" "$root_manifest" \
  xtensa-esp32-none-elf combined --features ble-foundation,shared-ble-runtime
check_trouble_roles "Medinote default CheerTok central" "$waveshare_manifest" \
  xtensa-esp32s3-none-elf central

echo "BLE controller patch check passed"
