#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

integer_setting() {
    local name="$1"
    local value="$2"
    if ! [[ "$value" =~ ^[0-9]+$ ]]; then
        echo "check_stack_risk.sh: ${name} must be an integer" >&2
        exit 1
    fi
}

find_xtensa_tool() {
    local configured="$1"
    local name="$2"
    if [[ -n "$configured" ]]; then
        printf '%s\n' "$configured"
        return
    fi
    local candidate
    candidate="$(command -v "xtensa-esp-elf-${name}" || true)"
    if [[ -z "$candidate" ]]; then
        candidate="$(command -v "xtensa-esp32-elf-${name}" || true)"
    fi
    if [[ -z "$candidate" ]] && command -v rustup >/dev/null 2>&1; then
        local rust_sysroot
        rust_sysroot="$(rustup run esp rustc --print sysroot 2>/dev/null || true)"
        if [[ -n "$rust_sysroot" ]]; then
            candidate="$(find "$rust_sysroot" -type f \
                \( -name "xtensa-esp-elf-${name}" -o -name "xtensa-esp32-elf-${name}" \) \
                -print -quit 2>/dev/null || true)"
        fi
    fi
    printf '%s\n' "$candidate"
}

run_inkplate_elf_gate() {
    local elf="$1"
    if [[ ! -f "$elf" ]]; then
        echo "stack-risk: missing Inkplate ELF: $elf" >&2
        exit 1
    fi

    local frame_ceiling="${INKPLATE_EXTERNAL_ALLOC_FRAME_MAX_BYTES:-128}"
    integer_setting INKPLATE_EXTERNAL_ALLOC_FRAME_MAX_BYTES "$frame_ceiling"

    local objdump nm
    objdump="$(find_xtensa_tool "${XTENSA_OBJDUMP:-}" objdump)"
    nm="$(find_xtensa_tool "${XTENSA_NM:-}" nm)"
    if [[ -z "$objdump" || ! -x "$objdump" || -z "$nm" || ! -x "$nm" ]]; then
        echo "stack-risk: Xtensa objdump and nm are required for the Inkplate ELF gate" >&2
        exit 1
    fi

    local symbols
    symbols="$(mktemp)"
    trap 'rm -f "$symbols"' RETURN
    "$nm" -S -C "$elf" >"$symbols"

    frame_bytes() {
        local label="$1"
        local suffix="$2"
        local matches count address stop operand bytes
        matches="$(awk -v suffix="$suffix" '
            length($0) >= length(suffix) && substr($0, length($0) - length(suffix) + 1) == suffix {
                print $1
            }
        ' "$symbols")"
        count="$(printf '%s\n' "$matches" | sed '/^$/d' | wc -l | tr -d '[:space:]')"
        if [[ "$count" != "1" ]]; then
            echo "stack-risk: expected one Inkplate ${label} symbol, found ${count}" >&2
            exit 1
        fi
        address="$matches"
        stop="$((16#$address + 16))"
        operand="$($objdump -d -C --start-address="0x${address}" --stop-address="$stop" "$elf" \
            | awk '/entry[[:space:]]+a1,/ { print $NF; exit }')"
        if [[ -z "$operand" ]]; then
            echo "stack-risk: Inkplate ${label} has no Xtensa entry frame" >&2
            exit 1
        fi
        if [[ "$operand" == 0x* ]]; then
            bytes="$((16#${operand#0x}))"
        else
            bytes="$operand"
        fi
        integer_setting "Inkplate ${label} frame" "$bytes"
        if (( bytes > frame_ceiling )); then
            echo "stack-risk: Inkplate ${label} frame ${bytes} exceeds ${frame_ceiling}" >&2
            exit 1
        fi
        printf '%s\n' "$bytes"
    }

    local display_frame runner_frame draw_frame
    display_frame="$(frame_bytes display-allocation ' <meditamer_product::firmware::display::state::DisplayLoopState>::allocate')"
    runner_frame="$(frame_bytes sd-runner-allocation ' meditamer_product::firmware::storage::sd_task::runtime_loop::allocate_runner')"
    draw_frame="$(frame_bytes draw-buffer-allocation ' meditamer_product::firmware::ui::lvgl::backend::init::allocate_draw_buffer')"
    # Opt-in trace storage is large enough that a hidden return-value temporary
    # can consume nearly the entire CPU0 stack. Check the complete allocator
    # body as well as its entry: Xtensa emits movsp for oversized frames.
    local trace_symbol=' meditamer_product::firmware::trace::allocate_collector'
    if rg -q -F "$trace_symbol" "$symbols"; then
        local trace_frame trace_address trace_size trace_body
        trace_frame="$(frame_bytes trace-allocation "$trace_symbol")"
        read -r trace_address trace_size < <(awk -v suffix="$trace_symbol" '
            substr($0, length($0) - length(suffix) + 1) == suffix { print $1, $2 }
        ' "$symbols")
        trace_body="$($objdump -d -C --start-address="0x${trace_address}" \
            --stop-address="$((16#$trace_address + 16#$trace_size))" "$elf")"
        if rg -q '[[:space:]]movsp[[:space:]]' <<<"$trace_body"; then
            echo 'stack-risk: trace allocation contains oversized dynamic stack adjustment'
            exit 1
        fi
        echo "stack-risk: trace allocation passed frame=${trace_frame} no_dynamic_adjustment=true"
    fi
    echo "stack-risk: Inkplate ELF passed display_allocation=${display_frame} sd_runner_allocation=${runner_frame} draw_buffer_allocation=${draw_frame}"
}

run_medinote_elf_gate() {
    local elf="$1"
    if [[ ! -f "$elf" ]]; then
        echo "stack-risk: missing Medinote ELF: $elf" >&2
        exit 1
    fi

    local stack_floor="${MEDINOTE_LINKED_STACK_FLOOR_BYTES:-50000}"
    local path_margin="${MEDINOTE_STACK_PATH_MARGIN_BYTES:-8192}"
    local ble_allowance="${MEDINOTE_BLE_OTHER_FRAMES_BYTES:-4096}"
    local hourglass_allowance="${MEDINOTE_HOURGLASS_OTHER_FRAMES_BYTES:-4096}"
    integer_setting MEDINOTE_LINKED_STACK_FLOOR_BYTES "$stack_floor"
    integer_setting MEDINOTE_STACK_PATH_MARGIN_BYTES "$path_margin"
    integer_setting MEDINOTE_BLE_OTHER_FRAMES_BYTES "$ble_allowance"
    integer_setting MEDINOTE_HOURGLASS_OTHER_FRAMES_BYTES "$hourglass_allowance"

    local objdump nm
    objdump="$(find_xtensa_tool "${XTENSA_OBJDUMP:-}" objdump)"
    nm="$(find_xtensa_tool "${XTENSA_NM:-}" nm)"
    if [[ -z "$objdump" || ! -x "$objdump" || -z "$nm" || ! -x "$nm" ]]; then
        echo "stack-risk: Xtensa objdump and nm are required for the Medinote ELF gate" >&2
        exit 1
    fi

    local stack_hex stack_bytes
    stack_hex="$($objdump -h "$elf" | awk '$2 == ".stack" { print $3 }')"
    if [[ -z "$stack_hex" ]]; then
        echo "stack-risk: Medinote ELF has no .stack section" >&2
        exit 1
    fi
    stack_bytes="$((16#$stack_hex))"
    if (( stack_bytes < stack_floor )); then
        echo "stack-risk: Medinote linked stack ${stack_bytes} is below floor ${stack_floor}" >&2
        exit 1
    fi

    local symbols
    symbols="$(mktemp)"
    trap 'rm -f "$symbols"' RETURN
    "$nm" -S -C "$elf" >"$symbols"

    symbol_address() {
        local label="$1"
        local suffix="$2"
        local matches count
        matches="$(awk -v suffix="$suffix" '
            length($0) >= length(suffix) && substr($0, length($0) - length(suffix) + 1) == suffix {
                print $1
            }
        ' "$symbols")"
        count="$(printf '%s\n' "$matches" | sed '/^$/d' | wc -l | tr -d '[:space:]')"
        if [[ "$count" != "1" ]]; then
            echo "stack-risk: expected one ${label} symbol, found ${count}" >&2
            exit 1
        fi
        printf '%s\n' "$matches"
    }

    symbol_type() {
        local label="$1"
        local suffix="$2"
        local matches count
        matches="$(awk -v suffix="$suffix" '
            length($0) >= length(suffix) && substr($0, length($0) - length(suffix) + 1) == suffix {
                print $3
            }
        ' "$symbols")"
        count="$(printf '%s\n' "$matches" | sed '/^$/d' | wc -l | tr -d '[:space:]')"
        if [[ "$count" != "1" ]]; then
            echo "stack-risk: expected one ${label} symbol, found ${count}" >&2
            exit 1
        fi
        printf '%s\n' "$matches"
    }

    frame_bytes() {
        local label="$1"
        local suffix="$2"
        local ceiling="$3"
        local address stop operand bytes
        address="$(symbol_address "$label" "$suffix")" || return 1
        stop="$((16#$address + 16))"
        operand="$($objdump -d -C --start-address="0x${address}" --stop-address="$stop" "$elf" \
            | awk '/entry[[:space:]]+a1,/ { print $NF; exit }')"
        if [[ -z "$operand" ]]; then
            echo "stack-risk: ${label} has no Xtensa entry frame" >&2
            exit 1
        fi
        if [[ "$operand" == 0x* ]]; then
            bytes="$((16#${operand#0x}))"
        else
            bytes="$operand"
        fi
        integer_setting "${label} frame" "$bytes"
        if (( bytes > ceiling )); then
            echo "stack-risk: ${label} frame ${bytes} exceeds ${ceiling}" >&2
            exit 1
        fi
        printf '%s\n' "$bytes"
    }

    local main_frame executor_frame cheertok_frame live_connect_frame
    local radio_supervisor_frame=0 off_service_frame=0
    local cheertok_inner_frame live_poll_frame live_epoch_frame live_session_frame live_init_frame live_branch_frame
    local runtime_ui_frame hourglass_frame adapter_frame motion_frame
    main_frame="$(frame_bytes main ' main' 1024)"
    executor_frame="$(frame_bytes executor ' <esp_rtos::embassy::Executor>::run_inner::<medinote_waveshare::__xtensa_lx_rt_main::{closure#0}, <esp_rtos::embassy::Executor>::run::NoHooks>' 512)"
    # ADR-0029 removes the standalone input task from the combined image.
    # Require exactly one composition, counting the shared supervisor and
    # its BLE off-service poll in addition to the existing BLE path frames.
    local input_poll=' <embassy_executor::raw::TaskStorage<medinote_waveshare::cheertok::__input_task_task::__input_task_task_inner_function::{closure#0}>>::poll'
    local radio_poll=' <embassy_executor::raw::TaskStorage<medinote_waveshare::net_host::__radio_supervisor_task_task::__radio_supervisor_task_task_inner_function::{closure#0}>>::poll'
    local input_present radio_present
    input_present="$(awk -v suffix="$input_poll" 'length($0) >= length(suffix) && substr($0, length($0) - length(suffix) + 1) == suffix { n++ } END { print n+0 }' "$symbols")"
    radio_present="$(awk -v suffix="$radio_poll" 'length($0) >= length(suffix) && substr($0, length($0) - length(suffix) + 1) == suffix { n++ } END { print n+0 }' "$symbols")"
    if [[ "$input_present" == 1 && "$radio_present" == 0 ]]; then
        cheertok_frame="$(frame_bytes cheertok "$input_poll" 1024)"
    elif [[ "$input_present" == 0 && "$radio_present" == 1 ]]; then
        cheertok_frame="$(frame_bytes radio-task "$radio_poll" 1024)"
        radio_supervisor_frame="$(frame_bytes radio-supervisor ' netstack::owner::run_radio_supervisor_inner::<medinote_waveshare::net_host::Host, medinote_waveshare::net_host::CheerTokOffService>::{closure#0}' 4096)"
        off_service_frame="$(frame_bytes cheertok-off-service ' <core::pin::Pin<&mut <medinote_waveshare::net_host::CheerTokOffService as netstack::owner::RadioOffService>::run::{closure#0}> as core::future::future::Future>::poll' 1024)"
    else
        echo "stack-risk: expected exactly one Medinote radio task composition, found input=${input_present} supervisor=${radio_present}" >&2
        exit 1
    fi
    frame_bytes report-storage ' medinote_waveshare::cheertok::report_storage' 256 >/dev/null
    # Restartable BLE fences coroutine polling explicitly. Count every new
    # caller frame as well as the epoch/session frames; keep the original
    # aggregate live-connect ceiling and modeled path allowance unchanged.
    # The compiler may inline the task body into TaskStorage::poll. That poll
    # frame is measured above; add a separate inner frame only when the ELF
    # actually retains one. A duplicate still fails frame_bytes' unique-symbol
    # check instead of silently weakening the aggregate bound.
    local cheertok_inner_suffix=' medinote_waveshare::cheertok::__input_task_task::__input_task_task_inner_function::{closure#0}'
    if awk -v suffix="$cheertok_inner_suffix" '
        length($0) >= length(suffix) && substr($0, length($0) - length(suffix) + 1) == suffix { found = 1 }
        END { exit found ? 0 : 1 }
    ' "$symbols"; then
        cheertok_inner_frame="$(frame_bytes cheertok-inner "$cheertok_inner_suffix" 1024)"
    else
        cheertok_inner_frame=0
    fi
    live_poll_frame="$(frame_bytes live-connect-poll ' medinote_waveshare::cheertok::poll_live_session::<ble::live_connect::runtime::run_with_resources::{closure#0}>' 128)"
    cheertok_frame="$((cheertok_frame + cheertok_inner_frame + off_service_frame + live_poll_frame))"
    if (( cheertok_frame > 1024 )); then
        echo "stack-risk: aggregate cheertok frame ${cheertok_frame} exceeds 1024" >&2
        exit 1
    fi
    live_epoch_frame="$(frame_bytes live-connect-epoch ' ble::live_connect::runtime::run_with_resources::{closure#0}' 34000)"
    live_session_frame="$(frame_bytes live-connect-session ' ble::live_connect::runtime::run_session::{closure#0}' 34000)"
    live_init_frame="$(frame_bytes live-connect-init ' ble::live_connect::runtime::initialize_stack' 34000)"
    # Initialization returns before session polling; model the larger branch.
    live_branch_frame="$live_session_frame"
    if (( live_init_frame > live_branch_frame )); then
        live_branch_frame="$live_init_frame"
    fi
    live_connect_frame="$((live_epoch_frame + live_branch_frame))"
    if (( live_connect_frame > 34000 )); then
        echo "stack-risk: aggregate live-connect frame ${live_connect_frame} exceeds 34000" >&2
        exit 1
    fi
    runtime_ui_frame="$(frame_bytes runtime-ui ' <embassy_executor::raw::TaskStorage<medinote_waveshare::runtime_ui::__runtime_ui_task_task::__runtime_ui_task_task_inner_function::{closure#0}>>::poll' 4096)"
    hourglass_frame="$(frame_bytes hourglass-tick ' <medinote_waveshare::runtime_ui::hourglass_runtime::HourglassRuntime>::tick' 1024)"
    # The hourglass model moved out of the product and into its own chip-neutral
    # crate (products/medinote/src/apps/hourglass/* -> platform/ui/visuals/hourglass/src/*),
    # so these two lost their `medinote::apps::` prefix. The frames themselves
    # are unchanged -- this is a path rename, not a code change.
    adapter_frame="$(frame_bytes hourglass-adapter ' <hourglass::paper_backend::ParticleStore>::step' 256)"
    motion_frame="$(frame_bytes hourglass-motion ' <hourglass::paper_cellular::PaperCellular>::step_blocks' 1024)"

    local report_type
    report_type="$(symbol_type live-connect-report '::REPORT')"
    if [[ "$report_type" != "B" && "$report_type" != "b" ]]; then
        echo "stack-risk: Medinote live-connect report must be zero-initialized .bss, found symbol type ${report_type}" >&2
        exit 1
    fi

    local ble_path hourglass_path ble_margin hourglass_margin
    ble_path="$((main_frame + executor_frame + cheertok_frame + radio_supervisor_frame + live_connect_frame + ble_allowance))"
    hourglass_path="$((main_frame + executor_frame + runtime_ui_frame + hourglass_frame + adapter_frame + motion_frame + hourglass_allowance))"
    ble_margin="$((stack_bytes - ble_path))"
    hourglass_margin="$((stack_bytes - hourglass_path))"
    if (( ble_margin < path_margin )); then
        echo "stack-risk: modeled Medinote BLE path margin ${ble_margin} is below ${path_margin}" >&2
        exit 1
    fi
    if (( hourglass_margin < path_margin )); then
        echo "stack-risk: modeled Medinote hourglass path margin ${hourglass_margin} is below ${path_margin}" >&2
        exit 1
    fi

    echo "stack-risk: Medinote ELF passed linked_stack=${stack_bytes} ble_path=${ble_path} ble_margin=${ble_margin} hourglass_path=${hourglass_path} hourglass_margin=${hourglass_margin}"
}

if [[ "${1:-}" == "--medinote-elf" ]]; then
    if [[ "$#" != 2 ]]; then
        echo "usage: check_stack_risk.sh --medinote-elf <firmware.elf>" >&2
        exit 2
    fi
    run_medinote_elf_gate "$2"
    exit 0
fi
if [[ "${1:-}" == "--inkplate-elf" ]]; then
    if [[ "$#" != 2 ]]; then
        echo "usage: check_stack_risk.sh --inkplate-elf <firmware.elf>" >&2
        exit 2
    fi
    run_inkplate_elf_gate "$2"
    exit 0
fi
if [[ "$#" != 0 ]]; then
    echo "usage: check_stack_risk.sh [--inkplate-elf <firmware.elf> | --medinote-elf <firmware.elf>]" >&2
    exit 2
fi

threshold="${STACK_RISK_LOCAL_ARRAY_MAX_BYTES:-4096}"
integer_setting STACK_RISK_LOCAL_ARRAY_MAX_BYTES "$threshold"

cd "$repo_root"

scan_roots=(
    platform/ui/render/src
    products/meditamer/src/firmware
    products/medinote/src
    targets/meditamer-inkplate/src
    targets/medinote-waveshare/src
)

for root in "${scan_roots[@]}"; do
    if [[ ! -e "$root" ]]; then
        echo "check_stack_risk.sh: configured scan root does not exist: ${root}" >&2
        exit 1
    fi
done

files=()
if command -v rg >/dev/null 2>&1; then
    while IFS= read -r file; do
        files+=("$file")
    done < <(rg --files "${scan_roots[@]}" 2>/dev/null | rg '\.rs$' || true)
else
    while IFS= read -r file; do
        files+=("$file")
    done < <(find "${scan_roots[@]}" -maxdepth 20 -type f -name '*.rs' 2>/dev/null || true)
fi

if [[ "${#files[@]}" -eq 0 ]]; then
    echo "check_stack_risk.sh: no firmware Rust files found; skipping"
    exit 0
fi

violations=0

constant_array_size() {
    local file="$1"
    local name="$2"
    local definition

    definition="$(rg -n \
        "^[[:space:]]*(pub[[:space:]]+)?const[[:space:]]+${name}([[:space:]]*:[^=;]+)?[[:space:]]*=[[:space:]]*[0-9_]+[[:space:]]*;" \
        "$file" 2>/dev/null \
        | sed -n '1{s/.*=[[:space:]]*//; s/[[:space:];].*//; p;}' \
        | tr -d '_' || true)"

    if [[ "$definition" =~ ^[0-9]+$ ]]; then
        printf '%s\n' "$definition"
    fi
}

for file in "${files[@]}"; do
    while IFS= read -r match; do
        line_no="${match%%:*}"
        text="${match#*:}"
        if [[ "$text" == *"stack-risk-reviewed"* ]]; then
            continue
        fi
        # Static storage and vec! repetition do not create local stack arrays.
        if [[ "$text" =~ ^[[:space:]]*(pub[[:space:]]+)?static([[:space:]]+mut)?[[:space:]] ]] ||
            [[ "$text" == *"vec!["* ]]; then
            continue
        fi
        size="$(printf '%s' "$text" \
            | sed -En 's/.*\[[[:space:]]*(0[[:space:]]*)?u8[[:space:]]*;[[:space:]]*([0-9_]+)[[:space:]]*\].*/\2/p' \
            | tr -d '_')"
        if [[ -z "$size" ]]; then
            constant_name="$(printf '%s' "$text" \
                | sed -En 's/.*\[[[:space:]]*(0[[:space:]]*)?u8[[:space:]]*;[[:space:]]*([A-Za-z_][A-Za-z0-9_]*)[[:space:]]*\].*/\2/p')"
            if [[ -n "$constant_name" ]]; then
                size="$(constant_array_size "$file" "$constant_name")"
            fi
        fi
        if [[ -z "$size" || ! "$size" =~ ^[0-9]+$ ]]; then
            continue
        fi
        if (( size > threshold )); then
            echo "stack-risk: ${file}:${line_no} has [u8; ${size}] (threshold=${threshold})" >&2
            echo "  add 'stack-risk-reviewed' comment to line after manual review if this is intentional." >&2
            violations=1
        fi
    done < <(rg -n '\[[[:space:]]*(0[[:space:]]*)?u8[[:space:]]*;[[:space:]]*([0-9_]+|[A-Za-z_][A-Za-z0-9_]*)[[:space:]]*\]' "$file" || true)
done

if (( violations != 0 )); then
    exit 1
fi

echo "stack-risk: no high-risk fixed [u8; N] arrays above threshold ${threshold} detected"
