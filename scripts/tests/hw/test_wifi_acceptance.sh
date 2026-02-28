#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../../lib/run_hostctl.sh
source "$script_dir/../../lib/run_hostctl.sh"

reject_legacy_env_vars "test_wifi_acceptance.sh" \
    HOSTCTL_PORT \
    HOSTCTL_BAUD \
    HOSTCTL_WIFI_UPLOAD_CYCLES \
    HOSTCTL_WIFI_UPLOAD_PAYLOAD_BYTES \
    HOSTCTL_WIFI_UPLOAD_CONNECT_TIMEOUT_SEC \
    HOSTCTL_WIFI_UPLOAD_LISTEN_TIMEOUT_SEC \
    HOSTCTL_WIFI_UPLOAD_HTTP_TIMEOUT_SEC \
    HOSTCTL_WIFI_UPLOAD_HEALTH_TIMEOUT_SEC \
    HOSTCTL_WIFI_UPLOAD_STAT_TIMEOUT_MS \
    HOSTCTL_WIFI_UPLOAD_IP_DISCOVERY_TIMEOUT_SEC \
    HOSTCTL_WIFI_UPLOAD_OPERATION_RETRIES \
    HOSTCTL_WIFI_UPLOAD_REMOTE_ROOT \
    HOSTCTL_WIFI_UPLOAD_PAYLOAD_PATH \
    HOSTCTL_WIFI_UPLOAD_SSID \
    HOSTCTL_WIFI_UPLOAD_PASSWORD \
    HOSTCTL_WIFI_UPLOAD_LOCK_PATH \
    HOSTCTL_WIFI_UPLOAD_TEST_NAME \
    HOSTCTL_WIFI_UPLOAD_DHCP_TIMEOUT_MS \
    HOSTCTL_WIFI_UPLOAD_PINNED_DHCP_TIMEOUT_MS

required=(
    HOSTCTL_NET_PORT
    HOSTCTL_NET_BAUD
    HOSTCTL_NET_SSID
    HOSTCTL_NET_PASSWORD
    HOSTCTL_NET_POLICY_PATH
    HOSTCTL_NET_LOG_PATH
)
for name in "${required[@]}"; do
    if [[ -z "${!name:-}" ]]; then
        echo "test_wifi_acceptance.sh: missing required env var: $name" >&2
        exit 1
    fi
done

args=(test wifi-acceptance)
if [[ -n "${1:-}" ]]; then
    args+=("$1")
fi
run_hostctl "${args[@]}"
