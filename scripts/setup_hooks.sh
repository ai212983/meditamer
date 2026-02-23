#!/usr/bin/env bash

set -euo pipefail

if ! command -v lefthook >/dev/null 2>&1; then
  cat >&2 <<'EOF'
lefthook is required to install repository-managed git hooks.
Install it with one of:
  brew install lefthook
  cargo install --locked lefthook
EOF
  exit 1
fi

lefthook install
echo "Installed git hooks from lefthook.yml"
