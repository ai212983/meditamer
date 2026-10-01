#!/usr/bin/env bash
# Build Inkplate firmware; the shared driver also supplies Clippy's composition.
set -euo pipefail
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
# shellcheck source=../../scripts/lib/inkplate_cargo.sh
source "$repo_root/scripts/lib/inkplate_cargo.sh"
inkplate_cargo build "$@"
