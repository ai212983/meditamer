#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
patch_root="$repo_root/vendor/esp-radio-1.0.0-beta.0-bounded"
ble_mod="$patch_root/src/ble/mod.rs"
btdm="$patch_root/src/ble/btdm.rs"
controller="$patch_root/src/ble/controller/mod.rs"
tx_cancellation="$patch_root/src/ble/tx_cancellation.rs"
expected_tree_digest="4019a3738d1b312acd55030b26ac41691ebecb592bd1b3c0f91285b63d403a93"

fail() {
  echo "BLE controller patch check failed: $*" >&2
  exit 1
}

for source in "$ble_mod" "$btdm" "$controller" "$tx_cancellation"; do
  [[ -f "$source" ]] || fail "missing ${source#"$repo_root/"}"
done

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

echo "BLE controller patch check passed"
