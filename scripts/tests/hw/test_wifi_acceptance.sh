#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../../lib/serial_port.sh
source "$script_dir/../../lib/serial_port.sh"
# shellcheck source=../../lib/experiment_novelty_guard.sh
source "$script_dir/../../lib/experiment_novelty_guard.sh"
load_repo_env_file_if_present ".env.local"
ensure_hostctl_net_port "test_wifi_acceptance.sh"
enforce_wifi_upload_experiment_novelty_guard "test_wifi_acceptance.sh"

reject_legacy_env_vars "test_wifi_acceptance.sh" \
    HOSTCTL_PORT \
    HOSTCTL_BAUD

required=(
    HOSTCTL_NET_PORT
    HOSTCTL_NET_BAUD
    HOSTCTL_NET_SSID
    HOSTCTL_NET_PASSWORD
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
"$script_dir/../../hostctl.sh" "${args[@]}"
