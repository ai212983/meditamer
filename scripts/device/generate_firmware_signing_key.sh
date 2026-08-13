#!/usr/bin/env bash

set -euo pipefail

if (( $# != 1 )); then
    echo "usage: $0 <private-seed-path>" >&2
    exit 2
fi

key_path="$1"
if [[ -e "$key_path" ]]; then
    echo "refusing to overwrite existing signing key: $key_path" >&2
    exit 1
fi

mkdir -p "$(dirname "$key_path")"
openssl rand 32 >"$key_path"
chmod 600 "$key_path"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
host_target="$(rustup run stable rustc -vV | awk '/^host:/ {print $2}')"
RUSTUP_TOOLCHAIN=stable cargo run \
    --quiet \
    --manifest-path "$repo_root/tools/hostctl/Cargo.toml" \
    --target "$host_target" \
    -- firmware-key --key "$key_path"
