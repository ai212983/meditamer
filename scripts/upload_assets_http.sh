#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=./lib/run_hostctl.sh
source "$script_dir/lib/run_hostctl.sh"

reject_legacy_env_vars "upload_assets_http.sh" \
    UPLOAD_TOKEN \
    UPLOAD_CHUNK_SIZE \
    UPLOAD_SD_BUSY_TOTAL_RETRY_SEC \
    UPLOAD_NET_RECOVERY_TIMEOUT_SEC \
    UPLOAD_NET_RECOVERY_POLL_SEC \
    UPLOAD_CONNECT_TIMEOUT_SEC \
    UPLOAD_SKIP_MKDIR \
    UPLOAD_TRACE_REQUESTS

host=""
port="8080"
src=""
dst="/assets"
timeout="60"
token=""
rms=()

while [[ $# -gt 0 ]]; do
    case "$1" in
        --host)
            host="$2"
            shift 2
            ;;
        --port)
            port="$2"
            shift 2
            ;;
        --src)
            src="$2"
            shift 2
            ;;
        --dst)
            dst="$2"
            shift 2
            ;;
        --timeout)
            timeout="$2"
            shift 2
            ;;
        --token)
            token="$2"
            shift 2
            ;;
        --rm)
            rms+=("$2")
            shift 2
            ;;
        *)
            echo "Unknown argument: $1" >&2
            exit 1
            ;;
    esac
done

if [[ -z "$host" ]]; then
    echo "--host is required" >&2
    exit 1
fi

args=(upload --host "$host" --port "$port" --dst "$dst" --timeout "$timeout")
if [[ -n "$src" ]]; then
    args+=(--src "$src")
fi
if [[ -n "$token" ]]; then
    args+=(--token "$token")
fi
for rm in "${rms[@]}"; do
    args+=(--rm "$rm")
done

run_hostctl "${args[@]}"
