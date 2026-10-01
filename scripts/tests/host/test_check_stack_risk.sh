#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../../.." && pwd)"
fixture_rel="products/meditamer/src/firmware/_stack_risk_test_fixture.rs"
fixture_path="$repo_root/$fixture_rel"
inkplate_fixture_rel="targets/meditamer-inkplate/src/_stack_risk_test_fixture.rs"
inkplate_fixture_path="$repo_root/$inkplate_fixture_rel"
fixture_tools="$(mktemp -d)"

cleanup() {
  rm -f "$fixture_path" "$inkplate_fixture_path"
  rm -r "$fixture_tools"
}
trap cleanup EXIT

cat >"$fixture_path" <<'RUST'
pub fn stack_risk_fixture_violation() {
    let _buffer: [u8; 5000] = [0; 5000];
    let _ = _buffer.len();
}
RUST

if "$repo_root/scripts/ci/check_stack_risk.sh" >/dev/null 2>&1; then
  echo "expected check_stack_risk.sh to fail for oversized local array" >&2
  exit 1
fi

STACK_RISK_LOCAL_ARRAY_MAX_BYTES=6000 "$repo_root/scripts/ci/check_stack_risk.sh" >/dev/null

cat >"$inkplate_fixture_path" <<'RUST'
const CHUNK_BYTES: usize = 5000;

pub fn stack_risk_inkplate_fixture_violation() {
    let _buffer = [0u8; CHUNK_BYTES];
    let _ = _buffer.len();
}
RUST

if "$repo_root/scripts/ci/check_stack_risk.sh" >/dev/null 2>&1; then
  echo "expected check_stack_risk.sh to scan Inkplate target modules and resolve constant array sizes" >&2
  exit 1
fi

rm -f "$inkplate_fixture_path"

cat >"$fixture_path" <<'RUST'
pub fn stack_risk_fixture_reviewed() {
    let _buffer: [u8; 5000] = [0; 5000]; // stack-risk-reviewed
    let _ = _buffer.len();
}
RUST

"$repo_root/scripts/ci/check_stack_risk.sh" >/dev/null

cat >"$fixture_path" <<'RUST'
static mut HEAP_BACKING: [u8; 5000] = [0; 5000];

pub fn heap_array_fixture() {
    let _buffer = alloc::vec![0u8; 5000];
    let _ = _buffer.len();
}
RUST

"$repo_root/scripts/ci/check_stack_risk.sh" >/dev/null

fake_elf="$fixture_tools/medinote.elf"
fake_nm="$fixture_tools/xtensa-nm"
fake_objdump="$fixture_tools/xtensa-objdump"
: >"$fake_elf"

cat >"$fake_nm" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' \
  '00001000 00000001 T main' \
  '00001100 00000001 T <esp_rtos::embassy::Executor>::run_inner::<medinote_waveshare::__xtensa_lx_rt_main::{closure#0}, <esp_rtos::embassy::Executor>::run::NoHooks>' \
  '00001800 00000001 T medinote_waveshare::cheertok::report_storage' \
  '00001d00 00000001 T medinote_waveshare::cheertok::poll_live_session::<ble::live_connect::runtime::run_with_resources::{closure#0}>' \
  '00001300 00000001 T ble::live_connect::runtime::run_with_resources::{closure#0}' \
  '00001e00 00000001 T ble::live_connect::runtime::run_session::{closure#0}' \
  '00001f00 00000001 T ble::live_connect::runtime::initialize_stack' \
  '00001400 00000001 T <embassy_executor::raw::TaskStorage<medinote_waveshare::runtime_ui::__runtime_ui_task_task::__runtime_ui_task_task_inner_function::{closure#0}>>::poll' \
  '00001500 00000001 T <medinote_waveshare::runtime_ui::hourglass_runtime::HourglassRuntime>::tick' \
  "00001700 00001380 ${FAKE_REPORT_TYPE:-B} medinote_waveshare::cheertok::__input_task_task::__input_task_task_inner_function::{closure#0}::REPORT"
if [[ "${FAKE_RADIO_COMPOSITION:-input}" == "input" || "${FAKE_RADIO_COMPOSITION:-input}" == "both" ]]; then
  printf '%s\n' '00001200 00000001 T <embassy_executor::raw::TaskStorage<medinote_waveshare::cheertok::__input_task_task::__input_task_task_inner_function::{closure#0}>>::poll'
fi
if [[ "${FAKE_RADIO_COMPOSITION:-input}" == "combined" || "${FAKE_RADIO_COMPOSITION:-input}" == "both" ]]; then
  printf '%s\n' \
    '00002100 00000001 T <embassy_executor::raw::TaskStorage<medinote_waveshare::net_host::__radio_supervisor_task_task::__radio_supervisor_task_task_inner_function::{closure#0}>>::poll' \
    '00002200 00000001 T <core::pin::Pin<&mut <medinote_waveshare::net_host::CheerTokOffService as netstack::owner::RadioOffService>::run::{closure#0}> as core::future::future::Future>::poll'
  if [[ "${FAKE_NM_MODE:-}" != "missing-radio-supervisor" ]]; then
    printf '%s\n' '00002300 00000001 T netstack::owner::run_radio_supervisor_inner::<medinote_waveshare::net_host::Host, medinote_waveshare::net_host::CheerTokOffService>::{closure#0}'
  fi
fi
if [[ "${FAKE_RADIO_COMPOSITION:-input}" == "input" && "${FAKE_NM_MODE:-}" != "inline-cheertok-inner" ]]; then
  printf '%s\n' '00001c00 00000001 T medinote_waveshare::cheertok::__input_task_task::__input_task_task_inner_function::{closure#0}'
fi
if [[ "${FAKE_NM_MODE:-}" != "missing-hourglass-adapter" ]]; then
  printf '%s\n' '00001600 00000001 T <hourglass::paper_backend::ParticleStore>::step'
fi
if [[ "${FAKE_NM_MODE:-}" != "missing-hourglass-core" ]]; then
  printf '%s\n' '00001b00 00000001 T <hourglass::paper_cellular::PaperCellular>::step_blocks'
fi
if [[ "${FAKE_NM_MODE:-}" != "missing-inkplate-display" ]]; then
  printf '%s\n' '00001900 00000001 T <meditamer_product::firmware::display::state::DisplayLoopState>::allocate'
fi
if [[ "${FAKE_NM_MODE:-}" != "missing-inkplate-runner" ]]; then
  printf '%s\n' '00001a00 00000001 T meditamer_product::firmware::storage::sd_task::runtime_loop::allocate_runner'
fi
printf '%s\n' '00002000 00000001 T meditamer_product::firmware::ui::lvgl::backend::init::allocate_draw_buffer'
SH

cat >"$fake_objdump" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
if [[ " $* " == *' -h '* ]]; then
  printf '%s\n' ' 22 .stack        0000c7a8  3fccef58  3fccef58  00000000  2**2'
  exit 0
fi
address=""
for arg in "$@"; do
  case "$arg" in
    --start-address=0x*) address="${arg#--start-address=0x}" ;;
  esac
done
case "$address" in
  00001000) frame=400 ;;
  00001100) frame=96 ;;
  00001200) frame=112 ;;
  00001800) frame="${FAKE_REPORT_FRAME:-32}" ;;
  00001300) frame=0x7f00 ;;
  00001c00) frame=64 ;;
  00001d00) frame=32 ;;
  00001e00) frame=64 ;;
  00001f00) frame=48 ;;
  00001400) frame=0x740 ;;
  00001500) frame=224 ;;
  00001600) frame=32 ;;
  00001b00) frame=208 ;;
  00001900) frame="${FAKE_INKPLATE_DISPLAY_FRAME:-80}" ;;
  00001a00) frame="${FAKE_INKPLATE_RUNNER_FRAME:-64}" ;;
  00002000) frame=64 ;;
  00002100) frame=64 ;;
  00002200) frame="${FAKE_OFF_SERVICE_FRAME:-128}" ;;
  00002300) frame="${FAKE_RADIO_SUPERVISOR_FRAME:-2288}" ;;
  *) exit 1 ;;
esac
printf '%s\n' "${address}: entry a1, ${frame}"
SH
chmod +x "$fake_nm" "$fake_objdump"

elf_gate=(env XTENSA_NM="$fake_nm" XTENSA_OBJDUMP="$fake_objdump" \
  "$repo_root/scripts/ci/check_stack_risk.sh" --medinote-elf "$fake_elf")
"${elf_gate[@]}" >/dev/null
FAKE_NM_MODE=inline-cheertok-inner "${elf_gate[@]}" >/dev/null
FAKE_RADIO_COMPOSITION=combined "${elf_gate[@]}" >/dev/null

for composition in both none; do
  if FAKE_RADIO_COMPOSITION="$composition" "${elf_gate[@]}" >/dev/null 2>&1; then
    echo "expected Medinote ELF gate to reject radio composition ${composition}" >&2
    exit 1
  fi
done
if FAKE_RADIO_COMPOSITION=combined FAKE_NM_MODE=missing-radio-supervisor "${elf_gate[@]}" >/dev/null 2>&1; then
  echo "expected Medinote ELF gate to reject a missing supervisor frame" >&2
  exit 1
fi
if FAKE_RADIO_COMPOSITION=combined FAKE_RADIO_SUPERVISOR_FRAME=4112 "${elf_gate[@]}" >/dev/null 2>&1; then
  echo "expected Medinote ELF gate to reject an oversized supervisor frame" >&2
  exit 1
fi
if FAKE_RADIO_COMPOSITION=combined FAKE_OFF_SERVICE_FRAME=960 "${elf_gate[@]}" >/dev/null 2>&1; then
  echo "expected Medinote ELF gate to reject an oversized aggregate BLE caller frame" >&2
  exit 1
fi
if FAKE_RADIO_COMPOSITION=combined MEDINOTE_STACK_PATH_MARGIN_BYTES=13000 "${elf_gate[@]}" >/dev/null 2>&1; then
  echo "expected Medinote ELF gate to count the supervisor in the modeled BLE path" >&2
  exit 1
fi

if MEDINOTE_LINKED_STACK_FLOOR_BYTES=52000 "${elf_gate[@]}" >/dev/null 2>&1; then
  echo "expected Medinote ELF gate to reject a linked-stack floor regression" >&2
  exit 1
fi
if MEDINOTE_STACK_PATH_MARGIN_BYTES=14000 "${elf_gate[@]}" >/dev/null 2>&1; then
  echo "expected Medinote ELF gate to reject an insufficient modeled-path margin" >&2
  exit 1
fi
if FAKE_NM_MODE=missing-hourglass-adapter "${elf_gate[@]}" >/dev/null 2>&1; then
  echo "expected Medinote ELF gate to reject a missing hourglass adapter symbol" >&2
  exit 1
fi
if FAKE_NM_MODE=missing-hourglass-core "${elf_gate[@]}" >/dev/null 2>&1; then
  echo "expected Medinote ELF gate to reject a missing hourglass core symbol" >&2
  exit 1
fi
if MEDINOTE_LINKED_STACK_FLOOR_BYTES=invalid "${elf_gate[@]}" >/dev/null 2>&1; then
  echo "expected Medinote ELF gate to reject a malformed threshold" >&2
  exit 1
fi
if FAKE_REPORT_TYPE=D "${elf_gate[@]}" >/dev/null 2>&1; then
  echo "expected Medinote ELF gate to reject initialized report storage" >&2
  exit 1
fi
if FAKE_REPORT_FRAME=512 "${elf_gate[@]}" >/dev/null 2>&1; then
  echo "expected Medinote ELF gate to reject a report initializer frame regression" >&2
  exit 1
fi

inkplate_elf_gate=(env XTENSA_NM="$fake_nm" XTENSA_OBJDUMP="$fake_objdump" \
  "$repo_root/scripts/ci/check_stack_risk.sh" --inkplate-elf "$fake_elf")
"${inkplate_elf_gate[@]}" >/dev/null

if FAKE_INKPLATE_DISPLAY_FRAME=144 "${inkplate_elf_gate[@]}" >/dev/null 2>&1; then
  echo "expected Inkplate ELF gate to reject a display allocation frame regression" >&2
  exit 1
fi
if FAKE_INKPLATE_RUNNER_FRAME=144 "${inkplate_elf_gate[@]}" >/dev/null 2>&1; then
  echo "expected Inkplate ELF gate to reject an SD runner allocation frame regression" >&2
  exit 1
fi
if FAKE_NM_MODE=missing-inkplate-runner "${inkplate_elf_gate[@]}" >/dev/null 2>&1; then
  echo "expected Inkplate ELF gate to reject a missing allocation symbol" >&2
  exit 1
fi
if INKPLATE_EXTERNAL_ALLOC_FRAME_MAX_BYTES=invalid "${inkplate_elf_gate[@]}" >/dev/null 2>&1; then
  echo "expected Inkplate ELF gate to reject a malformed threshold" >&2
  exit 1
fi

echo "stack-risk fixture tests passed"
