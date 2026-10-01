#!/usr/bin/env bash

set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../../.." && pwd)"
fixture_root="$(mktemp -d)"
trap 'rm -rf "$fixture_root"' EXIT

mkdir -p "$fixture_root/bin" "$fixture_root/config" "$fixture_root/src"

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
    RCA_SCAN_ROOTS="src" \
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
  RCA_SCAN_ROOTS="src" \
  RCA_TEST_METRICS_FILE="$fixture_root/metrics.json" \
  RCA_TOP_N=1 \
  RCA_ENFORCE=1 \
  RCA_RATCHET=0 \
  "$repo_root/scripts/ci/lint_code_analysis.sh" >/dev/null 2>&1; then
  echo "expected strict non-ratchet mode to enforce file SLOC" >&2
  exit 1
fi

# A configured scan root that does not exist must fail loud, not have
# `rust-code-analysis-cli` silently treat it as zero files -- this is
# exactly how `platform/ui/render` went unscanned before this fixture's
# `RCA_SCAN_ROOTS` override existed (ui-runtime-finalization plan, Phase 7).
write_baseline
write_metrics 'src/large.rs' 1
if env \
  PATH="$fixture_root/bin:$PATH" \
  RCA_REPO_ROOT="$fixture_root" \
  RCA_SCAN_ROOTS="src missing-root" \
  RCA_TEST_METRICS_FILE="$fixture_root/metrics.json" \
  RCA_TOP_N=1 \
  RCA_ENFORCE=1 \
  RCA_RATCHET=1 \
  "$repo_root/scripts/ci/lint_code_analysis.sh" >"$fixture_root/missing_root.out" 2>&1; then
  echo "expected a missing scan root to fail loud" >&2
  exit 1
fi
grep -q 'scan root does not exist: missing-root' "$fixture_root/missing_root.out"

# Per-callable nargs guard: rust-code-analysis reports nargs.total as the sum
# of a function's own parameters plus every closure nested in its body, so the
# lint judges each callable by max(functions_max, closures_max) instead.
write_nargs_metrics() {
  # args: fn_name parent_fn_max parent_cl_max parent_total
  #       child_cl_max child_total child_count child_name
  jq -n \
    --arg fn_name "$1" \
    --argjson fn_max "$2" \
    --argjson cl_max "$3" \
    --argjson total "$4" \
    --argjson child_cl_max "$5" \
    --argjson child_total "$6" \
    --argjson child_count "$7" \
    --arg child_name "$8" \
    '{
      name: "src/nargs.rs",
      metrics: {loc: {sloc: 10}},
      spaces: [
        {
          kind: "function",
          name: $fn_name,
          start_line: 1,
          end_line: 50,
          metrics: {
            cognitive: {max: 1},
            cyclomatic: {max: 1},
            nargs: {total: $total, functions_max: $fn_max, closures_max: $cl_max}
          },
          spaces: (
            [range(0; $child_count) | {
              kind: "function",
              name: $child_name,
              start_line: 2,
              end_line: 3,
              metrics: {
                cognitive: {max: 0},
                cyclomatic: {max: 0},
                nargs: {total: $child_total, functions_max: 0, closures_max: $child_cl_max}
              },
              spaces: []
            }]
          )
        }
      ]
    }' >"$fixture_root/metrics.json"
}

# Ten 1-param closures inside a 3-param function aggregate to nargs.total 13
# (https_memory_probe `request`); per-callable max is 3, so this must pass.
write_baseline
write_nargs_metrics 'request' 3 1 13 1 1 10 '<anonymous>'
run_guard >/dev/null

# A genuine 9-parameter function must still fail.
write_baseline
write_nargs_metrics 'big_fn' 9 0 9 0 0 0 '<anonymous>'
if run_guard >"$fixture_root/nargs_fn.out" 2>&1; then
  echo "expected a 9-parameter function to fail the nargs gate" >&2
  exit 1
fi
grep -q 'fn_nargs new' "$fixture_root/nargs_fn.out"
grep -q 'src/nargs.rs::big_fn' "$fixture_root/nargs_fn.out"

# A genuine 9-parameter closure inside a 2-parameter function must still fail,
# with the closure itself reported.
write_baseline
write_nargs_metrics 'wrapper' 2 9 11 9 9 1 '<anonymous>'
if run_guard >"$fixture_root/nargs_closure.out" 2>&1; then
  echo "expected a 9-parameter closure to fail the nargs gate" >&2
  exit 1
fi
grep -q 'fn_nargs new' "$fixture_root/nargs_closure.out"
grep -q 'src/nargs.rs::<anonymous>' "$fixture_root/nargs_closure.out"

echo "code-analysis guard fixture tests passed"
