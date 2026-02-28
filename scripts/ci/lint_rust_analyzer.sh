#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"

ra_bin="${RUST_ANALYZER_BIN:-}"
if [[ -z "$ra_bin" ]]; then
  if command -v rust-analyzer >/dev/null 2>&1; then
    ra_bin="$(command -v rust-analyzer)"
  else
    echo >&2 "rust-analyzer binary not found."
    echo >&2 "Install component: rustup component add rust-analyzer"
    exit 1
  fi
fi

echo "rust-analyzer baseline"
echo "binary: $ra_bin"
ra_toolchain="${RUST_ANALYZER_TOOLCHAIN:-stable}"
echo "toolchain: $ra_toolchain"

declare -a common_args=(
  --disable-build-scripts
  --disable-proc-macros
)

(
  cd "$repo_root"
  RUSTUP_TOOLCHAIN="$ra_toolchain" \
    "$ra_bin" -q analysis-stats . "${common_args[@]}" --skip-inference --no-sysroot
)

# Diagnostics are run in lightweight mode and summarized by class for signal over noise.
tmp_diag="$(mktemp)"
trap 'rm -f "$tmp_diag"' EXIT
(
  cd "$repo_root"
  set +e
  RUSTUP_TOOLCHAIN="$ra_toolchain" \
    "$ra_bin" -q diagnostics . "${common_args[@]}" >"$tmp_diag" 2>&1
  rc=$?
  set -e
  if [[ $rc -ne 0 ]]; then
    echo "diagnostics exited with code $rc (continuing with summary)"
  fi
)

echo
echo "rust-analyzer diagnostics summary"
if command -v rg >/dev/null 2>&1; then
  rg -o 'RustcHardError\("E[0-9]+"\)|RustcHardError\("[a-z-]+"\)|Ra\("[a-z-]+", Error\)' "$tmp_diag" \
    | sort \
    | uniq -c \
    | sort -nr \
    || true
else
  grep -Eo 'RustcHardError\("E[0-9]+"\)|RustcHardError\("[a-z-]+"\)|Ra\("[a-z-]+", Error\)' "$tmp_diag" \
    | sort \
    | uniq -c \
    | sort -nr \
    || true
fi
