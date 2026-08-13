#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../../.." && pwd)"
fixture_root="$(mktemp -d)"
trap 'rm -rf "$fixture_root"' EXIT

mkdir -p "$fixture_root/bin" "$fixture_root/config"

cat >"$fixture_root/bin/rust-code-analysis-cli" <<'SCRIPT'
#!/usr/bin/env bash
set -euo pipefail
exec /bin/cat "$RCA_TEST_METRICS_FILE"
SCRIPT
chmod +x "$fixture_root/bin/rust-code-analysis-cli"

write_metrics() {
  local name="$1"
  local sloc="$2"
  cat >"$fixture_root/metrics.json" <<JSON
{"name":"$name","metrics":{"loc":{"sloc":$sloc}},"spaces":[]}
JSON
}

write_baseline() {
  local name="${1:-}"
  local sloc="${2:-0}"
  local file_sloc='{}'
  if [[ -n "$name" ]]; then
    file_sloc="{\"$name\":$sloc}"
  fi
  cat >"$fixture_root/config/rca-baseline.json" <<JSON
{
  "version": 1,
  "thresholds": {
    "max_file_sloc": 1000,
    "warn_file_sloc": 600,
    "max_fn_cognitive": 40,
    "max_fn_cyclomatic": 32,
    "max_fn_nargs": 8
  },
  "offenders": {
    "file_sloc": $file_sloc,
    "fn_cognitive": {},
    "fn_cyclomatic": {},
    "fn_nargs": {}
  }
}
JSON
}

run_guard() {
  env \
    PATH="$fixture_root/bin:$PATH" \
    RCA_REPO_ROOT="$fixture_root" \
    RCA_TEST_METRICS_FILE="$fixture_root/metrics.json" \
    RCA_TOP_N=1 \
    RCA_ENFORCE=1 \
    RCA_RATCHET=1 \
    "$repo_root/scripts/ci/lint_code_analysis.sh"
}

write_baseline
write_metrics 'src/large.rs' 1001
if run_guard >"$fixture_root/new.out" 2>&1; then
  echo "expected a new file above 1000 SLOC to fail" >&2
  exit 1
fi
grep -q 'file_sloc new' "$fixture_root/new.out"

write_baseline 'src/large.rs' 1001
run_guard >/dev/null

write_metrics 'src/large.rs' 1002
if run_guard >"$fixture_root/regressed.out" 2>&1; then
  echo "expected a baselined file that grew to fail" >&2
  exit 1
fi
grep -q 'file_sloc regressed' "$fixture_root/regressed.out"

write_baseline
write_metrics 'src/tests/large.rs' 5000
run_guard >/dev/null

write_metrics 'src/large.rs' 1001
if env \
  PATH="$fixture_root/bin:$PATH" \
  RCA_REPO_ROOT="$fixture_root" \
  RCA_TEST_METRICS_FILE="$fixture_root/metrics.json" \
  RCA_TOP_N=1 \
  RCA_ENFORCE=1 \
  RCA_RATCHET=0 \
  "$repo_root/scripts/ci/lint_code_analysis.sh" >/dev/null 2>&1; then
  echo "expected strict non-ratchet mode to enforce file SLOC" >&2
  exit 1
fi

echo "code-analysis guard fixture tests passed"
