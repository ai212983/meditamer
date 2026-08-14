#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
patch_root="$repo_root/vendor/esp-radio-1.0.0-beta.0-bounded"
driver_root="$repo_root/vendor/esp-radio-rtos-driver-0.3.0-retained"
ble_mod="$patch_root/src/ble/mod.rs"
btdm="$patch_root/src/ble/btdm.rs"
controller="$patch_root/src/ble/controller/mod.rs"
tx_cancellation="$patch_root/src/ble/tx_cancellation.rs"
compat_queue="$patch_root/src/compat/queue.rs"
queue_lifecycle="$patch_root/src/compat/queue_lifecycle.rs"
wifi_adapter="$patch_root/src/wifi/os_adapter/mod.rs"
wifi_controller="$patch_root/src/wifi/mod.rs"
root_manifest="$repo_root/Cargo.toml"
serial_task="$repo_root/src/firmware/serial.rs"
command_dispatch="$repo_root/src/firmware/serial/command_dispatch.rs"
firmware_ble="$repo_root/src/firmware/ble/mod.rs"
expected_tree_digest="5f57341b1daf5182e183fbe1bbce4c46b85852d085f491b43cb5c345949df446"
expected_driver_digest="6210f7f63f290aaf6bd412faa6de8ae18dff225fdf340b25daf7b61eb7fe0e1f"
expected_esp_rtos_scheduler_digest="809eb9122d9e1a40718e48670148facd9a474c2869b926edd071f2c394a1bcbc"
expected_esp_rtos_task_digest="2460ac37416b4a041e6fab46ecf8b8dfcf8634a9fcdb0f1a307d8aa359082a2a"
expected_esp_rtos_timer_digest="c75310a53ecf3cc0ef820d57a551866f6aa29b48098ffbe289fbcf0e742a485a"
patch_manifest="$patch_root/MEDITAMER_PATCH.md"

fail() {
  echo "BLE controller patch check failed: $*" >&2
  exit 1
}

for source in "$ble_mod" "$btdm" "$controller" "$tx_cancellation" "$compat_queue" "$queue_lifecycle"; do
  [[ -f "$source" ]] || fail "missing ${source#"$repo_root/"}"
done
[[ -f "$driver_root/src/queue.rs" ]] || fail "missing fixed RTOS queue implementation"
[[ -f "$serial_task" ]] || fail "missing serial command memory-boundary implementation"
[[ -f "$command_dispatch" ]] || fail "missing BLE status formatter"
[[ -f "$firmware_ble" ]] || fail "missing BLE lifecycle implementation"
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
grep -Fq 'esp-radio-rtos-driver = "=0.3.0"' "$root_manifest" \
  || fail "direct RTOS settlement driver dependency is not exactly pinned"
grep -Fq 'unsafe extern "Rust" fn _esp_alloc_alloc(' "$repo_root/src/firmware/psram/provenance.rs" \
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
grep -Fq 'let teardown_transport = hci_transport_stats();' "$firmware_ble" \
  || fail "firmware does not resample transport faults after BLE teardown"
stack_clear_line="$(grep -n -m1 'HOST_STACK.clear();' "$firmware_ble" | cut -d: -f1)"
teardown_transport_line="$(grep -n -m1 'let teardown_transport = hci_transport_stats();' "$firmware_ble" | cut -d: -f1)"
[[ -n "$stack_clear_line" && -n "$teardown_transport_line" \
   && "$stack_clear_line" -lt "$teardown_transport_line" ]] \
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
low_overhead_body="$(sed -n '/^pub(super) async fn run_low_overhead_diagnostic_command/,/^}/p' "$command_dispatch")"
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
  grep -Fq "SerialCommand::$variant" <<<"$serial_route_body" \
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

grep -Fq 'version = "1.0.0-beta.0"' "$patch_root/Cargo.toml" || fail "unexpected base version"
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

esp_rtos_lock="$(awk 'BEGIN { found=0 } /^name = "esp-rtos"$/ { found=1 } found { print } found && /^$/ { exit }' "$repo_root/Cargo.lock")"
grep -Fq 'version = "0.3.0"' <<<"$esp_rtos_lock" \
  || fail "Cargo.lock no longer resolves esp-rtos 0.3.0"
grep -Fq 'checksum = "551f90766e1527edaa0c91e8d559e9e2a60397b545e93357ac61fb31845e5712"' <<<"$esp_rtos_lock" \
  || fail "Cargo.lock esp-rtos checksum changed"
esp_rtos_manifest="$(cargo metadata --locked --format-version 1 2>/dev/null | python3 -c '
import json, sys
packages = [p for p in json.load(sys.stdin)["packages"] if p["name"] == "esp-rtos" and p["version"] == "0.3.0"]
if len(packages) != 1:
    raise SystemExit(2)
print(packages[0]["manifest_path"])
')" || fail "could not resolve exactly one esp-rtos 0.3.0 source"
esp_rtos_root="$(dirname "$esp_rtos_manifest")"
actual_scheduler_digest="$(shasum -a 256 "$esp_rtos_root/src/scheduler.rs" | awk '{print $1}')"
actual_task_digest="$(shasum -a 256 "$esp_rtos_root/src/task/mod.rs" | awk '{print $1}')"
actual_timer_digest="$(shasum -a 256 "$esp_rtos_root/src/timer/mod.rs" | awk '{print $1}')"
[[ "$actual_scheduler_digest" == "$expected_esp_rtos_scheduler_digest" ]] \
  || fail "esp-rtos scheduler deletion source changed: $actual_scheduler_digest"
[[ "$actual_task_digest" == "$expected_esp_rtos_task_digest" ]] \
  || fail "esp-rtos task deletion source changed: $actual_task_digest"
[[ "$actual_timer_digest" == "$expected_esp_rtos_timer_digest" ]] \
  || fail "esp-rtos task sleep source changed: $actual_timer_digest"

expected_driver_manifest="$driver_root/Cargo.toml"
resolved_driver_manifest="$(cargo metadata --locked --format-version 1 2>/dev/null | python3 -c '
import json, os, sys
packages = [p for p in json.load(sys.stdin)["packages"] if p["name"] == "esp-radio-rtos-driver" and p["version"] == "0.3.0"]
if len(packages) != 1:
    raise SystemExit(2)
print(os.path.realpath(packages[0]["manifest_path"]))
')" || fail "could not resolve exactly one esp-radio-rtos-driver 0.3.0 source"
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

echo "BLE controller patch check passed"
