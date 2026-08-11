#!/usr/bin/env bash

set -euo pipefail

if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  echo "check_orphan_modules.sh: must run inside a git work tree" >&2
  exit 2
fi

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(git rev-parse --show-toplevel)"

exec python3 "$script_dir/check_orphan_modules.py" --repo-root "$repo_root"
